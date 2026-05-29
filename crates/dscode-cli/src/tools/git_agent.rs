/// Git commands, review, FIM edit, sub-agents, checklist, and test runner.
///
/// Async + sync helpers consumed by the DscHandler dispatcher in mod.rs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::tools::{cwd_join, ToolCtx};

// ── Git read tools ────────────────────────────────────────────

pub(crate) fn exec_git_log(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let max_count = v["max_count"].as_u64().unwrap_or(20).min(100);
    let path = v["path"].as_str().unwrap_or("");
    let mut cmd = std::process::Command::new("git");
    cmd.args(["log", "--oneline", "-n", &max_count.to_string()]);
    if !path.is_empty() { cmd.arg(path); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => format!("git log failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git log error: {e}"),
    }
}

pub(crate) fn exec_git_show(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let rev = v["rev"].as_str().unwrap_or("");
    if rev.is_empty() { return "error: no rev provided".to_string(); }
    let output = std::process::Command::new("git")
        .args(["show", "--stat", "--patch", rev])
        .current_dir(&ctx.cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            if s.len() > 8000 { s = format!("{}... (truncated, {} total)", &s[..8000], s.len()); }
            s
        }
        Ok(o) => format!("git show failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git show error: {e}"),
    }
}

pub(crate) fn exec_git_blame(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    if path.is_empty() { return "error: no path provided".to_string(); }
    let full_path = cwd_join(ctx, path);
    let start = v["start_line"].as_u64().unwrap_or(1).max(1);
    let max_lines = v["max_lines"].as_u64().unwrap_or(200).min(1000);
    let end = start + max_lines - 1;
    match std::process::Command::new("git")
        .args(["blame", &format!("-L{start},{end}"), "--", &full_path.to_string_lossy()])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => format!("git blame failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git blame error: {e}"),
    }
}

// ── Git write tools ───────────────────────────────────────────

pub(crate) fn exec_git_status(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    let mut cmd = std::process::Command::new("git");
    cmd.args(["status", "--short", "--branch"]);
    if !path.is_empty() { cmd.arg(path); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("git status failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git status error: {e}"),
    }
}

pub(crate) fn exec_git_diff(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    let cached = v["cached"].as_bool().unwrap_or(false);
    let mut cmd = std::process::Command::new("git");
    cmd.arg("diff");
    if cached { cmd.arg("--cached"); }
    if !path.is_empty() { cmd.arg(path); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            if out.is_empty() { "no changes".to_string() } else { out }
        }
        Ok(o) => format!("git diff failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git diff error: {e}"),
    }
}

pub(crate) fn exec_git_add(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or(".");
    match std::process::Command::new("git")
        .args(["add", path])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => format!("staged {path}"),
        Ok(o) => format!("git add failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git add error: {e}"),
    }
}

pub(crate) fn exec_git_commit(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let msg = v["message"].as_str().unwrap_or("");
    if msg.is_empty() { return "error: no commit message".to_string(); }
    match std::process::Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("commit failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("commit error: {e}"),
    }
}

pub(crate) fn exec_git_push(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let remote = v["remote"].as_str().unwrap_or("origin");
    let branch = v["branch"].as_str().unwrap_or("");
    let mut cmd = std::process::Command::new("git");
    cmd.args(["push", remote]);
    if !branch.is_empty() { cmd.arg(branch); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if out.is_empty() { "pushed successfully".to_string() } else { out }
        }
        Ok(o) => format!("push failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("push error: {e}"),
    }
}

// ── Review tool ───────────────────────────────────────────────

use crate::api::{resolve_api_key, resolve_base_url};

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
        "model": "deepseek-v4-pro",
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
        "model": "deepseek-v4-flash",
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
            match std::fs::write(&full_path, &new_content) {
                Ok(_) => format!("fim_edit applied to {path} ({} chars generated)", generated.len()),
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
    api_key: &str, base_url: &str, _cwd: &std::path::Path,
    prompt: &str, context_msgs: &[Value],
) -> String {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut api_msgs: Vec<Value> = if context_msgs.is_empty() {
        vec![json!({"role": "user", "content": prompt})]
    } else {
        let mut msgs = context_msgs.to_vec();
        msgs.push(json!({"role": "user", "content": prompt}));
        msgs
    };
    for _ in 0..8 {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": api_msgs,
            "max_tokens": 4096,
            "stream": false,
        });
        let tools = crate::api::tool_definitions();
        if !tools.is_empty() { body["tools"] = Value::Array(tools); }
        let resp = match client.post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body).send().await
        {
            Ok(r) => r,
            Err(e) => return format!("error: {e}"),
        };
        let data: Value = match resp.json().await {
            Ok(d) => d,
            Err(_) => return "parse error".to_string(),
        };
        let msg = &data["choices"][0]["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let tool_calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
        if tool_calls.is_empty() {
            return if content.is_empty() { "(empty)".to_string() } else { content };
        }
        let mut assistant = json!({"role": "assistant", "content": content});
        assistant["tool_calls"] = Value::Array(tool_calls.clone());
        api_msgs.push(assistant);
        for tc in &tool_calls {
            let name = tc["function"]["name"].as_str().unwrap_or("");
            let arguments = tc["function"]["arguments"].as_str().unwrap_or("{}");
            let tool_call = crate::api::ToolCall {
                id: tc["id"].as_str().unwrap_or("").to_string(),
                name: name.to_string(),
                arguments: arguments.to_string(),
            };
            let mut result = crate::api::execute_tool(&tool_call).await;
            if result.len() > 4000 {
                result = format!("{}... (truncated)", &result[..4000]);
            }
            api_msgs.push(json!({"role": "tool", "tool_call_id": tool_call.id, "content": result}));
        }
    }
    "max rounds reached".to_string()
}

pub(crate) async fn exec_agent_open(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let prompt = v["prompt"].as_str().unwrap_or("");
    if prompt.is_empty() { return "error: no prompt".to_string(); }
    let Some(api_key) = resolve_api_key() else { return "error: no API key".to_string() };
    let base_url = resolve_base_url();
    let agent_id = format!("agent-{}", Uuid::new_v4());
    let agents = global_agents().clone();
    let cwd = ctx.cwd.clone();
    let pk = api_key.clone();
    let bu = base_url.clone();
    let pr = prompt.to_string();
    let ctx_msgs: Vec<Value> = Vec::new();
    agents.lock().unwrap().insert(agent_id.clone(), SubAgentState {
        status: "running".into(), result: String::new(), created_at: Utc::now().timestamp(),
    });
    let id = agent_id.clone();
    tokio::spawn(async move {
        let result = run_sub_agent(&pk, &bu, &cwd, &pr, &ctx_msgs).await;
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

// ── User input tool ───────────────────────────────────────────

pub(crate) async fn exec_user_input(_args: &str) -> String {
    eprintln!("\n\x1B[33m[Agent needs input]\x1B[0m Type your response and press Enter:");
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => input.trim().to_string(),
        Err(_) => "input cancelled".to_string(),
    }
}
