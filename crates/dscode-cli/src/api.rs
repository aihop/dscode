/// Shared DeepSeek API client layer.
///
/// Thin connector — dscode engine doesn't expose a simple
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

// ── Tool definitions → now in crate::tools ──────────────────

/// Re-export tool_definitions from crate::tools.
pub fn tool_definitions() -> Vec<serde_json::Value> {
    crate::tools::tool_definitions()
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
    let mut line_buf = String::new();
    let mut in_code_block = false;
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
                // Reasoning — dimmed gray on stderr, no noisy [reasoning] tags
                if let Some(rt) = parsed["choices"][0]["delta"]["reasoning_content"].as_str() {
                    usage.reasoning_tokens += rt.len() as u64 / 4;
                    reasoning.push_str(rt);
                    if !rt.is_empty() {
                        eprint!("\x1B[90m{}\x1B[0m", rt);
                        io::stderr().flush().ok();
                    }
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
                    full.push_str(delta);
                    usage.tokens_out += delta.len() as u64 / 4;
                    // Render content: buffer lines for proper block-level Markdown
                    for ch in delta.chars() {
                        if ch == '\n' {
                            // Complete line — render with full Markdown support
                            let rendered = render_line(&line_buf, in_code_block);
                            print!("{rendered}");
                            // Track code fence state
                            let trimmed = line_buf.trim();
                            if trimmed.starts_with("```") {
                                in_code_block = !in_code_block;
                            }
                            line_buf.clear();
                            if narrow { print!("\r\x1B[2K"); col = 0; }
                        } else {
                            line_buf.push(ch);
                            if narrow {
                                col += 1;
                                if col >= max_col && ch.is_whitespace() {
                                    print!("\n\r\x1B[2K");
                                    col = 0;
                                }
                            }
                        }
                    }
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

    // Flush remaining content in line buffer
    if !line_buf.is_empty() {
        let rendered = render_line(&line_buf, in_code_block);
        print!("{rendered}");
    }
    println!();
    let final_calls: Vec<ToolCall> = tool_calls.into_values().filter(|t| !t.name.is_empty()).collect();
    Ok(StreamResult { content: full, reasoning_content: reasoning, tool_calls: final_calls, usage })
}

/// Render one complete line with Markdown formatting.
/// Inside code blocks: raw output. Outside: full md_to_ansi.
fn render_line(line: &str, in_code: bool) -> String {
    if in_code {
        // Inside code block: just add line break (full block assembled in md_to_ansi)
        format!("{}\n", line)
    } else {
        // Full Markdown rendering per line
        md_to_ansi_line(line)
    }
}

/// Render a single line (no newline added) with full Markdown → ANSI.
fn md_to_ansi_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() { return line.to_string(); }

    // Headings
    if let Some(rest) = trimmed.strip_prefix("# ") {
        return format!("\x1B[1;34m{}\x1B[0m\n", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        return format!("\x1B[1;36m{}\x1B[0m\n", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("### ") {
        return format!("\x1B[1m{}\x1B[0m\n", rest);
    }
    // Block quotes
    if let Some(rest) = trimmed.strip_prefix("> ") {
        return format!("\x1B[90m> {}\x1B[0m\n", rest);
    }
    // Code fence start/end
    if trimmed.starts_with("```") {
        let lang = trimmed.trim_start_matches("```").trim();
        let label = if lang.is_empty() { "code" } else { lang };
        return format!("\x1B[90m─── {} ───\x1B[0m\n", label);
    }
    // List items
    let prefix = if trimmed.starts_with("- ") { "  • " }
        else if trimmed.starts_with("* ") { "  • " }
        else if trimmed.starts_with("  ") { "  " }
        else { "" };
    let body = if !prefix.is_empty() && trimmed.len() > 2 {
        &trimmed[2..]
    } else {
        line
    };

    // Inline formatting on the body text
    let mut s = body.to_string();
    s = replace_pattern(&s, "**", "\x1B[1m", "\x1B[22m");
    s = replace_pattern(&s, "*", "\x1B[3m", "\x1B[23m");
    s = replace_inline_code(&s);

    if !prefix.is_empty() {
        format!("{}{}\n", prefix, s)
    } else {
        format!("{}\n", s)
    }
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
