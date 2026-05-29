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
    let mut in_table = false;
    let mut table_buf: Vec<String> = Vec::new();
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
                // Exact usage + cache stats
                if let Some(u) = parsed.get("usage") {
                    if let Some(t) = u["completion_tokens"].as_u64() { usage.tokens_out = t; }
                    if let Some(t) = u["completion_tokens_details"]["reasoning_tokens"].as_u64() { usage.reasoning_tokens = t; }
                    if let Some(t) = u["prompt_tokens"].as_u64() { usage.prompt_tokens = t; }
                    if let Some(t) = u["prompt_cache_hit_tokens"].as_u64() { usage.cache_hit_tokens = t; }
                    if let Some(t) = u["prompt_cache_miss_tokens"].as_u64() { usage.cache_miss_tokens = t; }
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

                    if narrow {
                        // Narrow: character-by-character for word wrap, batch output
                        let mut out = String::new();
                        for ch in delta.chars() {
                            if ch == '\n' {
                                // Table detection
                                let trimmed = line_buf.trim();
                                if trimmed.starts_with('|') {
                                    table_buf.push(line_buf.clone());
                                    in_table = true;
                                    line_buf.clear();
                                    out.push_str("\r\x1B[2K");
                                    col = 0;
                                    continue;
                                }
                                if in_table && !trimmed.is_empty() {
                                    out.push_str(&render_table(&table_buf));
                                    table_buf.clear();
                                    in_table = false;
                                }
                                out.push_str(&render_line(&line_buf, in_code_block));
                                if trimmed.starts_with("```") { in_code_block = !in_code_block; }
                                line_buf.clear();
                                out.push_str("\r\x1B[2K");
                                col = 0;
                            } else {
                                line_buf.push(ch);
                                col += 1;
                                if col >= max_col && ch.is_whitespace() {
                                    out.push_str("\n\r\x1B[2K");
                                    col = 0;
                                }
                            }
                        }
                        if !out.is_empty() { print!("{out}"); io::stdout().flush().ok(); }
                    } else {
                        // Non-narrow: batch lines, skip character-by-character iteration
                        if delta.contains('\n') {
                            let mut out = String::new();
                            for segment in delta.split_inclusive('\n') {
                                if let Some(content) = segment.strip_suffix('\n') {
                                    line_buf.push_str(content);
                                    // Table detection
                                    let trimmed = line_buf.trim();
                                    if trimmed.starts_with('|') {
                                        table_buf.push(line_buf.clone());
                                        in_table = true;
                                        line_buf.clear();
                                        continue;
                                    }
                                    if in_table && !trimmed.is_empty() {
                                        out.push_str(&render_table(&table_buf));
                                        table_buf.clear();
                                        in_table = false;
                                    }
                                    out.push_str(&render_line(&line_buf, in_code_block));
                                    if trimmed.starts_with("```") { in_code_block = !in_code_block; }
                                    line_buf.clear();
                                } else {
                                    line_buf.push_str(segment);
                                }
                            }
                            if !out.is_empty() { print!("{out}"); io::stdout().flush().ok(); }
                        } else {
                            // No newlines — just accumulate for the next chunk
                            line_buf.push_str(delta);
                        }
                    }
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
    // Flush any buffered table
    if in_table && !table_buf.is_empty() {
        print!("{}", render_table(&table_buf));
        table_buf.clear();
        in_table = false;
    }
    if line_buf.is_empty() && !in_table {
        // Only add newline if there was any output at all
        println!();
    }
    let final_calls: Vec<ToolCall> = tool_calls.into_values().filter(|t| !t.name.is_empty()).collect();
    Ok(StreamResult { content: full, reasoning_content: reasoning, tool_calls: final_calls, usage })
}

/// Render one complete line with Markdown formatting.
/// Inside code blocks: raw output (except closing ``` which shows a separator).
/// Outside: full md_to_ansi.
fn render_line(line: &str, in_code: bool) -> String {
    if in_code {
        if line.trim_start().starts_with("```") {
            format!("\x1B[90m{}\x1B[0m\n", "─".repeat(16))
        } else {
            format!("{}\n", line)
        }
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
    // List items (unordered and ordered)
    let list_prefix = if trimmed.starts_with("- ") { Some(format!("  • ")) }
        else if trimmed.starts_with("* ") { Some(format!("  • ")) }
        else {
            // Numbered list: "1. item", "12. item"
            let dot_pos = trimmed.find(". ");
            match dot_pos {
                Some(pos) if pos > 0 && trimmed[..pos].chars().all(|c| c.is_ascii_digit()) => {
                    Some(format!("  {}. ", &trimmed[..pos]))
                }
                _ => None,
            }
        };
    let body = if let Some(ref _p) = list_prefix {
        // Strip the list marker ("- ", "* ", "12. ")
        let skip = if trimmed.starts_with("- ") || trimmed.starts_with("* ") { 2 }
                    else { trimmed.find(". ").map(|p| p + 2).unwrap_or(2) };
        if skip < trimmed.len() { &trimmed[skip..] } else { "" }
    } else {
        line
    };

    // Inline formatting on the body text
    let mut s = body.to_string();
    s = replace_pattern(&s, "**", "\x1B[1m", "\x1B[22m");
    s = replace_pattern(&s, "*", "\x1B[3m", "\x1B[23m");
    s = replace_inline_code(&s);

    if let Some(p) = list_prefix {
        format!("{}{}\n", p, s)
    } else {
        format!("{}\n", s)
    }
}

/// Full Markdown → ANSI rendering for display of complete messages.
/// Supports: **bold**, *italic*, `code`, # headings, - lists, ```blocks```, > quotes.
pub fn md_to_ansi(text: &str) -> String {
    // Process line by line for block-level formatting
    let mut out = String::new();
    let mut in_code_block = false;
    let mut code_buf = String::new();

    let mut code_lang = String::new();
    let mut table_buf: Vec<&str> = Vec::new();

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

        // Table detection
        if line.trim_start().starts_with('|') {
            table_buf.push(line);
            continue;
        }
        if !table_buf.is_empty() {
            out.push_str(&render_table_str(&table_buf));
            table_buf.clear();
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

        // List items (unordered and ordered)
        let list_prefix = if trimmed.starts_with("- ") { Some("  • ".to_string()) }
            else if trimmed.starts_with("* ") { Some("  • ".to_string()) }
            else {
                let dot_pos = trimmed.find(". ");
                match dot_pos {
                    Some(pos) if pos > 0 && trimmed[..pos].chars().all(|c| c.is_ascii_digit()) => {
                        Some(format!("  {}. ", &trimmed[..pos]))
                    }
                    _ => None,
                }
            };
        let content = if let Some(ref _p) = list_prefix {
            let skip = if trimmed.starts_with("- ") || trimmed.starts_with("* ") { 2 }
                        else { trimmed.find(". ").map(|p| p + 2).unwrap_or(2) };
            if skip < trimmed.len() { &trimmed[skip..] } else { "" }
        } else {
            line
        };

        // Inline formatting
        let mut inline = if let Some(ref p) = list_prefix { format!("{}{}", p, content) } else { content.to_string() };
        // **bold**
        inline = replace_pattern(&inline, "**", "\x1B[1m", "\x1B[22m");
        // *italic*
        inline = replace_pattern(&inline, "*", "\x1B[3m", "\x1B[23m");
        // `code`
        inline = replace_inline_code(&inline);

        out.push_str(&inline);
        out.push('\n');
    }

    // Flush buffered table
    if !table_buf.is_empty() {
        out.push_str(&render_table_str(&table_buf));
        table_buf.clear();
    }

    // Close unclosed code block
    if in_code_block && !code_buf.is_empty() {
        out.push_str(&format!("\x1B[90m─── code ───\x1B[0m\n{}\n\x1B[90m────────────\x1B[0m\n", code_buf));
    }

    out
}

// ── Table rendering ────────────────────────────────────────────

/// Render a Markdown table from buffered rows with column alignment.
/// CJK/emoji characters count as double-width for alignment.
fn render_table(rows: &[String]) -> String {
    if rows.is_empty() { return String::new(); }

    // Parse cells, skip separator rows (|---|)
    let parsed: Vec<Vec<String>> = rows.iter()
        .map(|r| r.trim())
        .filter(|r| !r.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' '))
        .map(|r| {
            r.trim_start_matches('|').trim_end_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect()
        })
        .collect();

    if parsed.is_empty() { return String::new(); }

    let num_cols = parsed.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 { return String::new(); }

    // Calculate column widths
    let mut col_widths = vec![0usize; num_cols];
    for row in &parsed {
        for (i, cell) in row.iter().enumerate() {
            let w = display_width(cell);
            col_widths[i] = col_widths[i].max(w);
        }
    }

    // Render with 2-space gap between columns
    let mut out = String::new();
    for row in &parsed {
        out.push_str("  ");
        for (i, cell) in row.iter().enumerate() {
            if i > 0 { out.push_str("  "); }
            let w = display_width(cell);
            let pad = col_widths[i].saturating_sub(w);
            out.push_str(cell);
            if pad > 0 { out.push_str(&" ".repeat(pad)); }
        }
        out.push('\n');
    }
    out
}

/// Thin wrapper: convert &[&str] to owned Vec and delegate.
fn render_table_str(rows: &[&str]) -> String {
    let owned: Vec<String> = rows.iter().map(|s| s.to_string()).collect();
    render_table(&owned)
}

/// Approximate display width of a string (CJK/emoji = 2, ASCII = 1).
fn display_width(s: &str) -> usize {
    s.chars().map(|c| {
        let cp = c as u32;
        if cp > 0x2E80 { 2 } else { 1 }
    }).sum()
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
        let comment_prefixes = ["//", "#", "--"];
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
    print!("{}", md_to_ansi(&content));
    Ok((content, usage))
}
