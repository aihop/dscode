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
    pub reasoning_content: String,
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
                "description": "Read the contents of a file. Path relative to project root.",
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
                "name": "write_file",
                "description": "Create or overwrite a file with content. Creates parent dirs if needed. Use for new files or full rewrites.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path relative to project root"},
                        "content": {"type": "string", "description": "Full file content"}
                    },
                    "required": ["path", "content"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Replace text in an existing file. Use for surgical edits, not full rewrites. Searches for exact old text and replaces it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path relative to project root"},
                        "old": {"type": "string", "description": "Existing text to find (exact match)"},
                        "new": {"type": "string", "description": "Replacement text"}
                    },
                    "required": ["path", "old", "new"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_shell",
                "description": "Execute a shell command in the project root directory. Returns stdout+stderr. Blocked: rm -rf /, dd, mkfs, format, :(){ :|:& };:.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Shell command to run"}
                    },
                    "required": ["command"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_code",
                "description": "Search for a pattern in project files (grep). Returns matches with file names.",
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
                "description": "List files and dirs in a path. Shows type (dir/file) and name.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path relative to project root"}
                    },
                    "required": ["path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "git_status",
                "description": "Show git working tree status (changed, staged, untracked files).",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "git_diff",
                "description": "Show git diff of unstaged changes. If path given, scope to that file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Optional file path to scope diff"}
                    },
                    "required": []
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "git_commit",
                "description": "Stage all changes and create a git commit with the given message. Use git_status first to review what will be committed.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "message": {"type": "string", "description": "Commit message"}
                    },
                    "required": ["message"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "git_log",
                "description": "Show recent git commit history.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "count": {"type": "integer", "description": "Number of commits (default 10)"}
                    },
                    "required": []
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web for information. Returns ranked results with snippets and URLs.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Search query"}
                    },
                    "required": ["query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "fetch_url",
                "description": "Fetch the content of a URL (HTTP GET). Returns the body as text.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "HTTP/HTTPS URL to fetch"}
                    },
                    "required": ["url"]
                }
            }
        }),
    ]
}

/// Execute a tool call and return the result as a string.
/// Runs inside the project root cwd.
/// Safety: blocks destructive commands (rm -rf /, dd, etc.)
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
        "write_file" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let path_str = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            if path_str.is_empty() { return "error: no path provided".to_string(); }
            let full_path = if path_str.starts_with('/') { PathBuf::from(path_str) } else { cwd.join(path_str) };
            if let Some(parent) = full_path.parent() { std::fs::create_dir_all(parent).ok(); }
            match std::fs::write(&full_path, content) {
                Ok(_) => format!("written {} ({} bytes)", full_path.display(), content.len()),
                Err(e) => format!("error writing {}: {e}", full_path.display()),
            }
        }
        "edit_file" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let path_str = args["path"].as_str().unwrap_or("");
            let old = args["old"].as_str().unwrap_or("");
            let new = args["new"].as_str().unwrap_or("");
            if path_str.is_empty() { return "error: no path".to_string(); }
            let full_path = if path_str.starts_with('/') { PathBuf::from(path_str) } else { cwd.join(path_str) };
            match std::fs::read_to_string(&full_path) {
                Ok(content) => {
                    if !content.contains(old) {
                        return format!("error: exact match not found in {}", full_path.display());
                    }
                    let new_content = content.replace(old, new);
                    match std::fs::write(&full_path, &new_content) {
                        Ok(_) => format!("edited {}", full_path.display()),
                        Err(e) => format!("error writing {}: {e}", full_path.display()),
                    }
                }
                Err(e) => format!("error reading {}: {e}", full_path.display()),
            }
        }
        "run_shell" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let cmd_str = args["command"].as_str().unwrap_or("");
            if cmd_str.is_empty() { return "error: no command".to_string(); }
            // Safety: block destructive commands
            let lower = cmd_str.to_lowercase();
            let blocked = ["rm -rf /", "rm -rf /*", "dd if=", "mkfs.", "format ", ":(){ :|:& };:"];
            if blocked.iter().any(|b| lower.contains(b)) {
                return "blocked: destructive command not allowed".to_string();
            }
            match std::process::Command::new("sh")
                .args(["-c", cmd_str])
                .current_dir(&cwd)
                .output()
            {
                Ok(output) => {
                    let mut out = String::new();
                    if !output.stdout.is_empty() {
                        out.push_str(&String::from_utf8_lossy(&output.stdout));
                    }
                    if !output.stderr.is_empty() {
                        if !out.is_empty() { out.push('\n'); }
                        out.push_str(&String::from_utf8_lossy(&output.stderr));
                    }
                    if out.len() > 10000 {
                        out = format!("{}... (truncated, {} total)", &out[..10000], out.len());
                    }
                    if !output.status.success() {
                        out = format!("exit code {}: {}", output.status.code().unwrap_or(-1), out);
                    }
                    out
                }
                Err(e) => format!("exec error: {e}"),
            }
        }
        "git_status" => {
            let out = run_cmd(&cwd, "git", &["status", "--short", "-b"]);
            if out.is_empty() { "clean working tree".to_string() } else { out }
        }
        "git_diff" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let path = args["path"].as_str().unwrap_or("");
            if path.is_empty() { run_cmd(&cwd, "git", &["diff"]) }
            else { run_cmd(&cwd, "git", &["diff", "--", path]) }
        }
        "git_commit" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let msg = args["message"].as_str().unwrap_or("");
            if msg.is_empty() { return "error: no commit message".to_string(); }
            // Stage all, then commit
            let add = run_cmd(&cwd, "git", &["add", "-A"]);
            let commit = run_cmd(&cwd, "git", &["commit", "-m", msg]);
            format!("staged:\n{add}\n\ncommit:\n{commit}")
        }
        "git_log" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let count = args["count"].as_u64().unwrap_or(10).to_string();
            run_cmd(&cwd, "git", &["log", "--oneline", &format!("-{count}")])
        }
        "web_search" => {
            let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or_default();
            let query = args["query"].as_str().unwrap_or("");
            if query.is_empty() { return "no query".to_string(); }
            // Use DuckDuckGo's lite HTML API
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build().ok();
            match client {
                Some(c) => {
                    match c.get("https://lite.duckduckgo.com/lite/")
                        .query(&[("q", query)])
                        .header("User-Agent", "dscode/0.1")
                        .send()
                    {
                        Ok(resp) => {
                            let html = resp.text().unwrap_or_default();
                            // Simple extraction: find result links
                            let mut results = Vec::new();
                            for line in html.lines() {
                                if line.contains("<a rel=\"nofollow\" href=\"") {
                                    if let Some(href_start) = line.find("href=\"") {
                                        let rest = &line[href_start + 6..];
                                        if let Some(href_end) = rest.find('\"') {
                                            let url = &rest[..href_end];
                                            // Find text after the link
                                            results.push(format!("  {url}"));
                                        }
                                    }
                                }
                            }
                            if results.is_empty() { format!("no results for '{query}'") }
                            else { format!("web search results for '{query}':\n{}", results.join("\n")) }
                        }
                        Err(e) => format!("search failed: {e}"),
                    }
                }
                None => "search unavailable (client error)".to_string(),
            }
        }
        "fetch_url" => {
            let url = tc.arguments.trim_matches('"');
            if url.is_empty() { return "no url".to_string(); }
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build().ok();
            match client {
                Some(c) => {
                    match c.get(url).header("User-Agent", "dscode/0.1").send() {
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().unwrap_or_default();
                            let max_len = 8000;
                            let body = if body.len() > max_len {
                                format!("{}... (truncated, {} total)", &body[..max_len], body.len())
                            } else { body };
                            format!("HTTP {status}\n\n{body}")
                        }
                        Err(e) => format!("fetch failed: {e}"),
                    }
                }
                None => "fetch unavailable".to_string(),
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
                    // Only close reasoning when we get actual content (not empty delta after tool calls)
                    if showed_reasoning && !delta.is_empty() { showed_reasoning = false; eprintln!(); }
                    full.push_str(delta);
                    usage.tokens_out += delta.len() as u64 / 4;
                    // Track column for narrow terminal wrapping + clear residual chars
                    if narrow {
                        for ch in delta.chars() {
                            if ch == '\n' {
                                // Clear next line to avoid residual chars
                                print!("\r\x1B[2K");
                                col = 0;
                            } else {
                                col += 1;
                                if col >= max_col && ch.is_whitespace() {
                                    print!("\n\r\x1B[2K");
                                    col = 0;
                                }
                            }
                        }
                    }
                    // Lightweight inline markdown → ANSI on each delta
                    let rendered = md_inline(delta);
                    print!("{rendered}");
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
    Ok(StreamResult { content: full, reasoning_content: reasoning, tool_calls: final_calls, usage })
}

/// Lightweight inline Markdown → ANSI for streaming deltas.
/// Handles **bold**, *italic*, `code` — no newlines added, safe for fragments.
fn md_inline(text: &str) -> String {
    let mut s = text.to_string();
    s = replace_pattern(&s, "**", "\x1B[1m", "\x1B[22m");
    s = replace_pattern(&s, "*", "\x1B[3m", "\x1B[23m");
    s = replace_inline_code(&s);
    s
}

/// Full Markdown → ANSI rendering for display of complete messages.
/// Supports: **bold**, *italic*, `code`, # headings, - lists, ```blocks```, > quotes.
/// Supports: **bold**, *italic*, `code`, # headings, - lists, ```blocks```, > quotes.
pub fn md_to_ansi(text: &str) -> String {
    // Process line by line for block-level formatting
    let mut out = String::new();
    let mut in_code_block = false;
    let mut code_buf = String::new();

    let mut code_lang = String::new();

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                // End code block — render with syntax highlight
                let lang_label = if code_lang.is_empty() { "code".to_string() } else { code_lang.clone() };
                out.push_str(&format!("\x1B[90m─── {} ───\x1B[0m\n", lang_label));
                out.push_str(&highlight_code(&code_buf, &code_lang));
                out.push_str(&format!("\x1B[90m{}\x1B[0m\n", "─".repeat(16)));
                code_buf.clear();
                code_lang.clear();
                in_code_block = false;
            } else {
                // Opening fence — capture language
                let rest = line.trim_start().trim_start_matches("```").trim();
                code_lang = rest.to_string();
                in_code_block = true;
            }
            continue;
        }
        if in_code_block {
            code_buf.push_str(line);
            code_buf.push('\n');
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }

        // Headings
        if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push_str(&format!("\x1B[1;34m{}\x1B[0m\n", rest));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push_str(&format!("\x1B[1;36m{}\x1B[0m\n", rest));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push_str(&format!("\x1B[1m{}\x1B[0m\n", rest));
            continue;
        }

        // Block quotes
        if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push_str(&format!("\x1B[90m> {}\x1B[0m\n", rest));
            continue;
        }

        // List items
        let prefix = if trimmed.starts_with("- ") { "  • " } else if trimmed.starts_with("* ") { "  • " } else if trimmed.chars().next().map_or(false, |c| c.is_ascii_digit()) && trimmed.contains(". ") { "" } else { "" };

        // Inline formatting
        let mut inline = if !prefix.is_empty() { format!("{}{}", prefix, &trimmed[2..]) } else { line.to_string() };
        // **bold**
        inline = replace_pattern(&inline, "**", "\x1B[1m", "\x1B[22m");
        // *italic*
        inline = replace_pattern(&inline, "*", "\x1B[3m", "\x1B[23m");
        // `code`
        inline = replace_inline_code(&inline);

        out.push_str(&inline);
        out.push('\n');
    }

    // Close unclosed code block
    if in_code_block && !code_buf.is_empty() {
        out.push_str(&format!("\x1B[90m─── code ───\x1B[0m\n{}\n\x1B[90m────────────\x1B[0m\n", code_buf));
    }

    out
}

fn replace_pattern(text: &str, delim: &str, open: &str, close: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    let mut toggle = true;
    while let Some(pos) = rest.find(delim) {
        result.push_str(&rest[..pos]);
        if toggle { result.push_str(open); } else { result.push_str(close); }
        toggle = !toggle;
        rest = &rest[pos + delim.len()..];
    }
    result.push_str(rest);
    // Close unclosed
    if !toggle { result.push_str(close); }
    result
}

fn replace_inline_code(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    let mut toggle = true;
    while let Some(pos) = rest.find('`') {
        result.push_str(&rest[..pos]);
        if toggle { result.push_str("\x1B[36m"); } else { result.push_str("\x1B[0m"); }
        toggle = !toggle;
        rest = &rest[pos + 1..];
    }
    result.push_str(rest);
    if !toggle { result.push_str("\x1B[0m"); }
    result
}

/// Language-specific keyword lists for syntax highlighting
fn lang_keywords(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" | "rs" => &[
            "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait",
            "async", "await", "match", "if", "else", "for", "while", "loop", "return",
            "true", "false", "Some", "None", "Ok", "Err", "self", "Super", "crate",
            "where", "type", "const", "static", "unsafe", "ref", "move", "as", "in",
            "dyn", "impl", "pub", "super", "self", "String", "Vec", "Box", "Result",
        ],
        "python" | "py" => &[
            "def", "class", "return", "if", "elif", "else", "for", "while", "import",
            "from", "as", "try", "except", "finally", "with", "yield", "lambda",
            "True", "False", "None", "self", "async", "await", "in", "not", "and", "or",
            "print", "len", "range", "int", "str", "list", "dict", "set", "tuple",
        ],
        "javascript" | "js" | "typescript" | "ts" => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while",
            "class", "import", "export", "from", "async", "await", "try", "catch",
            "true", "false", "null", "undefined", "new", "this", "typeof",
            "console", "log", "require", "module",
        ],
        "go" | "golang" => &[
            "func", "return", "if", "else", "for", "range", "var", "const", "type",
            "struct", "interface", "map", "chan", "go", "defer", "select", "case",
            "switch", "package", "import", "nil", "true", "false", "make", "len",
            "error", "string", "int", "bool", "byte", "rune",
        ],
        "json" => &["true", "false", "null"],
        _ => &[],
    }
}

/// Simple syntax highlighter for code blocks.
/// Colors: keywords=blue, strings=green, comments=gray, numbers=yellow
fn highlight_code(code: &str, lang: &str) -> String {
    let keywords = lang_keywords(lang);
    let mut out = String::new();

    for line in code.lines() {
        let trimmed = line.trim();
        // Comment line
        let comment_prefixes = ["//", "#", "--", "//"];
        if comment_prefixes.iter().any(|p| trimmed.starts_with(p)) {
            out.push_str(&format!("\x1B[90m{}\x1B[0m\n", line));
            continue;
        }

        // Tokenize and color
        let mut result = String::new();
        let mut rest = line;
        while !rest.is_empty() {
            // String literals (double and single quoted)
            if let Some(pos) = rest.find(|c| c == '"' || c == '\'') {
                result.push_str(&rest[..pos]);
                let quote = rest[pos..].chars().next().unwrap();
                result.push_str(&rest[pos..pos+1]); // opening quote
                let inner_start = pos + 1;
                if let Some(end) = rest[inner_start..].find(quote) {
                    let inner = &rest[inner_start..inner_start + end];
                    result.push_str(&format!("\x1B[32m{}\x1B[0m{}", inner, quote));
                    rest = &rest[inner_start + end + 1..];
                } else {
                    // Unclosed string
                    result.push_str(&format!("\x1B[32m{}\x1B[0m", &rest[inner_start..]));
                    rest = "";
                }
                continue;
            }

            // Split by word boundaries
            let word_end = rest.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(rest.len());
            let word = &rest[..word_end];
            let after = if word_end < rest.len() { &rest[word_end..word_end+1] } else { "" };

            // Keyword highlighting
            if !word.is_empty() && keywords.contains(&word) {
                result.push_str(&format!("\x1B[34m{word}\x1B[0m"));
            } else {
                // Number highlighting
                if word.chars().all(|c| c.is_ascii_digit() || c == '.') && !word.is_empty() {
                    result.push_str(&format!("\x1B[33m{word}\x1B[0m"));
                } else {
                    result.push_str(word);
                }
            }
            result.push_str(after);
            rest = &rest[word_end + after.len()..];
        }
        out.push_str(&result);
        out.push('\n');
    }
    out
}

/// Helper: run a command and return stdout+stderr as string
fn run_cmd(cwd: &std::path::Path, program: &str, args: &[&str]) -> String {
    match std::process::Command::new(program).args(args).current_dir(cwd).output() {
        Ok(o) => {
            let mut out = String::new();
            if !o.stdout.is_empty() { out.push_str(&String::from_utf8_lossy(&o.stdout)); }
            if !o.stderr.is_empty() {
                if !out.is_empty() { out.push('\n'); }
                out.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            out.trim().to_string()
        }
        Err(e) => format!("error: {e}"),
    }
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
