/// Terminal Markdown rendering and ANSI formatting.
///
/// Pure rendering — no API, no I/O beyond stdio terminal detection.
/// Extracted from api.rs to keep files under 400 lines.

use std::io::{self, IsTerminal};

// ── Line rendering ──────────────────────────────────────────

/// Render one complete line with Markdown formatting.
/// Inside code blocks: syntax-highlighted when lang is known.
/// Outside: full md_to_ansi.
pub fn render_line(line: &str, in_code: bool, lang: &str) -> String {
    if in_code {
        // Syntax-highlighted line inside a code block
        format!("\x1B[90m│\x1B[0m {}", highlight_code_line(line, lang))
    } else {
        // Full Markdown rendering per line
        md_to_ansi_line(line)
    }
}

/// Render a single line (no newline added) with full Markdown → ANSI.
fn md_to_ansi_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return line.to_string();
    }

    // Headings
    if let Some(rest) = trimmed.strip_prefix("# ") {
        return format!("\x1B[1;34m{}\x1B[0m", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        return format!("\x1B[1;36m{}\x1B[0m", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("### ") {
        return format!("\x1B[1m{}\x1B[0m", rest);
    }

    // Block quotes
    if let Some(rest) = trimmed.strip_prefix("> ") {
        return format!("\x1B[90m> {}\x1B[0m", rest);
    }

    // List items (unordered and ordered)
    let body;
    let list_prefix = list_prefix_for(trimmed);
    if let Some(ref _p) = list_prefix {
        let skip = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            2
        } else {
            trimmed.find(". ").map(|p| p + 2).unwrap_or(2)
        };
        body = if skip < trimmed.len() {
            &trimmed[skip..]
        } else {
            ""
        };
    } else {
        body = line;
    }

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

/// Determine list prefix for a trimmed line.
fn list_prefix_for(trimmed: &str) -> Option<String> {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        Some("  • ".to_string())
    } else if let Some(dot_pos) = trimmed.find(". ") {
        if dot_pos > 0 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
            Some(format!("  {}. ", &trimmed[..dot_pos]))
        } else {
            None
        }
    } else {
        None
    }
}

// ── Full Markdown rendering ─────────────────────────────────

/// Full Markdown → ANSI rendering for display of complete messages.
/// Supports: **bold**, *italic*, `code`, # headings, - lists, ```blocks```, > quotes.
pub fn md_to_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;
    let mut code_buf = String::new();
    let mut code_lang = String::new();
    let mut table_buf: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                let lang_label = if code_lang.is_empty() {
                    "code".to_string()
                } else {
                    code_lang.clone()
                };
                out.push_str(&format!("\x1B[90m─── {} ───\x1B[0m\n", lang_label));
                out.push_str(&highlight_code(&code_buf, &code_lang));
                out.push_str(&format!("\x1B[90m{}\x1B[0m\n", "─".repeat(16)));
                code_buf.clear();
                code_lang.clear();
                in_code_block = false;
            } else {
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

        if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push_str(&format!("\x1B[90m> {}\x1B[0m\n", rest));
            continue;
        }

        let list_prefix = list_prefix_for(trimmed);
        let content = if let Some(ref _p) = list_prefix {
            let skip = if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                2
            } else {
                trimmed.find(". ").map(|p| p + 2).unwrap_or(2)
            };
            if skip < trimmed.len() {
                &trimmed[skip..]
            } else {
                ""
            }
        } else {
            line
        };

        let mut inline = if let Some(ref p) = list_prefix {
            format!("{}{}", p, content)
        } else {
            content.to_string()
        };
        inline = replace_pattern(&inline, "**", "\x1B[1m", "\x1B[22m");
        inline = replace_pattern(&inline, "*", "\x1B[3m", "\x1B[23m");
        inline = replace_inline_code(&inline);

        out.push_str(&inline);
        out.push('\n');
    }

    if !table_buf.is_empty() {
        out.push_str(&render_table_str(&table_buf));
    }

    if in_code_block && !code_buf.is_empty() {
        out.push_str(&format!(
            "\x1B[90m─── code ───\x1B[0m\n{}\n\x1B[90m────────────\x1B[0m\n",
            code_buf
        ));
    }

    out
}

// ── Table rendering ────────────────────────────────────────

/// Render a Markdown table from buffered rows with column alignment.
pub fn render_table(rows: &[String]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let parsed: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' '))
        .map(|r| {
            r.trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect()
        })
        .collect();

    if parsed.is_empty() {
        return String::new();
    }

    let num_cols = parsed.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return String::new();
    }

    let mut col_widths = vec![0usize; num_cols];
    for row in &parsed {
        for (i, cell) in row.iter().enumerate() {
            let w = display_width(cell);
            col_widths[i] = col_widths[i].max(w);
        }
    }

    let mut out = String::new();
    for row in &parsed {
        out.push_str("  ");
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            let w = display_width(cell);
            let pad = col_widths[i].saturating_sub(w);
            out.push_str(cell);
            if pad > 0 {
                out.push_str(&" ".repeat(pad));
            }
        }
        out.push('\n');
    }
    out
}

/// Fallback: convert &[&str] to owned Vec and delegate.
fn render_table_str(rows: &[&str]) -> String {
    let owned: Vec<String> = rows.iter().map(|s| s.to_string()).collect();
    render_table(&owned)
}

// ── ANSI utilities ─────────────────────────────────────────

/// Strip ANSI escape sequences for piped output.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut esc = false;
    for b in s.bytes() {
        if esc {
            if b == b'm' || b == b'H' || b == b'K' || b == b'J' {
                esc = false;
            }
            continue;
        }
        if b == 0x1b {
            esc = true;
            continue;
        }
        out.push(b as char);
    }
    out
}

/// Check if stdout is a terminal (vs piped to file or another command).
pub fn use_color() -> bool {
    io::stdout().is_terminal()
}

/// Print with ANSI stripping when piped.
pub fn oprint(text: &str) {
    if use_color() {
        print!("{text}");
    } else {
        print!("{}", strip_ansi(text));
    }
}

// ── Display width ──────────────────────────────────────────

/// Approximate display width of a string (CJK/emoji = 2, ASCII = 1).
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            if cp > 0x2E80 {
                2
            } else {
                1
            }
        })
        .sum()
}

// ── Inline formatting patterns ─────────────────────────────

fn replace_pattern(text: &str, delim: &str, open: &str, close: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    let mut toggle = true;
    while let Some(pos) = rest.find(delim) {
        result.push_str(&rest[..pos]);
        if toggle {
            result.push_str(open);
        } else {
            result.push_str(close);
        }
        toggle = !toggle;
        rest = &rest[pos + delim.len()..];
    }
    result.push_str(rest);
    if !toggle {
        result.push_str(close);
    }
    result
}

fn replace_inline_code(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    let mut toggle = true;
    while let Some(pos) = rest.find('`') {
        result.push_str(&rest[..pos]);
        if toggle {
            result.push_str("\x1B[36m");
        } else {
            result.push_str("\x1B[0m");
        }
        toggle = !toggle;
        rest = &rest[pos + 1..];
    }
    result.push_str(rest);
    if !toggle {
        result.push_str("\x1B[0m");
    }
    result
}

// ── Syntax highlighting ────────────────────────────────────

/// Language-specific keyword lists for syntax highlighting.
fn lang_keywords(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" | "rs" => &[
            "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait", "async",
            "await", "match", "if", "else", "for", "while", "loop", "return", "true", "false",
            "Some", "None", "Ok", "Err", "self", "Super", "crate", "where", "type", "const",
            "static", "unsafe", "ref", "move", "as", "in", "dyn", "impl", "pub", "super", "self",
            "String", "Vec", "Box", "Result",
        ],
        "python" | "py" => &[
            "def", "class", "return", "if", "elif", "else", "for", "while", "import", "from",
            "as", "try", "except", "finally", "with", "yield", "lambda", "True", "False", "None",
            "self", "async", "await", "in", "not", "and", "or", "print", "len", "range", "int",
            "str", "list", "dict", "set", "tuple",
        ],
        "javascript" | "js" | "typescript" | "ts" => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while", "class",
            "import", "export", "from", "async", "await", "try", "catch", "true", "false", "null",
            "undefined", "new", "this", "typeof", "console", "log", "require", "module",
        ],
        "go" | "golang" => &[
            "func", "return", "if", "else", "for", "range", "var", "const", "type", "struct",
            "interface", "map", "chan", "go", "defer", "select", "case", "switch", "package",
            "import", "nil", "true", "false", "make", "len", "error", "string", "int", "bool",
            "byte", "rune",
        ],
        "json" => &["true", "false", "null"],
        _ => &[],
    }
}

/// Highlight a single line of code with ANSI colors.
fn highlight_code_line(line: &str, lang: &str) -> String {
    let keywords = lang_keywords(lang);
    let trimmed = line.trim();
    let comment_prefixes = ["//", "#", "--"];
    if comment_prefixes.iter().any(|p| trimmed.starts_with(p)) {
        return format!("\x1B[90m{}\x1B[0m\n", line);
    }

    let mut result = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        if let Some(pos) = rest.find(|c| c == '"' || c == '\'') {
            let quote_len = rest[pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            result.push_str(&rest[..pos]);
            let quote = &rest[pos..pos + quote_len];
            let inner_start = pos + 1;
            if let Some(end) = rest[inner_start..].find(quote) {
                let inner = &rest[inner_start..inner_start + end];
                result.push_str(&format!("\x1B[32m{}\x1B[0m{}", inner, quote));
                rest = &rest[inner_start + end + 1..];
            } else {
                result.push_str(&format!("\x1B[32m{}\x1B[0m", &rest[inner_start..]));
                rest = "";
            }
            continue;
        }

        let word_end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let word = &rest[..word_end];
        let after = if word_end < rest.len() {
            rest[word_end..]
                .chars()
                .next()
                .map(|c| {
                    let end = word_end + c.len_utf8();
                    &rest[word_end..end]
                })
                .unwrap_or("")
        } else {
            ""
        };

        if !word.is_empty() && keywords.contains(&word) {
            result.push_str(&format!("\x1B[34m{word}\x1B[0m"));
        } else if word.chars().all(|c| c.is_ascii_digit() || c == '.') && !word.is_empty() {
            result.push_str(&format!("\x1B[33m{word}\x1B[0m"));
        } else {
            result.push_str(word);
        }
        result.push_str(after);
        rest = &rest[word_end + after.len()..];
    }
    result.push('\n');
    result
}

/// Simple syntax highlighter for code blocks (multi-line).
/// Colors: keywords=blue, strings=green, comments=gray, numbers=yellow
fn highlight_code(code: &str, lang: &str) -> String {
    let mut out = String::new();
    for line in code.lines() {
        out.push_str(&highlight_code_line(line, lang));
    }
    out
}
