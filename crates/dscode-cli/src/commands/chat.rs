/// Mobile-first interactive chat with DeepSeek.
///
/// Thin UX layer on top of dscode engine + shared api.rs.
/// Session persistence via SQLite (codewhale-state).

use crate::api::{self, resolve_model_name, resolve_api_key, resolve_base_url};
use crate::engine::{AgentEngine, AgentOptions};
use crate::tools;
use crate::utils::{is_narrow_terminal, terminal_width};
use rpassword; // for hidden API key input on first run
use chrono::Utc;
use clap::Args;
use rustyline::DefaultEditor;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Args)]
pub struct ChatArgs {
    #[arg(short = 'm', long, help = "Model (v4-pro, flash, r1, or full name)")]
    pub model: Option<String>,
    #[arg(short = 's', long, help = "Resume session by ID (prefix OK)")]
    pub session: Option<String>,
    #[arg(short = 'n', long, help = "Start fresh session (don't resume last)")]
    pub new: bool,
    #[arg(long, help = "System prompt (set once, persists in config)")]
    pub system: Option<String>,
    #[arg(long, help = "Disable tools (plain chat mode, no agent)")]
    pub plain: bool,
      #[arg(long, help = "Disable streaming output")]
      pub no_stream: bool,
    #[arg(long, help = "Approval mode: confirm before writing files or running shell commands")]
    pub approve: bool,
    #[arg(long, help = "Reasoning effort for R1 (low, medium, high)")]
    pub think: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliMode {
    Agent,
    Plan,
}

impl CliMode {
    fn label(&self) -> &'static str {
        match self {
            CliMode::Agent => "agent",
            CliMode::Plan => "plan",
        }
    }
    fn is_plan(&self) -> bool {
        matches!(self, CliMode::Plan)
    }
}

/// Read-only tools available in Plan mode + checklist for planning.
const PLAN_TOOLS: &[&str] = &[
    "read_file", "list_files", "list_tree", "get_file_info",
    "search_code", "file_search", "web_search", "fetch_url",
    "git_log", "git_show", "git_blame", "git_status", "git_diff",
    "review",
    "checklist_write", "checklist_add", "checklist_update", "checklist_list",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
    reasoning_content: Option<String>,
    created_at: i64,
}

// ── LLM-based context compaction ──────────────────────────────────

async fn compact_via_llm(
    messages: &mut Vec<Message>,
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    narrow: bool,
) -> bool {
    const COMPACT_AT_TOKENS: usize = 100_000;
    let estimated: usize = messages.iter().map(|m| m.content.len() / 4 + 1).sum();
    if estimated < COMPACT_AT_TOKENS {
        return false;
    }
    const KEEP: usize = 20;
    if messages.len() <= KEEP + 2 {
        return false;
    }
    let split = messages.len() - KEEP;

    let old_msgs: Vec<Message> = messages[..split].to_vec();
    let mut summary_prompt = String::from(
        "Summarize this conversation concisely in Chinese. Preserve:\n\
         - Key decisions and their rationale\n\
         - Code changes and file paths\n\
         - User requirements and constraints\n\
         - Open questions or blockers\n\n\
         Conversation:\n",
    );
    for m in &old_msgs {
        let role = &m.role;
        let text: String = m.content.chars().take(800).collect();
        summary_prompt.push_str(&format!("<{}>\n{}\n", role, text));
    }

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": crate::api::default_model(false),
        "messages": [{"role": "user", "content": summary_prompt}],
        "max_tokens": 2048,
        "stream": false,
    });
    let resp = match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("\x1B[33m─ compaction request failed: {e}\x1B[0m");
            return false;
        }
    };

    let data: serde_json::Value = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            if narrow { eprintln!("\x1B[33m─ compaction parse failed: {e}\x1B[0m"); }
            return false;
        }
    };

    let summary = match data["choices"][0]["message"]["content"].as_str() {
        Some(s) => s.trim(),
        None => {
            if narrow { eprintln!("\x1B[33m─ compaction returned empty\x1B[0m"); }
            return false;
        }
    };

    messages.drain(..split);
    messages.insert(
        0,
        Message {
            role: "system".into(),
            content: format!("[Prior conversation summary]\n{}", summary),
            reasoning_content: None,
            created_at: old_msgs.first().map(|m| m.created_at).unwrap_or(0),
        },
    );

    if narrow {
        let new_est: usize = messages.iter().map(|m| m.content.len() / 4 + 1).sum();
        eprintln!(
            "─ compacted {} old msgs via LLM ({} tok → {} tok)",
            old_msgs.len(), estimated, new_est
        );
    }
    true
}

pub async fn run(args: &ChatArgs) {
    api::ensure_default_config();
    let model = resolve_model_name(
        &args.model.clone().unwrap_or_else(|| api::default_model(true)),
    );
    let api_key = if let Some(key) = resolve_api_key() {
        key
    } else {
        eprintln!("\x1B[33mNo API key found. Set it now to start chatting.\x1B[0m");
        let input = rpassword::prompt_password("Enter your DeepSeek API key (sk-...): ")
            .unwrap_or_else(|_| {
                print!("Enter your DeepSeek API key (sk-...): ");
                io::stdout().flush().unwrap();
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).unwrap();
                buf
            });
        let key = input.trim().to_string();
        if key.is_empty() || !key.starts_with("sk-") {
            eprintln!("\x1B[31merror: invalid API key\x1B[0m");
            std::process::exit(1);
        }
        // Save to config
        let path = crate::utils::dscode_dir().join("config.toml");
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
        if let Ok(mut store) = codewhale_config::ConfigStore::load(Some(path.clone())) {
            store.config.api_key = Some(key.clone());
            let _ = store.save();
        }
        eprintln!("\x1B[32m✓ API key saved\x1B[0m");
        key
    };
    let base_url = resolve_base_url();
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .build().unwrap();

      let store = open_store();
      let cwd = std::env::current_dir().unwrap_or_default();
      let (thread_id, mut messages) = if let Some(sid) = &args.session {
          match store.as_ref().and_then(|s| load_store_thread(s, sid)) {
              Some((id, msgs)) => { eprintln!("(resumed session {})", &id[..8]); (id, msgs) }
              None => (new_thread(&store, &model), Vec::new())
          }
    } else {
        // Smart resume: if the latest session was updated within 30 min, continue it
        // Otherwise start fresh (old session = different task)
        const AUTO_RESUME_SECS: i64 = 30 * 60;
        let now = chrono::Utc::now().timestamp();
        match store.as_ref().and_then(|s| latest_store_thread(s, &cwd)) {
            Some((id, msgs, _updated_at)) if now - _updated_at < AUTO_RESUME_SECS => {
                eprintln!("(resumed)");
                (id, msgs)
            }
            _ => (new_thread(&store, &model), Vec::new()),
        }
    };

    let narrow = is_narrow_terminal();
    let mut tw = terminal_width();

    if !narrow {
        println!("dscode · {model}  (Ctrl+C /help) [{} msgs]", messages.len());
        println!("{}", "─".repeat(std::cmp::min(usize::from(tw.saturating_sub(1)), 50)));
    }

    let mut rl = DefaultEditor::new().ok();
    // Per-project history: hash the cwd so history doesn't mix across projects
    let cwd_hash = {
        let cwd = std::env::current_dir().unwrap_or_default();
        let s = cwd.to_string_lossy();
        let h: u64 = s.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        format!("{:016x}", h)
    };
    let hist_path = config_dir().join(format!("history_{}.txt", &cwd_hash[..12]));
    if let Some(ref mut rl) = rl { let _ = rl.load_history(&hist_path); }

    let git_branch = get_git_branch();
    let project_dir = std::env::current_dir().ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    let engine = AgentEngine::new(client.clone(), base_url.clone(), api_key.clone());
    let mut mode = CliMode::Agent;

    loop {
        tw = terminal_width();
        let prompt = if narrow {
            let mode_tag = if mode.is_plan() { "\x1B[33mP\x1B[0m " } else { "" };
            if let Some(ref b) = git_branch { format!("{mode_tag}\x1B[35m{b}\x1B[0m [{}]> ", messages.len()) }
            else { format!("{mode_tag}[{}]> ", messages.len()) }
        } else {
            let dir = project_dir.as_deref().unwrap_or("");
            let branch = git_branch.as_deref().unwrap_or("");
            if mode.is_plan() {
                format!("\x1B[36m{dir}\x1B[0m \x1B[35m{branch}\x1B[0m [\x1B[33mplan\x1B[0m {}]> ", messages.len())
            } else {
                format!("\x1B[36m{dir}\x1B[0m \x1B[35m{branch}\x1B[0m [{}]> ", messages.len())
            }
        };

        let input = if let Some(ref mut rl) = rl {
            match rl.readline(&prompt) {
                Ok(line) => { let _ = rl.add_history_entry(&line); let _ = rl.save_history(&hist_path); line }
                Err(rustyline::error::ReadlineError::Interrupted) => { println!(); break; }
                Err(_) => break,
            }
        } else {
            print!("{prompt}"); io::stdout().flush().unwrap();
            let mut buf = String::new();
            match io::stdin().read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => buf,
                Err(_) => break,
            }
        };

        let mut input = input.trim_end().to_string();
        if input.is_empty() { continue; }

        if input.ends_with('\\') {
            input.pop();
            loop {
                let sub = rl_readline_raw(&mut rl, &hist_path);
                if sub.is_empty() { break; }
                let cont = sub.ends_with('\\');
                let sub = if cont { sub[..sub.len()-1].to_string() } else { sub };
                input.push('\n'); input.push_str(&sub);
                if !cont { break; }
            }
        } else if input.starts_with("```") {
            let fence = input.clone();
            loop {
                let sub = rl_readline_raw(&mut rl, &hist_path);
                input.push('\n'); input.push_str(&sub);
                if sub.trim() == "```" || sub == fence { break; }
            }
        }

        match input.trim() {
            "/exit" | "/quit" => break,
            "/clear" => {
                messages.clear();
                print!("\x1B[2J\x1B[H");
                if !narrow {
                    println!("dscode · {model}");
                    println!("{}", "─".repeat(std::cmp::min(usize::from(tw.saturating_sub(1)), 50)));
                }
                continue;
            }
            "/help" => {
                println!("/exit  quit    /clear  clear screen    /save  save session");
                println!("/mode plan    read-only research mode    /mode agent  full tool mode");
                continue;
            }
            "/save" => { persist_session(&store, &thread_id, &model, &messages); println!("saved"); continue; }
            "/mode" => {
                println!("Current mode: {}  (use /mode plan or /mode agent)", mode.label());
                continue;
            }
            cmd if cmd.starts_with("/mode ") => {
                let arg = cmd.strip_prefix("/mode ").unwrap_or("");
                match arg.trim() {
                    "plan" => { mode = CliMode::Plan; eprintln!("\x1B[33m○ plan mode\x1B[0m (read-only research + planning)"); }
                    "agent" => { mode = CliMode::Agent; eprintln!("\x1B[36m● agent mode\x1B[0m (full tools)"); }
                    other => eprintln!("unknown mode '{other}' — use plan or agent"),
                }
                continue;
            }
            _ => {}
        }

        messages.push(Message { role: "user".into(), content: input, reasoning_content: None, created_at: Utc::now().timestamp() });
        compact_via_llm(&mut messages, &client, &base_url, &api_key, narrow).await;

          let tools_list = if args.plain {
              None
          } else if mode.is_plan() {
              Some(api::tool_definitions_filtered(PLAN_TOOLS))
          } else {
              Some(api::tool_definitions_filtered(tools::CORE_TOOL_NAMES))
          };        
        let default_system = "\
You are dscode, a mobile-first AI coding agent powered by DeepSeek.
You are running directly in the project root directory. Always use relative paths for tools.

## Reasoning & Planning
- Before writing code, analyze the request. Decompose into sub-problems and solve one at a time.
- For complex tasks, investigate first: read_file, search_code, search_symbols, list_files before committing to an approach.
- Before each tool call, state your reasoning concisely. Example: \"Reading file X to check Y's type before implementing Z.\"
- If investigation contradicts your assumption, pause and re-evaluate. Do not force a fit.
- Use your thinking tokens for deep analysis of logic, edge cases, and trade-offs before acting.

## Code Quality
- **Self-Correction**: If a tool fails (e.g., edit_file match error), read the file again and retry. If it fails twice, stop trying surgical edits — use `write_file` with the entire file content instead.
- **Preferred Edit Method**: For small changes in large files, **prefer `apply_patch`** over `edit_file` — it uses git's fuzzy matching and is more robust. For whole-file changes, use `write_file`.
- **Edit Precision**: When using `edit_file`, always include the `line` hint parameter to avoid ambiguity. Read the file fresh before editing — never rely on memory of its content.
- **Batch Reads**: Read multiple files in a single round when you need context from different parts of the project. Reduces total rounds.
- **Batch Shell**: Combine multiple shell commands into one `run_shell` call using `&&` or `;`. Each separate call costs a round — batch aggressively.
- **Verification Loop**: After every significant code change, run `cargo check` (or relevant linter) via `run_shell` to catch syntax errors immediately.
- **Type safety**: Prefer Rust's type system over runtime checks.
- **Error handling**: Use Result, attach context. Avoid unwrap/expect except in tests.
- **Edge cases**: Always consider empty input, boundary values, error paths, and concurrent access.
- **Minimal diffs**: Change only what is needed. Do not reformat unrelated lines.
- **Single-file limit**: Files exceeding 400 lines should be split into focused modules.

## Verification
- After editing a file, read it back to confirm the change.
- After writing Rust code, run `run_shell(command: \"cargo check\")` to verify compilation.
- After fixing a test, run `test_runner` to confirm.
- Report actual output. Never claim success without evidence.
- Tool call failures must be reported. Never hide errors.
- Promises must be followed by immediate tool execution. Never say \"I will\" without doing it.

## Communication
- Be concise. Use code blocks for code, paths, and shell commands.
- After completing work, summarize what changed and why.
- When uncertain, say \"I don't know\" — never fabricate.\
";

        let agent_prompt = api::load_agent_md();
        let plan_prefix = if mode.is_plan() {
            "\n## Current Mode: Plan\nYou are in **Plan mode** — read-only research and design.\n\
             - Use read_file, search_code, list_files, git tools to investigate the codebase.\n\
             - Use checklist_write to produce a structured plan before any implementation.\n\
             - You CANNOT edit files, run arbitrary shell commands, or make git commits.\n\
             - When you have a complete plan, tell the user to switch to agent mode with /mode agent.\n"
        } else { "" };
        let sys_content = if let Some(ref ap) = agent_prompt {
            format!("{}{}\n\n{}", default_system, plan_prefix, ap)
        } else if let Some(sp) = &args.system {
            format!("{}{}\n\n{}", default_system, plan_prefix, sp)
        } else {
            format!("{}{}", default_system, plan_prefix)
        };

        let history: Vec<serde_json::Value> = messages.iter().map(|m| {
            let mut j = serde_json::json!({"role": m.role, "content": m.content});
            if let Some(ref rc) = m.reasoning_content { j["reasoning_content"] = serde_json::Value::String(rc.clone()); }
            j
        }).collect();

            let options = AgentOptions {
                model: model.clone(),
                system_prompt: sys_content,
                tools: tools_list,
                max_rounds: 100,
                narrow,
                silent: false,
                approval_mode: args.approve,
                terminal_width: tw,
                cwd: std::env::current_dir().unwrap_or_default(),
                reasoning_effort: args.think.clone().or(Some("medium".to_string())),
            };        match engine.run_loop(&options, history).await {
            Ok((new_api_msgs, usage)) => {
                if usage.tokens_out > 0 {
                    if narrow {
                        eprintln!("\x1B[90m─ usage: {:.0} tok (reasoning: {:.0})\x1B[0m", usage.tokens_out, usage.reasoning_tokens);
                    } else {
                        eprint!("\x1B[90m─ {:.0} tok (reasoning: {:.0})\x1B[0m", usage.tokens_out, usage.reasoning_tokens);
                        if usage.cache_hit_tokens > 0 {
                            eprint!(" cache: {:.0}", usage.cache_hit_tokens);
                        }
                        eprintln!();
                    }
                }
                // The first message is system, the rest are user/assistant/tool messages.
                // We want to find the NEW assistant/tool messages and update our `messages` vector.
                // Actually, let's just extract all assistant messages from the result that are NOT in our `messages` yet.
                // A simpler way: the model might have called tools. The final message in `new_api_msgs`
                // should be the model's final response if it didn't call tools, or the last assistant message.
                
                // For `chat.rs`, we only care about the final user-facing response to show in history.
                // But we also need to store tool results in `api_msgs` for the NEXT turn.
                // Wait, if we use a persistent `api_msgs` for the next turn, we need to handle it.
                
                // Actually, chat.rs's `messages` only stores user and final assistant responses for display.
                // Tool calls are usually transient in the session history unless we want to persist them.
                // Let's find the last assistant message.
                
                if let Some(last_msg) = new_api_msgs.last() {
                    if last_msg["role"] == "assistant" {
                        let content = last_msg["content"].as_str().unwrap_or("").to_string();
                        let rc = last_msg["reasoning_content"].as_str().map(|s| s.to_string());
                        
                        // Check if this was a tool call that we already processed
                        // Actually, the engine's final message is what we want.
                        messages.push(Message {
                            role: "assistant".into(),
                            content,
                            reasoning_content: rc,
                            created_at: Utc::now().timestamp(),
                        });
                    }
                }
                persist_session(&store, &thread_id, &model, &messages);
            }
            Err(e) => {
                eprintln!("\nerror: {e}");
            }
        }
    }
}

// ── State store (codewhale-state SQLite) ─────────────────────────

use codewhale_state::{StateStore, ThreadListFilters, ThreadMetadata, ThreadStatus, SessionSource};

fn db_path() -> PathBuf {
    crate::utils::dscode_dir().join("state.db")
}

fn open_store() -> Option<StateStore> {
    let path = db_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    StateStore::open(Some(path)).ok()
}

fn new_thread(store: &Option<StateStore>, model: &str) -> String {
    let id = Uuid::new_v4().to_string();
    let Some(store) = store else { return id };
    let now = Utc::now().timestamp();
    let _ = store.upsert_thread(&ThreadMetadata {
        id: id.clone(), rollout_path: None, preview: String::new(), ephemeral: false,
        model_provider: model.to_string(), created_at: now, updated_at: now,
        status: ThreadStatus::Running, path: None, cwd: std::env::current_dir().unwrap_or_default(),
        cli_version: env!("CARGO_PKG_VERSION").to_string(), source: SessionSource::Interactive,
        name: None, sandbox_policy: None, approval_mode: None, archived: false, archived_at: None,
        git_sha: None, git_branch: None, git_origin_url: None, memory_mode: None,
    });
    id
}

fn persist_session(store: &Option<StateStore>, thread_id: &str, model: &str, messages: &[Message]) {
    let Some(store) = store else { return };
    let _ = store.clear_messages(thread_id);
    for m in messages {
        let item = m.reasoning_content.as_ref().map(|rc| serde_json::json!({"reasoning_content": rc}));
        let _ = store.append_message(thread_id, &m.role, &m.content, item);
    }
    let now = Utc::now().timestamp();
    let mut thread = store.get_thread(thread_id).ok().flatten().unwrap_or_else(|| {
        ThreadMetadata {
            id: thread_id.to_string(), rollout_path: None, preview: String::new(), ephemeral: false,
            model_provider: model.to_string(), created_at: now, updated_at: now,
            status: ThreadStatus::Running, path: None, cwd: std::env::current_dir().unwrap_or_default(),
            cli_version: env!("CARGO_PKG_VERSION").to_string(), source: SessionSource::Interactive,
            name: None, sandbox_policy: None, approval_mode: None, archived: false, archived_at: None,
            git_sha: None, git_branch: None, git_origin_url: None, memory_mode: None,
        }
    });
    thread.updated_at = now;
    thread.preview = messages.first().map(|m| m.content.chars().take(120).collect()).unwrap_or_default();
    if thread.name.is_none() && messages.len() >= 2 {
        if let Some(first_user) = messages.iter().find(|m| m.role == "user") {
            let name: String = first_user.content.chars().take(40).collect();
            thread.name = Some(name.trim().to_string());
        }
    }
    let _ = store.upsert_thread(&thread);
}

fn msg_from_record(m: codewhale_state::MessageRecord) -> Message {
    let rc = m.item.as_ref().and_then(|v| v["reasoning_content"].as_str()).map(|s| s.to_string());
    Message { role: m.role, content: m.content, reasoning_content: rc, created_at: m.created_at }
}

fn load_store_thread(store: &StateStore, id: &str) -> Option<(String, Vec<Message>)> {
    if let Ok(Some(t)) = store.get_thread(id) {
        let msgs = store.list_messages(&t.id, None).unwrap_or_default().into_iter().map(msg_from_record).collect();
        return Some((t.id, msgs));
    }
    let threads = store.list_threads(ThreadListFilters { include_archived: false, limit: Some(100) }).ok()?;
    for t in threads {
        if t.id.starts_with(id) {
            let msgs = store.list_messages(&t.id, None).unwrap_or_default().into_iter().map(msg_from_record).collect();
            return Some((t.id, msgs));
        }
    }
    None
}

/// Find the most recent session for the current project directory,
/// returning (id, messages, updated_at) if one exists.
fn latest_store_thread(store: &StateStore, cwd: &std::path::Path) -> Option<(String, Vec<Message>, i64)> {
    use codewhale_state::ThreadListFilters;
    let threads = store.list_threads(ThreadListFilters { include_archived: false, limit: Some(50) }).ok()?;
    let t = threads.into_iter()
        .filter(|t| t.cwd == cwd)
        .max_by_key(|t| t.updated_at)?;
    let msgs = store.list_messages(&t.id, None).unwrap_or_default().into_iter().map(msg_from_record).collect();
    Some((t.id, msgs, t.updated_at))
}

fn get_git_branch() -> Option<String> {
    let out = std::process::Command::new("git").args(["rev-parse", "--abbrev-ref", "HEAD"]).output().ok()?;
    if out.status.success() {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !branch.is_empty() && branch != "HEAD" { Some(branch) } else { None }
    } else { None }
}

fn config_dir() -> PathBuf {
    crate::utils::dscode_dir()
}

fn rl_readline_raw(rl: &mut Option<DefaultEditor>, hist_path: &PathBuf) -> String {
    if let Some(ref mut rl) = rl {
        match rl.readline("") {
            Ok(line) => { let _ = rl.add_history_entry(&line); let _ = rl.save_history(hist_path); line.trim_end().to_string() }
            Err(_) => String::new(),
        }
    } else {
        let mut buf = String::new(); io::stdin().read_line(&mut buf).ok(); buf.trim_end().to_string()
    }
}
