/// Shared DeepSeek API client layer.
///
/// Thin connector — CodeWhale engine doesn't expose a simple
/// "send → stream" API, so this bridges the gap.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

// ── Model resolution ──────────────────────────────────────────

pub fn resolve_model_name(input: &str) -> String {
    match input {
        "v4-pro" | "v4pro"       => "deepseek-v4-pro",
        "v4-flash" | "v4flash" | "flash" => "deepseek-v4-flash",
        "v3"                      => "deepseek-v3",
        "v3.2" | "v32"           => "deepseek-v3.2",
        "r1"                      => "deepseek-r1",
        "chat"                    => "deepseek-chat",
        "reasoner"                => "deepseek-reasoner",
        "coder"                   => "deepseek-coder",
        other                     => other,
    }.to_string()
}

pub fn default_model(flash: bool) -> String {
    let path = config_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(path)) {
        if let Some(m) = store.config.providers.deepseek.model { if !m.is_empty() { return m; } }
    }
    if flash { "deepseek-v4-flash" } else { "deepseek-v4-pro" }.to_string()
}

pub fn resolve_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") { if !key.trim().is_empty() { return Some(key); } }
    let path = config_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(path)) {
        if let Some(key) = store.config.api_key { if !key.trim().is_empty() { return Some(key); } }
    }
    None
}

pub fn resolve_base_url() -> String {
    let path = config_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(path)) {
        if let Some(url) = store.config.providers.deepseek.base_url { return url; }
    }
    "https://api.deepseek.com/beta".to_string()
}

fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config")).join("dscode").join("config.toml")
}

// ── Types ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct UsageInfo {
    pub model: String,
    pub tokens_out: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug)]
pub struct StreamResult {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: UsageInfo,
}

#[derive(Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// ── Tool definitions (function calling) ───────────────────────

pub fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read the contents of a file. Path is relative to the project root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path relative to project root"}
                    },
                    "required": ["path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_code",
                "description": "Search for a pattern in project files (grep). Returns matching lines with file names.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Search pattern (regex)"},
                        "path": {"type": "string", "description": "Optional subdirectory to search"}
                    },
                    "required": ["pattern"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List files and directories in a path.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path relative to project root"}
                    },
                    "required": ["path"]
                }
            }
        }),
    ]
}

/// Execute a tool call and return the result as a string.
/// Runs inside the project root cwd.
pub fn execute_tool(tc: &ToolCall) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match tc.name.as_str() {
        "read_file" => {
            let path_str = tc.arguments.trim_matches('"');
            let full_path = if path_str.starts_with('/') {
                PathBuf::from(path_str)
            } else {
                cwd.join(path_str)
            };
            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    let lines: Vec<&str> = content.lines().collect();
                    let max_lines = 500;
                    if lines.len() > max_lines {
                        let head: Vec<&str> = lines[..max_lines].to_vec();
                        format!("{} (showing first {max_lines} of {} lines)\n{}", 
                            full_path.display(), lines.len(), head.join("\n"))
                    } else {
                        format!("{} ({} lines)\n{}", full_path.display(), lines.len(), content)
                    }
                }
                Err(e) => format!("error reading {}: {e}", full_path.display()),
            }
        }
        "search_code" => {
            // Parse args: {"pattern": "...", "path": "..."}
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let pattern = args["pattern"].as_str().unwrap_or("");
            let search_path = args["path"].as_str().unwrap_or(".");
            if pattern.is_empty() { return "no pattern provided".to_string(); }
            let full_search_path = if search_path.starts_with('/') {
                PathBuf::from(search_path)
            } else {
                cwd.join(search_path)
            };
            let mut results = Vec::new();
            let cmd = std::process::Command::new("grep")
                .args(["-rn", "--include=*.rs", "--include=*.toml", "--include=*.md",
                       "--include=*.html", "--include=*.sh", "--include=*.yml", "--include=*.json",
                       "--include=*.css", "--include=*.js", "--include=*.ts"])
                .args(["-e", pattern])
                .arg(&full_search_path)
                .output();
            match cmd {
                Ok(output) if output.status.success() => {
                    let out = String::from_utf8_lossy(&output.stdout);
                    for line in out.lines().take(60) {
                        results.push(line.to_string());
                    }
                    if results.is_empty() { format!("no matches for '{pattern}'") }
                    else { results.join("\n") }
                }
                Ok(_) => format!("no matches for '{pattern}'"),
                Err(e) => format!("search failed: {e}"),
            }
        }
        "list_files" => {
            let path_str = tc.arguments.trim_matches('"');
            let full_path = if path_str.starts_with('/') {
                PathBuf::from(path_str)
            } else {
                cwd.join(path_str)
            };
            match std::fs::read_dir(&full_path) {
                Ok(entries) => {
                    let mut items: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            let ty = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) { "dir" } else { "file" };
                            format!("  {ty:4}  {name}")
                        })
                        .collect();
                    items.sort();
                    format!("{} ({} entries):\n{}", full_path.display(), items.len(), items.join("\n"))
                }
                Err(e) => format!("error listing {}: {e}", full_path.display()),
            }
        }
        _ => format!("unknown tool: {}", tc.name),
    }
}

// ── API calls ─────────────────────────────────────────────────

/// Call DeepSeek chat completions with optional tools (function calling).
/// Returns content + any tool_calls from the model.
pub async fn call_stream(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
    tools: Option<&[serde_json::Value]>,
    narrow: bool,
    terminal_width: u16,
) -> Result<StreamResult, String> {
    use futures_util::StreamExt;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "max_tokens": 8192,
    });
    if let Some(t) = tools {
        if !t.is_empty() {
            body["tools"] = serde_json::Value::Array(t.to_vec());
        }
    }

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API {status}: {text}"));
    }

    let mut full = String::new();
    let mut reasoning = String::new();
    let mut showed_reasoning = false;
    let mut stream = response.bytes_stream();
    let mut col: u16 = 0;
    let max_col = terminal_width.saturating_sub(2);
    let mut usage = UsageInfo::default();
    // Tool call accumulation (streamed deltas)
    let mut tool_calls: BTreeMap<usize, ToolCall> = BTreeMap::new();

    loop {
        let chunk = match stream.next().await {
            Some(Ok(c)) => c,
            Some(Err(_)) => break,
            None => break,
        };
        for line in String::from_utf8_lossy(&chunk).lines() {
            let data = match line.trim() {
                l if l.is_empty() || l == "data: [DONE]" => continue,
                l => match l.strip_prefix("data: ") { Some(s) => s, None => continue },
            };
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                // Exact usage
                if let Some(u) = parsed.get("usage") {
                    if let Some(t) = u["completion_tokens"].as_u64() { usage.tokens_out = t; }
                    if let Some(t) = u["completion_tokens_details"]["reasoning_tokens"].as_u64() { usage.reasoning_tokens = t; }
                }
                // Reasoning
                if let Some(rt) = parsed["choices"][0]["delta"]["reasoning_content"].as_str() {
                    usage.reasoning_tokens += rt.len() as u64 / 4;
                    reasoning.push_str(rt);
                    if !showed_reasoning { showed_reasoning = true; eprint!("\n[reasoning] "); }
                    io::stderr().flush().ok();
                }
                // Tool calls (streaming deltas)
                if let Some(tc_array) = parsed["choices"][0]["delta"]["tool_calls"].as_array() {
                    for tc_delta in tc_array {
                        let idx = tc_delta["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_calls.entry(idx).or_insert(ToolCall {
                            id: String::new(), name: String::new(), arguments: String::new(),
                        });
                        if let Some(id) = tc_delta["id"].as_str() { entry.id = id.to_string(); }
                        if let Some(name) = tc_delta["function"]["name"].as_str() { entry.name = name.to_string(); }
                        if let Some(args) = tc_delta["function"]["arguments"].as_str() { entry.arguments.push_str(args); }
                    }
                }
                // Content
                if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                    if showed_reasoning { showed_reasoning = false; eprintln!(); }
                    full.push_str(delta);
                    usage.tokens_out += delta.len() as u64 / 4;
                    if narrow {
                        for ch in delta.chars() {
                            if ch == '\n' { col = 0; } else {
                                col += 1;
                                if col >= max_col && ch.is_whitespace() { print!("\n"); col = 0; }
                            }
                        }
                    }
                    print!("{delta}");
                    io::stdout().flush().ok();
                }
                // Non-streaming tool_calls (from finish_reason)
                if let Some(tc_array) = parsed["choices"][0]["message"]["tool_calls"].as_array() {
                    for tc in tc_array {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_calls.entry(idx).or_insert(ToolCall {
                            id: String::new(), name: String::new(), arguments: String::new(),
                        });
                        if let Some(id) = tc["id"].as_str() { entry.id = id.to_string(); }
                        if let Some(name) = tc["function"]["name"].as_str() { entry.name = name.to_string(); }
                        if let Some(args) = tc["function"]["arguments"].as_str() { entry.arguments = args.to_string(); }
                    }
                }
            }
        }
    }

    if showed_reasoning { eprintln!(); }
    println!();
    let final_calls: Vec<ToolCall> = tool_calls.into_values().filter(|t| !t.name.is_empty()).collect();
    Ok(StreamResult { content: full, tool_calls: final_calls, usage })
}

/// Non-streaming call (no tool support needed for now, but returns content)
pub async fn call_nonstream(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
) -> Result<(String, UsageInfo), String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model, "messages": messages, "stream": false, "max_tokens": 8192,
    });
    let response = client.post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send().await.map_err(|e| format!("connection failed: {e}"))?;
    if !response.status().is_success() {
        let s = response.status(); let t = response.text().await.unwrap_or_default();
        return Err(format!("API {s}: {t}"));
    }
    let data: serde_json::Value = response.json().await.map_err(|e| format!("parse: {e}"))?;
    let usage = UsageInfo {
        model: model.to_string(),
        tokens_out: data["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        reasoning_tokens: data["usage"]["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0),
    };
    let content = data["choices"][0]["message"]["content"].as_str().unwrap_or("(no response)").to_string();
    println!("{content}");
    Ok((content, usage))
}
