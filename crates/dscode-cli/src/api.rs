/// Shared DeepSeek API client layer.
///
/// Thin connector — dscode engine doesn't expose a simple
/// "send → stream" API, so this bridges the gap.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::render;

// ── Constants ─────────────────────────────────────────────────

/// V4 has 1M context — keep tool results intact up to 120K chars by default.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 40_000;

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

/// Ensure a default config exists. Creates one on first run.
pub fn ensure_default_config() {
    let path = config_path();
    if path.exists() { return; }
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    let mut store = match codewhale_config::ConfigStore::load(Some(path.clone())) {
        Ok(s) => s,
        Err(_) => codewhale_config::ConfigStore::load(Some(path.clone()))
            .expect(" config should load"),
    };
    store.config.providers.deepseek.model = Some("deepseek-v4-flash".to_string());
    store.config.providers.deepseek.base_url = Some("https://api.deepseek.com/beta".to_string());
    if let Err(e) = store.save() {
        eprintln!("warning: failed to create default config: {e}");
    }
}

// ── Types ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct UsageInfo {
    pub model: String,
    pub tokens_out: u64,
    pub reasoning_tokens: u64,
    pub prompt_tokens: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
}

#[derive(Debug)]
pub struct StreamResult {
    pub content: String,
    pub reasoning_content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: UsageInfo,
    pub finish_reason: Option<String>,
}

#[derive(Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// ── Tool definitions → now in crate::tools ──────────────────

/// Re-export tool_definitions from crate::tools.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    crate::tools::tool_definitions()
}

/// Re-export tool_definitions_filtered from crate::tools.
pub fn tool_definitions_filtered(names: &[&str]) -> Vec<serde_json::Value> {
    crate::tools::tool_definitions_filtered(names)
}

/// Execute a tool call and return the result as a string.
/// Delegates to crate::tools for the actual implementation.
pub async fn execute_tool(tc: &ToolCall) -> String {
    crate::tools::execute_tool(tc).await
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
    silent: bool,
    terminal_width: u16,
    reasoning_effort: Option<&str>,
) -> Result<StreamResult, String> {
    use futures_util::StreamExt;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "max_tokens": 65536,
    });
    if let Some(re) = reasoning_effort {
        body["reasoning_effort"] = serde_json::Value::String(re.to_string());
    }
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
    let mut line_buf = String::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut stream = response.bytes_stream();
    let mut col: u16 = 0;
    let max_col = terminal_width.saturating_sub(2);
    let mut usage = UsageInfo::default();
    let mut finish_reason: Option<String> = None;
    // Tool call accumulation (streamed deltas)
    let mut tool_calls: BTreeMap<usize, ToolCall> = BTreeMap::new();

    loop {
        let chunk = match stream.next().await {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                if !silent {
                    eprintln!("\x1B[33m\n[网络错误: {e}]\x1B[0m");
                }
                break;
            }
            None => break,
        };
        for line in String::from_utf8_lossy(&chunk).lines() {
            let data = match line.trim() {
                l if l.is_empty() || l == "data: [DONE]" => continue,
                l => match l.strip_prefix("data: ") { Some(s) => s, None => continue },
            };
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                // Finish reason (diagnose truncation)
                if let Some(fr) = parsed["choices"][0]["finish_reason"].as_str() {
                    finish_reason = Some(fr.to_string());
                    if fr == "length" && !silent {
                        eprintln!("\x1B[33m\n[响应被 token 上上限截断]\x1B[0m");
                    }
                }
                // Exact usage + cache stats
                if let Some(u) = parsed.get("usage") {
                    if let Some(t) = u["completion_tokens"].as_u64() { usage.tokens_out = t; }
                    if let Some(t) = u["completion_tokens_details"]["reasoning_tokens"].as_u64() { usage.reasoning_tokens = t; }
                    if let Some(t) = u["prompt_tokens"].as_u64() { usage.prompt_tokens = t; }
                }

                // Content delta
                let delta = parsed["choices"][0]["delta"]["content"].as_str().unwrap_or("");
                let reasoning_delta = parsed["choices"][0]["delta"]["reasoning_content"].as_str().unwrap_or("");

                // Accumulate and print reasoning
                if !reasoning_delta.is_empty() {
                    reasoning.push_str(reasoning_delta);
                    if !silent {
                        // Print reasoning in gray, handle newlines
                        for ch in reasoning_delta.chars() {
                            if ch == '\n' {
                                print!("\n");
                            } else {
                                print!("\x1B[90m{ch}\x1B[0m");
                            }
                        }
                        io::stdout().flush().ok();
                    }
                }

                if !delta.is_empty() {
                    full.push_str(delta);
                    if narrow {
                        // Narrow terminal: word-wrap per character, break on whitespace
                        let mut out = String::new();
                        for ch in delta.chars() {
                            if ch == '\n' {
                                out.push_str(&render::render_line(&line_buf, in_code_block, &code_lang));
                                out.push('\n');
                                // Update code block status after rendering the line
                                let trimmed = line_buf.trim();
                                if trimmed.starts_with("```") {
                                    if !in_code_block { code_lang = trimmed.trim_start_matches("```").trim().to_string(); }
                                    else { code_lang.clear(); }
                                    in_code_block = !in_code_block;
                                }
                                line_buf.clear();
                                col = 0;
                            } else {
                                line_buf.push(ch);
                                let char_w = if ch as u32 > 0x2E80 { 2 } else { 1 };
                                col += char_w;
                                // Force wrap even without whitespace if we hit the absolute limit
                                // but prefer wrapping at whitespace.
                                if col >= max_col {
                                    if ch.is_whitespace() || col >= max_col + 10 {
                                        out.push_str(&render::render_line(&line_buf, in_code_block, &code_lang));
                                        out.push('\n');
                                        line_buf.clear();
                                        col = 0;
                                    }
                                }
                            }
                        }
                        if !out.is_empty() && !silent { render::oprint(&out); io::stdout().flush().ok(); }
                    } else {
                        // Non-narrow: still use render_line but don't force wrap as aggressively
                        let mut out = String::new();
                        for ch in delta.chars() {
                            if ch == '\n' {
                                out.push_str(&render::render_line(&line_buf, in_code_block, &code_lang));
                                out.push('\n');
                                let trimmed = line_buf.trim();
                                if trimmed.starts_with("```") {
                                    if !in_code_block { code_lang = trimmed.trim_start_matches("```").trim().to_string(); }
                                    else { code_lang.clear(); }
                                    in_code_block = !in_code_block;
                                }
                                line_buf.clear();
                            } else {
                                line_buf.push(ch);
                                // For non-narrow, we can print characters directly if they are not 
                                // part of a potential markdown trigger, but for simplicity 
                                // and correctness of rendering, we still buffer lines.
                                // However, we can at least ensure we don't skip the last line.
                            }
                        }
                        if !out.is_empty() && !silent { render::oprint(&out); io::stdout().flush().ok(); }
                    }
                }
                // Non-streaming tool_calls (from message)
                if let Some(tc_array) = parsed["choices"][0]["message"]["tool_calls"].as_array() {
                    for tc in tc_array {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_calls.entry(idx).or_insert(ToolCall {
                            id: String::new(), name: String::new(), arguments: String::new(),
                        });
                        if let Some(id) = tc["id"].as_str() { entry.id = id.to_string(); }
                        if let Some(name) = tc["function"]["name"].as_str() { entry.name = name.to_string(); }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.arguments = args.to_string();
                        } else if !tc["function"]["arguments"].is_null() {
                            entry.arguments = tc["function"]["arguments"].to_string();
                        }
                    }
                }
                // Streaming tool_calls (from delta)
                if let Some(tc_array) = parsed["choices"][0]["delta"]["tool_calls"].as_array() {
                    for tc in tc_array {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_calls.entry(idx).or_insert(ToolCall {
                            id: String::new(), name: String::new(), arguments: String::new(),
                        });
                        if let Some(id) = tc["id"].as_str() { entry.id = id.to_string(); }
                        if let Some(name) = tc["function"]["name"].as_str() { entry.name = name.to_string(); }
                        if let Some(args) = tc["function"]["arguments"].as_str() {
                            entry.arguments.push_str(args);
                        }
                    }
                }
            }
        }
    }

    if !line_buf.is_empty() && !silent {
        let flushed = render::render_line(&line_buf, in_code_block, &code_lang);
        if !flushed.is_empty() { render::oprint(&flushed); io::stdout().flush().ok(); }
    }
    if !silent { println!(); }

    // Robust JSON repair for tool call arguments (handle mobile network interruptions)
    for tc in tool_calls.values_mut() {
        if !tc.arguments.is_empty() {
            tc.arguments = repair_json(&tc.arguments);
        }
    }

    Ok(StreamResult {
        content: full,
        reasoning_content: reasoning,
        tool_calls: tool_calls.into_values().collect(),
        usage,
        finish_reason,
    })
}

/// Attempt to repair incomplete JSON by appending missing closing delimiters.
/// Essential for handling partial tool call streaming in unstable mobile networks.
fn repair_json(input: &str) -> String {
    if serde_json::from_str::<serde_json::Value>(input).is_ok() {
        return input.to_string();
    }
    let mut repaired = input.to_string();
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for c in input.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if c == '{' || c == '[' {
            stack.push(c);
        } else if c == '}' {
            if stack.last() == Some(&'{') { stack.pop(); }
        } else if c == ']' {
            if stack.last() == Some(&'[') { stack.pop(); }
        }
    }

    if in_string {
        repaired.push('"');
    }
    while let Some(c) = stack.pop() {
        if c == '{' {
            repaired.push('}');
        } else if c == '[' {
            repaired.push(']');
        }
    }
    repaired
}

/// Load AGENT.md / AGENTS.md / CLAUDE.md from project root — cached once per session.
pub fn load_agent_md() -> Option<String> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let cwd = std::env::current_dir().ok()?;
        for name in &["AGENT.md", "AGENTS.md", "CLAUDE.md"] {
            let path = cwd.join(name);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        return Some(format!("Project context from {}:\n{}", name, trimmed));
                    }
                }
            }
        }
        None
    }).clone()
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
        "model": model, "messages": messages, "stream": false, "max_tokens": 65536,
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
    let u = &data["usage"];
    let usage = UsageInfo {
        model: model.to_string(),
        tokens_out: u["completion_tokens"].as_u64().unwrap_or(0),
        reasoning_tokens: u["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0),
        prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
        cache_hit_tokens: u["prompt_cache_hit_tokens"].as_u64().unwrap_or(0),
        cache_miss_tokens: u["prompt_cache_miss_tokens"].as_u64().unwrap_or(0),
    };
    let content = data["choices"][0]["message"]["content"].as_str().unwrap_or("(no response)").to_string();
    if render::use_color() { print!("{}", render::md_to_ansi(&content)); } else { println!("{content}"); }
    Ok((content, usage))
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repair_json_already_valid() {
        assert_eq!(repair_json(r#"{"path": "src/main.rs"}"#), r#"{"path": "src/main.rs"}"#);
        assert_eq!(repair_json(r#"{"a":1,"b":[2,3]}"#), r#"{"a":1,"b":[2,3]}"#);
        assert_eq!(repair_json(r#"[]"#), r#"[]"#);
        assert_eq!(repair_json(r#"{}"#), r#"{}"#);
    }

    #[test]
    fn test_repair_json_missing_brace() {
        let repaired = repair_json(r#"{"path": "src/main.rs""#);
        assert!(repaired.ends_with('}'), "should close the object");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed["path"], "src/main.rs");
    }

    #[test]
    fn test_repair_json_missing_bracket() {
        let repaired = repair_json(r#"{"items": [1, 2, 3"#);
        assert!(repaired.contains(']'), "should close the array");
        assert!(repaired.ends_with('}'), "should close the object");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed["items"][2], 3);
    }

    #[test]
    fn test_repair_json_nested() {
        let repaired = repair_json(r#"{"outer": {"inner": [1, 2"#);
        assert!(repaired.ends_with('}'), "should end with closing outer object");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed["outer"]["inner"][1], 2);
    }

    #[test]
    fn test_repair_json_unclosed_string() {
        let repaired = repair_json(r#"{"key": "unfinished"#);
        assert!(repaired.contains(r#""unfinished""#), "should close the string");
        assert!(repaired.ends_with('}'), "should close the object");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed["key"], "unfinished");
    }

    #[test]
    fn test_repair_json_empty() {
        assert_eq!(repair_json(""), "");
    }

    #[test]
    fn test_repair_json_array_only() {
        let repaired = repair_json(r#"["a", "b"#);
        assert!(repaired.ends_with(']'), "should close the array");
        let parsed: serde_json::Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed[0], "a");
        assert_eq!(parsed[1], "b");
    }

    #[test]
    fn test_resolve_model_name() {
        assert_eq!(resolve_model_name("v4-pro"), "deepseek-v4-pro");
        assert_eq!(resolve_model_name("flash"), "deepseek-v4-flash");
        assert_eq!(resolve_model_name("r1"), "deepseek-r1");
        assert_eq!(resolve_model_name(""), "");
        assert_eq!(resolve_model_name("custom"), "custom");
        // Unknown names pass through
        assert_eq!(resolve_model_name("V4-PRO"), "V4-PRO");
        assert_eq!(resolve_model_name("my-model"), "my-model");
    }

    #[test]
    fn test_default_model_names() {
        let flash = default_model(true);
        assert!(flash.contains("flash"), "flash=true should return flash model");
        let pro = default_model(false);
        assert!(pro.contains("pro"), "flash=false should return pro model");
    }
}
