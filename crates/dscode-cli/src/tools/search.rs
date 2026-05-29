/// Search and discovery tools (Grep, Fuzzy, Web).

use crate::tools::{cwd_join, ToolCtx};
use serde_json::Value;

// ── Search code (grep) ────────────────────────────────────────

pub(crate) fn exec_search_code(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let pattern = v["pattern"].as_str().unwrap_or("");
    let search_path = v["path"].as_str().unwrap_or(".");
    if pattern.is_empty() {
        return "no pattern provided".to_string();
    }
    let full_search_path = cwd_join(ctx, search_path);
    let mut results = Vec::new();
    let cmd = std::process::Command::new("grep")
        .args([
            "-rn", "--include=*.rs", "--include=*.toml", "--include=*.md",
            "--include=*.html", "--include=*.sh", "--include=*.yml",
            "--include=*.json", "--include=*.css", "--include=*.js", "--include=*.ts",
        ])
        .args(["-e", pattern])
        .arg(&full_search_path)
        .output();
    match cmd {
        Ok(output) if output.status.success() => {
            let out = String::from_utf8_lossy(&output.stdout);
            for line in out.lines().take(60) {
                results.push(line.to_string());
            }
            if results.is_empty() {
                format!("no matches for '{pattern}'")
            } else {
                results.join("\n")
            }
        }
        Ok(_) => format!("no matches for '{pattern}'"),
        Err(e) => format!("search failed: {e}"),
    }
}

// ── File search (fuzzy filename) ──────────────────────────────

pub(crate) fn exec_file_search(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    let search_path = v["path"].as_str().unwrap_or(".");
    let limit = v["limit"].as_u64().unwrap_or(20).min(100) as usize;
    if query.is_empty() {
        return "no query provided".to_string();
    }
    let root = cwd_join(ctx, search_path);
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            if results.len() >= limit {
                break;
            }
            let path = entry.path();
            if path.file_name().map_or(false, |n| {
                n.to_string_lossy().to_lowercase().contains(&query_lower)
            }) {
                if let Ok(rel) = path.strip_prefix(&root) {
                    results.push(format!("  {}", rel.display()));
                } else {
                    results.push(format!("  {}", path.display()));
                }
            }
            if path.is_dir() {
                dirs.push(path);
            }
        }
        if results.len() >= limit {
            break;
        }
    }
    if results.is_empty() {
        format!("no files matching '{query}'")
    } else {
        format!("{} files matching '{query}':\n{}", results.len(), results.join("\n"))
    }
}

// ── Web search ────────────────────────────────────────────────

pub(crate) fn exec_web_search(_ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    if query.is_empty() {
        return "no query provided".to_string();
    }
    let encoded = urlencoding(query);
    match std::process::Command::new("curl")
        .args(["-s", "-L", "-o", "-", "--max-time", "10",
            &format!("https://lite.duckduckgo.com/lite/?q={}", encoded)])
        .output()
    {
        Ok(output) if output.status.success() => {
            let html = String::from_utf8_lossy(&output.stdout);
            extract_duckduckgo_results(&html)
        }
        _ => match std::process::Command::new("curl")
            .args(["-s", "-L", "-o", "-", "--max-time", "10",
                &format!("https://api.duckduckgo.com/?q={}&format=json", encoded)])
            .output()
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).to_string()
            }
            _ => "web search unavailable (install curl)".to_string(),
        },
    }
}

// ── Fetch URL ─────────────────────────────────────────────────

pub(crate) fn exec_fetch_url(_ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let url = v["url"].as_str().unwrap_or("");
    if url.is_empty() {
        return "no URL provided".to_string();
    }
    match std::process::Command::new("curl")
        .args(["-s", "-L", "-o", "-", "--max-time", "15", url])
        .output()
    {
        Ok(output) if output.status.success() => {
            let mut body = String::from_utf8_lossy(&output.stdout).to_string();
            if body.len() > 10000 {
                body = format!("{}... (truncated, {} total)", &body[..10000], body.len());
            }
            body
        }
        Ok(output) => format!("fetch failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)),
        Err(e) => format!("fetch failed: {e}"),
    }
}

// ── Helpers ───────────────────────────────────────────────────

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => '+'.to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

fn extract_duckduckgo_results(html: &str) -> String {
    let mut results = Vec::new();
    for line in html.lines() {
        let line = line.trim();
        if line.starts_with("<a") && line.contains("class=\"result-link\"") {
            if let Some(start) = line.find("href=\"") {
                let rest = &line[start + 6..];
                if let Some(end) = rest.find('"') {
                    let url = &rest[..end];
                    results.push(format!("  {url}"));
                }
            }
        }
        if line.starts_with("<td") && !results.is_empty() {
            let text = line
                .replace("<td>", "").replace("</td>", "")
                .replace("<b>", "").replace("</b>", "")
                .replace("&amp;", "&").replace("&quot;", "\"")
                .replace("&#x27;", "'")
                .trim().to_string();
            if !text.is_empty() {
                if let Some(last) = results.last_mut() {
                    last.push_str(&format!("\n    {text}"));
                }
            }
        }
    }
    if results.is_empty() {
        if html.contains("No results") {
            return "no results".to_string();
        }
        return "could not extract results".to_string();
    }
    results.join("\n")
}
