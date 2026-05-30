/// Review, FIM edit, sub-agents, checklist, tests, and shell tools.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::tools::ToolCtx;
use crate::api::{resolve_api_key, resolve_base_url};
use crate::engine::{AgentEngine, AgentOptions};

// ── Review tool ───────────────────────────────────────────────

pub(crate) async fn exec_review(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let target = v["target"].as_str().unwrap_or("");
    if target.is_empty() { return "error: no target provided".to_string(); }

    let code = if target == "diff" {
        let o = std::process::Command::new("git").args(["diff"]).current_dir(&ctx.cwd).output();
        match o { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(), _ => return "no diff".to_string() }
    } else if target == "staged" {
        let o = std::process::Command::new("git").args(["diff", "--cached"]).current_dir(&ctx.cwd).output();
        match o { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(), _ => return "no staged changes".to_string() }
    } else {
        let full = if target.starts_with('/') { PathBuf::from(target) } else { ctx.cwd.join(target) };
        match std::fs::read_to_string(&full) { Ok(c) => c, Err(e) => return format!("error reading {target}: {e}") }
    };
    if code.is_empty() { return "nothing to review".to_string(); }
    let code = if code.len() > 32_000 { format!("{}... (truncated)", &code[..32_000]) } else { code };

    let Some(api_key) = resolve_api_key() else { return "error: no API key".to_string() };
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": crate::api::default_model(false),
        "messages": [
            {"role": "system", "content": "You are a senior code reviewer. Return a concise review with: summary, issues (severity/title/description), and suggestions. Be direct and actionable."},
            {"role": "user", "content": format!("Review this code:\n```\n{}```", code)}
        ],
        "max_tokens": 8192,
        "stream": false,
    });
    match client.post(&url).header("Authorization", format!("Bearer {api_key}")).json(&body).send().await {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or_default();
            data["choices"][0]["message"]["content"].as_str().unwrap_or("(no response)").to_string()
        }
        Err(e) => format!("review API error: {e}"),
    }
}

// ── FIM edit tool ─────────────────────────────────────────────

pub(crate) async fn exec_fim_edit(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    let prefix_anchor = v["prefix_anchor"].as_str().unwrap_or("");
    let suffix_anchor = v["suffix_anchor"].as_str().unwrap_or("");
    let max_tokens = v["max_tokens"].as_u64().unwrap_or(1024).min(4096);
    if path.is_empty() || prefix_anchor.is_empty() || suffix_anchor.is_empty() {
        return "error: path, prefix_anchor, and suffix_anchor are required".to_string();
    }
    let full_path = if path.starts_with('/') { PathBuf::from(path) } else { ctx.cwd.join(path) };
    let content = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(e) => return format!("error reading {path}: {e}"),
    };
    let pa_start = match content.find(prefix_anchor) {
        Some(p) => p,
        None => return format!("prefix_anchor not found in {path}"),
    };
    let sa_start = match content[pa_start + 1..].find(suffix_anchor) {
        Some(p) => pa_start + 1 + p,
        None => return format!("suffix_anchor not found after prefix_anchor in {path}"),
    };
    if sa_start <= pa_start + prefix_anchor.len() {
        return "error: suffix_anchor overlaps with prefix_anchor".to_string();
    }
    let prompt = &content[..pa_start + prefix_anchor.len()];
    let suffix = &content[sa_start..];
    let pa_rest = &content[pa_start + prefix_anchor.len()..];
    if pa_rest.find(prefix_anchor).is_some() {
        return format!("error: prefix_anchor appears multiple times in {path} — include more context");
    }
    let sa_rest = &content[sa_start + suffix_anchor.len()..];
    if sa_rest.find(suffix_anchor).is_some() {
        return format!("error: suffix_anchor appears multiple times in {path} — include more context");
    }
    let Some(api_key) = resolve_api_key() else { return "error: no API key".to_string() };
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();
    let url = format!("{}/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": crate::api::resolve_model_name(&crate::api::default_model(false)),
        "prompt": prompt,
        "suffix": suffix,
        "max_tokens": max_tokens,
    });
    match client.post(&url).header("Authorization", format!("Bearer {api_key}")).json(&body).send().await {
        Ok(resp) => {
            let data: Value = resp.json().await.unwrap_or_default();
            let generated = data["choices"][0]["text"].as_str().unwrap_or("").to_string();
            if generated.is_empty() {
                return "FIM generated empty content".to_string();
            }
            let new_content = format!("{}{}{}", &content[..pa_start + prefix_anchor.len()], generated, &content[sa_start..]);
            crate::tools::file::backup_before_write(&full_path);
            match std::fs::write(&full_path, &new_content) {
                Ok(_) => {
                    let diff = crate::tools::file::diff_preview(ctx, path);
                    if !diff.is_empty() {
                        format!("fim_edit applied to {path} ({} chars generated)\n{}", generated.len(), diff)
                    } else {
                        format!("fim_edit applied to {path} ({} chars generated)", generated.len())
                    }
                }
                Err(e) => format!("error writing {path}: {e}"),
            }
        }
        Err(e) => format!("FIM API error: {e}"),
    }
}

// ── Sub-agent system ──────────────────────────────────────────

#[derive(Clone)]
struct SubAgentState {
    status: String,
    result: String,
    created_at: i64,
}

fn global_agents() -> &'static Arc<Mutex<HashMap<String, SubAgentState>>> {
    static AGENTS: OnceLock<Arc<Mutex<HashMap<String, SubAgentState>>>> = OnceLock::new();
    AGENTS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

async fn run_sub_agent(
    api_key: &str, base_url: &str, cwd: &std::path::Path,
    prompt: &str, context_msgs: &[Value],
    role: Option<&str>,
) -> String {
    let client = reqwest::Client::new();
    let engine = AgentEngine::new(client, base_url.to_string(), api_key.to_string());
    
    let history: Vec<Value> = if context_msgs.is_empty() {
        vec![json!({"role": "user", "content": prompt})]
    } else {
        let mut msgs = context_msgs.to_vec();
        msgs.push(json!({"role": "user", "content": prompt}));
        msgs
    };

    let base_prompt = match role {
        Some("architect") => "You are a software architect. Focus on high-level design, file structure, and dependency management. Your goal is to research the codebase and provide a detailed implementation plan. Do not write complex implementation code unless necessary.",
        Some("coder") => "You are a senior software engineer. Your goal is to implement the requested features or fixes with high quality. Follow existing patterns and ensure the code is idiomatic.",
        Some("reviewer") => "You are a rigorous code reviewer. Analyze the code for logic errors, security vulnerabilities, and performance bottlenecks. Provide actionable feedback.",
        Some("tester") => "You are a QA engineer. Focus on writing and running tests to verify the correctness of the code. Cover edge cases and error paths.",
        Some("explore") => "You are a code explorer. Your job is to read and understand the codebase. Use read_file, search_code, file_search, git_log, git_blame to find relevant code and understand its structure. Return path:line-range evidence. Do not modify any files.",
        Some("plan") => "You are a technical planner. Analyze the requirements and codebase, then produce a structured plan using checklist_write. Break the work into concrete steps with file paths and approach details. Do not write implementation code.",
        Some("verifier") => "You are a QA verifier. Run tests and validation commands to verify correctness. Report results clearly: pass/fail, failing assertions, and error messages. Do not fix failures — capture them for the parent to address.",
        _ => "You are a focused sub-agent. Complete the specific task assigned to you. Do not over-scope.",
    };

    // Read-only roles: can search and read but not write
    let read_only_roles = ["explore", "plan", "verifier"];
    let is_read_only = role.map_or(false, |r| read_only_roles.contains(&r));
    let tools = if is_read_only {
        Some(crate::api::tool_definitions_filtered(&[
            "read_file", "list_files", "list_tree", "get_file_info",
            "search_code", "file_search", "web_search", "fetch_url",
            "git_log", "git_show", "git_blame", "git_status", "git_diff",
            "review", "checklist_write", "checklist_add", "checklist_update", "checklist_list",
        ]))
    } else {
        Some(crate::api::tool_definitions())
    };

    let options = AgentOptions {
        model: crate::api::resolve_model_name(&crate::api::default_model(false)),
        system_prompt: format!("{}\n\n## Rules\n1. Before writing code, read the relevant files first.\n2. After writing code, verify by reading the file back.\n3. Report results concisely: what you did, what changed, any issues found.\n4. If blocked, report the blocker clearly — do not guess.\n5. Use your reasoning tokens to analyze edge cases before acting.", base_prompt),
        tools,
        max_rounds: 15,
        narrow: false,
        silent: true,
        approval_mode: false,
        terminal_width: 80,
        cwd: cwd.to_path_buf(),
        reasoning_effort: Some("medium".to_string()),
    };

    match engine.run_loop(&options, history).await {
        Ok((new_msgs, _usage)) => {
            if let Some(last) = new_msgs.last() {
                if last["role"] == "assistant" {
                    return last["content"].as_str().unwrap_or("").to_string();
                }
            }
            "(empty)".to_string()
        }
        Err(e) => format!("error: {e}"),
    }
}

pub(crate) async fn exec_agent_open(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let prompt = v["prompt"].as_str().unwrap_or("");
    let role = v["role"].as_str().map(|s| s.to_string());
    let context = v["context"].as_str().unwrap_or("");
    if prompt.is_empty() { return "error: no prompt".to_string(); }
    let Some(api_key) = resolve_api_key() else { return "error: no API key".to_string() };
    let base_url = resolve_base_url();
    let agent_id = format!("agent-{}", Uuid::new_v4());
    let agents = global_agents().clone();
    let cwd = ctx.cwd.clone();
    let pk = api_key.clone();
    let bu = base_url.clone();
    let pr = prompt.to_string();
    let mut ctx_msgs: Vec<Value> = Vec::new();
    if !context.is_empty() {
        ctx_msgs.push(json!({"role": "system", "content": format!("Background Context:\n{}", context)}));
    }
    agents.lock().unwrap().insert(agent_id.clone(), SubAgentState {
        status: "running".into(), result: String::new(), created_at: Utc::now().timestamp(),
    });
    let id = agent_id.clone();
    tokio::spawn(async move {
        let result = run_sub_agent(&pk, &bu, &cwd, &pr, &ctx_msgs, role.as_deref()).await;
        if let Some(state) = agents.lock().unwrap().get_mut(&id) {
            state.status = if result.starts_with("error:") { "failed".into() } else { "completed".into() };
            state.result = result;
        }
    });
    agent_id
}

pub(crate) async fn exec_agent_eval(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let id = v["agent_id"].as_str().unwrap_or("");
    let agents = global_agents().lock().unwrap();
    match agents.get(id) {
        Some(s) => json!({"status": s.status, "result": s.result, "created_at": s.created_at}).to_string(),
        None => "not found".to_string(),
    }
}

pub(crate) async fn exec_agent_close(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let id = v["agent_id"].as_str().unwrap_or("");
    let mut agents = global_agents().lock().unwrap();
    match agents.remove(id) {
        Some(s) => json!({"status": s.status, "result": s.result}).to_string(),
        None => "not found".to_string(),
    }
}

// ── Checklist ─────────────────────────────────────────────────

#[derive(Clone)]
struct ChecklistItem {
    id: usize,
    content: String,
    status: String,
}

fn global_checklist() -> &'static Arc<Mutex<(usize, Vec<ChecklistItem>)>> {
    static LIST: OnceLock<Arc<Mutex<(usize, Vec<ChecklistItem>)>>> = OnceLock::new();
    LIST.get_or_init(|| Arc::new(Mutex::new((0, Vec::new()))))
}

pub(crate) fn exec_checklist_write(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let todos = v["todos"].as_array().cloned().unwrap_or_default();
    let mut list = global_checklist().lock().unwrap();
    let mut next_id = list.0;
    let items: Vec<ChecklistItem> = todos.iter().map(|t| {
        let id = next_id; next_id += 1;
        ChecklistItem {
            id,
            content: t["content"].as_str().unwrap_or("").to_string(),
            status: t["status"].as_str().unwrap_or("pending").to_string(),
        }
    }).collect();
    list.0 = next_id;
    list.1 = items;
    format!("checklist set ({} items)", list.1.len())
}

pub(crate) fn exec_checklist_add(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let content = v["content"].as_str().unwrap_or("");
    if content.is_empty() { return "error: no content".to_string(); }
    let status = v["status"].as_str().unwrap_or("pending").to_string();
    let mut list = global_checklist().lock().unwrap();
    let item = ChecklistItem { id: list.0, content: content.to_string(), status };
    list.0 += 1;
    list.1.push(item);
    format!("added item {}", list.0 - 1)
}

pub(crate) fn exec_checklist_update(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let id = v["id"].as_u64().unwrap_or(u64::MAX) as usize;
    let status = v["status"].as_str().unwrap_or("");
    if status.is_empty() { return "error: no status".to_string(); }
    let mut list = global_checklist().lock().unwrap();
    for item in &mut list.1 {
        if item.id == id {
            item.status = status.to_string();
            return format!("item {id} → {status}");
        }
    }
    format!("item {id} not found")
}

pub(crate) fn exec_checklist_list() -> String {
    let list = global_checklist().lock().unwrap();
    if list.1.is_empty() { return "checklist is empty".to_string(); }
    let lines: Vec<String> = list.1.iter()
        .map(|i| format!("  {} [{}] {}", i.id, i.status, i.content))
        .collect();
    format!("Checklist ({} items):\n{}", list.1.len(), lines.join("\n"))
}

// ── Test runner ───────────────────────────────────────────────

pub(crate) async fn exec_test_runner(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let cmd = v["command"].as_str().unwrap_or("cargo test").to_string();
    let cwd = ctx.cwd.clone();
    let cmd2 = cmd.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("sh").args(["-c", &cmd2]).current_dir(&cwd).output()
    }).await;
    match output {
        Ok(Ok(o)) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let mut result = String::new();
            if !o.status.success() {
                result.push_str("exit code: ");
                result.push_str(&o.status.code().unwrap_or(-1).to_string());
                result.push('\n');
            }
            if !stdout.is_empty() { result.push_str(&stdout); }
            if !stderr.is_empty() {
                if !result.is_empty() { result.push('\n'); }
                result.push_str(&stderr);
            }
            if result.len() > 16000 {
                result = format!("{}... (truncated, {} total)", &result[..16000], result.len());
            }
            result
        }
        _ => format!("failed to run '{cmd}'"),
    }
}

// ── Batch: execute multiple tools in one round ─────────────

pub(crate) async fn exec_batch(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let commands = v["commands"].as_array().cloned().unwrap_or_default();
    if commands.is_empty() {
        return "error: no commands provided".to_string();
    }
    let mut results: Vec<String> = Vec::new();
    for (i, cmd) in commands.iter().enumerate() {
        let tool_name = cmd["tool"].as_str().unwrap_or("");
        if tool_name.is_empty() {
            results.push(format!("[{}] error: no tool name", i + 1));
            continue;
        }
        // Build the arguments from the command, excluding the 'tool' key
        let arguments = {
            let mut map = serde_json::Map::new();
            for (k, v) in cmd.as_object().unwrap_or(&serde_json::Map::new()) {
                if k != "tool" {
                    map.insert(k.clone(), v.clone());
                }
            }
            Value::Object(map).to_string()
        };
        let tc = crate::api::ToolCall {
            id: format!("batch-{}", i),
            name: tool_name.to_string(),
            arguments,
        };
        let result = crate::tools::execute_tool(&tc).await;
        // Truncate large results to save context
        let display = if result.len() > 500 {
            format!("{}... (truncated)", &result[..500])
        } else {
            result
        };
        results.push(format!("[Step {}] {}:\n{}", i + 1, tool_name, display));
    }
    format!("Batch complete ({} steps):\n{}", results.len(), results.join("\n\n"))
}

// ── Memory: persistent cross-session storage ────────────────

fn memory_path() -> std::path::PathBuf {
    crate::utils::dscode_dir().join("memory.md")
}

/// Load memory content for injection into system prompt.
pub(crate) fn load_memory() -> String {
    let path = memory_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            format!("\n## User Memory\n{}\n", trimmed)
        } else {
            String::new()
        }
    } else {
        String::new()
    }
}

pub(crate) fn exec_remember(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let key = v["key"].as_str().unwrap_or("");
    let value = v["value"].as_str().unwrap_or("");
    if key.is_empty() || value.is_empty() {
        return "error: both key and value required".to_string();
    }
    let path = memory_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let entry = format!("{}: {}\n", key.trim(), value.trim());
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(e) = file.write_all(entry.as_bytes()) {
                return format!("error writing memory: {e}");
            }
            format!("remembered: {} = {}", key, value)
        }
        Err(e) => format!("error opening memory: {e}"),
    }
}

pub(crate) fn exec_recall(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    if query.is_empty() {
        return "error: no query provided".to_string();
    }
    let path = memory_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return "no memory found".to_string(),
    };
    let q_lower = query.to_lowercase();
    let mut matches: Vec<&str> = content.lines()
        .filter(|l| l.to_lowercase().contains(&q_lower))
        .collect();
    if matches.is_empty() {
        return format!("nothing remembered matching '{query}'");
    }
    if matches.len() > 20 {
        matches.truncate(20);
    }
    format!("Memory matches for '{}':\n{}", query, matches.join("\n"))
}

// ── User input tool ───────────────────────────────────────────

pub(crate) async fn exec_request_user_input(_args: &str) -> String {
    eprintln!("\n\x1B[33m[Agent needs input]\x1B[0m Type your response and press Enter:");
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => input.trim().to_string(),
        Err(_) => "input cancelled".to_string(),
    }
}

// ── Shell execution ───────────────────────────────────────────

pub(crate) fn exec_run_shell(ctx: &ToolCtx, args: &str) -> String {
    use crate::tools::policy_engine;
    use codewhale_execpolicy::{AskForApproval, ExecPolicyContext};

    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let cmd_str = v["command"].as_str().unwrap_or("");
    if cmd_str.is_empty() {
        return "error: no command".to_string();
    }
    let engine = policy_engine();
    let decision = engine.check(ExecPolicyContext {
        command: cmd_str,
        cwd: &ctx.cwd.to_string_lossy(),
        ask_for_approval: AskForApproval::UnlessTrusted,
        sandbox_mode: None,
    });
    match decision {
        Ok(d) if !d.allow => return format!("blocked: {}", d.reason()),
        Ok(_) => {}
        Err(e) => return format!("policy error: {e}"),
    }
    match std::process::Command::new("sh")
        .args(["-c", cmd_str])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(output) => {
            let mut out = String::new();
            if !output.stdout.is_empty() {
                out.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            if out.len() > 40000 {
                out = format!("{}... (truncated, {} total)", &out[..40000], out.len());
            }
            if !output.status.success() {
                out = format!("exit code {}: {}", output.status.code().unwrap_or(-1), out);
            }
            out
        }
        Err(e) => format!("exec error: {e}"),
    }
}
