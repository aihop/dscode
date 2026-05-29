/// Mobile-first interactive chat with DeepSeek.
///
/// Thin UX layer on top of dscode engine + shared api.rs.
/// Session persistence via SQLite (codewhale-state).

use crate::api::{self, resolve_model_name, resolve_api_key, resolve_base_url};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
    created_at: i64,
}

pub async fn run(args: &ChatArgs) {
    let model = resolve_model_name(
        &args.model.clone().unwrap_or_else(|| api::default_model(false)),
    );
    let stream = !args.no_stream;

    let api_key = resolve_api_key().unwrap_or_else(|| {
        eprintln!("error: no DeepSeek API key found");
        eprintln!("  Set one with:  dscode auth login");
        eprintln!("  Or export:     export DEEPSEEK_API_KEY=sk-...");
        std::process::exit(1);
    });
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();

    // Open state store (fail gracefully → in-memory only)
    let store = open_store();

    // Session resume: explicit -s, or auto-resume latest, or --new
    let (thread_id, mut messages) = if let Some(sid) = &args.session {
        match store.as_ref().and_then(|s| load_store_thread(s, sid)) {
            Some((id, msgs)) => { eprintln!("(resumed session {})", &id[..8]); (id, msgs) }
            None => { eprintln!("session '{sid}' not found, starting new"); (new_thread(&store, &model), Vec::new()) }
        }
    } else if args.new {
        (new_thread(&store, &model), Vec::new())
    } else {
        // Auto-resume: find the most recent session
        match store.as_ref().and_then(|s| latest_store_thread(s)) {
            Some((id, msgs)) => { eprintln!("(resumed latest session {})", &id[..8]); (id, msgs) }
            None => (new_thread(&store, &model), Vec::new())
        }
    };

    let narrow = is_narrow_terminal();
    let tw = terminal_width();
    let max_rounds: usize = 20; // trim oldest when exceeded

    let initial_msgs = messages.len();
    if !narrow {
        println!("dscode · {model}  (Ctrl+C /help) [{} msgs]", initial_msgs);
        println!("{}", "─".repeat(std::cmp::min(usize::from(tw.saturating_sub(1)), 50)));
    }

    // Initialize rustyline for zsh-like input editing
    let mut rl = if narrow {
        // Disable raw mode on narrow terminals (some SSH clients struggle)
        DefaultEditor::new().ok()
    } else {
        DefaultEditor::new().ok()
    };

    // Load history
    let hist_path = config_dir().join("history.txt");
    if let Some(ref mut rl) = rl {
        let _ = rl.load_history(&hist_path);
    }

    // Detect git branch for prompt
    let git_branch = get_git_branch();

    loop {
        let branch_tag = if let Some(ref b) = git_branch {
            if narrow { format!("\x1B[35m{b}\x1B[0m ") } else { format!("\x1B[35m{b}\x1B[0m ") }
        } else { String::new() };
        let prompt = format!("{branch_tag}> ");

        let input = if let Some(ref mut rl) = rl {
            match rl.readline(&prompt) {
                Ok(line) => {
                    let _ = rl.add_history_entry(&line);
                    let _ = rl.save_history(&hist_path);
                    line
                }
                Err(rustyline::error::ReadlineError::Interrupted) => { println!(); break; }
                Err(_) => break, // EOF or error
            }
        } else {
            // Fallback: plain readline
            print!("{prompt}"); io::stdout().flush().unwrap();
            let mut buf = String::new();
            match io::stdin().read_line(&mut buf) {
                Ok(0) => break,
                Ok(_) => buf,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => { println!(); break; }
                Err(e) => { eprintln!("\nerror: {e}"); break; }
            }
        };

        let mut input = input.trim_end().to_string();
        if input.is_empty() { continue; }

        // Multi-line input: line ends with \  or starts with ```
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

        // Built-in commands (match on trimmed single-line)
        let cmd = input.trim();
        match cmd {
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
                println!("/exit  quit    /clear  clear screen");
                println!("/save  save now");
                continue;
            }
            "/save" => { persist_session(&store, &thread_id, &model, &messages); println!("saved"); continue; }
            _ => {}
        }

        let ts = Utc::now().timestamp();
        messages.push(Message { role: "user".into(), content: input, created_at: ts });

        // Context window: keep last N rounds
        if messages.len() > max_rounds * 2 {
            let trimmed = messages.len() - max_rounds * 2;
            messages.drain(0..trimmed);
            if narrow { eprintln!("─ trimmed {trimmed} old msgs to save context"); }
        }

        let tools_list = if args.plain { vec![] } else { api::tool_definitions() };
        let active_tools: Option<&[serde_json::Value]> = if tools_list.is_empty() { None } else { Some(&tools_list) };

        // Auto-inject AGENT.md as system prompt if it exists in project root
        let agent_prompt = load_agent_md();
        let mut api_msgs: Vec<serde_json::Value> = Vec::new();
        if let Some(ref ap) = agent_prompt {
            if api_msgs.is_empty() || api_msgs[0]["role"] != "system" {
                api_msgs.push(serde_json::json!({"role": "system", "content": ap}));
            }
        }
        if let Some(sp) = &args.system { if !sp.is_empty() { api_msgs.push(serde_json::json!({"role": "system", "content": sp})); } }
        api_msgs.extend(messages.iter().map(|m| serde_json::json!({"role": m.role, "content": m.content})));

        // Agent loop: chat → tool_calls → execute → chat → ...
        let max_agent_rounds = 15;
        let mut agent_round = 0;
        loop {
            agent_round += 1;
            if agent_round > max_agent_rounds { break; }

            let result = api::call_stream(&client, &base_url, &api_key, &model, &api_msgs, active_tools, narrow, tw).await;

            match result {
                Ok(stream_res) => {
                    // Add assistant message with any tool_calls to context
                    let mut assistant_msg = serde_json::json!({
                        "role": "assistant",
                        "content": stream_res.content,
                    });
                    if !stream_res.tool_calls.is_empty() {
                        // DeepSeek requires reasoning_content to be echoed back if present
                        if !stream_res.reasoning_content.is_empty() {
                            assistant_msg["reasoning_content"] = serde_json::Value::String(stream_res.reasoning_content.clone());
                        }
                        let tc_json: Vec<serde_json::Value> = stream_res.tool_calls.iter().map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": { "name": tc.name, "arguments": tc.arguments }
                            })
                        }).collect();
                        assistant_msg["tool_calls"] = serde_json::Value::Array(tc_json);
                        api_msgs.push(assistant_msg);

                        // Execute tools
                        for tc in &stream_res.tool_calls {
                            let result = api::execute_tool(tc).await;
                            api_msgs.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tc.id,
                                "content": result,
                            }));
                            if narrow {
                                eprintln!("─ tool: {}(..) → {} chars", tc.name, result.len());
                            }
                        }
                        if narrow { eprintln!("─ {:.0} tok, continuing...", stream_res.usage.tokens_out); }
                        continue; // next agent round
                    } else {
                        // Pure text response
                        messages.push(Message {
                            role: "assistant".into(),
                            content: stream_res.content.clone(),
                            created_at: Utc::now().timestamp(),
                        });
                        if narrow { eprintln!("─ {:.0} tok", stream_res.usage.tokens_out); }
                        persist_session(&store, &thread_id, &model, &messages);
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("\nerror: {e}");
                    if !stream { messages.pop(); }
                    break;
                }
            }
        }
    }

    persist_session(&store, &thread_id, &model, &messages);
}

// ── State store (codewhale-state SQLite) ─────────────────────────

use codewhale_state::{StateStore, ThreadListFilters, ThreadMetadata, ThreadStatus, SessionSource};

fn db_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("dscode").join("state.db")
}

fn open_store() -> Option<StateStore> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    StateStore::open(Some(path)).ok()
}

/// Create a new thread in the store and return its id.
fn new_thread(store: &Option<StateStore>, model: &str) -> String {
    let id = Uuid::new_v4().to_string();
    let Some(store) = store else { return id };
    let now = Utc::now().timestamp();
    let cwd = std::env::current_dir().unwrap_or_default();
    let _ = store.upsert_thread(&ThreadMetadata {
        id: id.clone(),
        rollout_path: None,
        preview: String::new(),
        ephemeral: false,
        model_provider: model.to_string(),
        created_at: now,
        updated_at: now,
        status: ThreadStatus::Running,
        path: None,
        cwd,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        source: SessionSource::Interactive,
        name: None,
        sandbox_policy: None,
        approval_mode: None,
        archived: false,
        archived_at: None,
        git_sha: None,
        git_branch: None,
        git_origin_url: None,
        memory_mode: None,
    });
    id
}

/// Persist all messages and update thread metadata.
fn persist_session(store: &Option<StateStore>, thread_id: &str, model: &str, messages: &[Message]) {
    let Some(store) = store else { return };
    // Re-write all messages
    let _ = store.clear_messages(thread_id);
    for m in messages {
        let _ = store.append_message(thread_id, &m.role, &m.content, None);
    }
    // Update thread metadata
    let now = Utc::now().timestamp();
    let preview = messages.first()
        .map(|m| m.content.chars().take(120).collect())
        .unwrap_or_default();
    let mut thread = store.get_thread(thread_id).ok().flatten().unwrap_or_else(|| {
        let cwd = std::env::current_dir().unwrap_or_default();
        ThreadMetadata {
            id: thread_id.to_string(),
            rollout_path: None,
            preview: String::new(),
            ephemeral: false,
            model_provider: model.to_string(),
            created_at: now,
            updated_at: now,
            status: ThreadStatus::Running,
            path: None,
            cwd,
            cli_version: env!("CARGO_PKG_VERSION").to_string(),
            source: SessionSource::Interactive,
            name: None,
            sandbox_policy: None,
            approval_mode: None,
            archived: false,
            archived_at: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            memory_mode: None,
        }
    });
    thread.updated_at = now;
    thread.preview = preview;
    let _ = store.upsert_thread(&thread);
}

/// Load thread messages from store by exact or prefix id.
fn load_store_thread(store: &StateStore, id: &str) -> Option<(String, Vec<Message>)> {
    // Exact match
    if let Ok(Some(t)) = store.get_thread(id) {
        let msgs = store.list_messages(&t.id, None).unwrap_or_default()
            .into_iter().map(|m| Message { role: m.role, content: m.content, created_at: m.created_at }).collect();
        return Some((t.id, msgs));
    }
    // Prefix match
    let threads = store.list_threads(ThreadListFilters { include_archived: false, limit: Some(100) }).ok()?;
    for t in threads {
        if t.id.starts_with(id) {
            let msgs = store.list_messages(&t.id, None).unwrap_or_default()
                .into_iter().map(|m| Message { role: m.role, content: m.content, created_at: m.created_at }).collect();
            return Some((t.id, msgs));
        }
    }
    None
}

/// Find the most recently updated thread.
fn latest_store_thread(store: &StateStore) -> Option<(String, Vec<Message>)> {
    let t = store.list_threads(ThreadListFilters { include_archived: false, limit: Some(1) })
        .ok()?.into_iter().next()?;
    let msgs = store.list_messages(&t.id, None).unwrap_or_default()
        .into_iter().map(|m| Message { role: m.role, content: m.content, created_at: m.created_at }).collect();
    Some((t.id, msgs))
}

// ── Terminal helpers ─────────────────────────────────────────────

fn terminal_width() -> u16 {
    if let Ok(cols) = std::env::var("COLUMNS") { if let Ok(w) = cols.parse::<u16>() { if w > 0 { return w; } } }
    if let Ok(o) = std::process::Command::new("stty").args(["size"]).stdin(std::process::Stdio::inherit()).output() {
        if let Ok(s) = String::from_utf8(o.stdout) {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if parts.len() == 2 { if let Ok(w) = parts[1].parse::<u16>() { if w > 0 { return w; } } }
        }
    }
    80
}

/// Load AGENT.md from project root if it exists
fn load_agent_md() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    for name in &["AGENT.md", "AGENTS.md", "CLAUDE.md"] {
        let path = cwd.join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return Some(format!("Project rules from {}:\n{}", name, trimmed));
                }
            }
        }
    }
    None
}

/// Detect current git branch name for prompt display
fn get_git_branch() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output().ok()?;
    if out.status.success() {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !branch.is_empty() && branch != "HEAD" { Some(branch) } else { None }
    } else { None }
}

fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config")).join("dscode")
}

fn is_narrow_terminal() -> bool { terminal_width() <= 80 }

/// Read one line using rustyline (or plain stdin fallback)
fn rl_readline_raw(rl: &mut Option<DefaultEditor>, hist_path: &PathBuf) -> String {
    if let Some(ref mut rl) = rl {
        match rl.readline("") {
            Ok(line) => {
                let _ = rl.add_history_entry(&line);
                let _ = rl.save_history(hist_path);
                line.trim_end().to_string()
            }
            Err(_) => String::new(),
        }
    } else {
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).ok();
        buf.trim_end().to_string()
    }
}
