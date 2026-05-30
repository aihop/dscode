/// Search and discovery tools (Grep, Fuzzy, Web).
///
/// Uses `ignore` crate for gitignore-aware file walking (no system grep dependency).
/// Falls back to curl for web/fetch (separate concern).

use crate::tools::{cwd_join, ToolCtx};
use serde_json::Value;

// ── Supported source extensions for code search ───────────────

const SEARCH_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "html", "sh", "yml", "yaml",
    "json", "css", "js", "ts", "tsx", "jsx", "py", "go", "c", "cpp", "h", "hpp",
];

const SYMBOL_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "cpp", "h", "hpp", "java",
];

// ── Helper: walk files respecting .gitignore, filter by extension ──

fn walk_files(root: &std::path::Path, extensions: &[&str]) -> Vec<std::path::PathBuf> {
    use ignore::WalkBuilder;
    let mut files = Vec::new();
    let walker = WalkBuilder::new(root)
        .standard_filters(true)    // respect .gitignore, hidden files, etc.
        .build();
    for entry in walker.flatten() {
        if entry.file_type().map_or(false, |t| t.is_file()) {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    files.push(entry.path().to_path_buf());
                }
            }
        }
    }
    files
}

// ── Search code (regex) ───────────────────────────────────────

pub(crate) fn exec_search_code(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let pattern_str = v["pattern"].as_str().unwrap_or("");
    let search_path = v["path"].as_str().unwrap_or(".");
    if pattern_str.is_empty() {
        return "no pattern provided".to_string();
    }
    let re = match regex::Regex::new(pattern_str) {
        Ok(r) => r,
        Err(e) => return format!("invalid regex '{pattern_str}': {e}"),
    };
    let root = cwd_join(ctx, search_path);
    if !root.exists() {
        return format!("path not found: {}", root.display());
    }
    let files = walk_files(&root, SEARCH_EXTENSIONS);
    if files.is_empty() {
        return format!("no searchable files found under {}", root.display());
    }
    let mut results = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f);
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                results.push(format!("{}:{}:{}", rel.display(), i + 1, line));
                if results.len() >= 60 {
                    break;
                }
            }
        }
        if results.len() >= 60 {
            break;
        }
    }
    if results.is_empty() {
        format!("no matches for '{pattern_str}'")
    } else {
        results.join("\n")
    }
}

// ── Symbol search (multi-pattern) ─────────────────────────────

pub(crate) fn exec_search_symbols(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    let search_path = v["path"].as_str().unwrap_or(".");
    if query.is_empty() {
        return "no query provided".to_string();
    }
    let q = regex::escape(query);
    let symbol_patterns = [
        format!(r"fn\s+{}\b", &q),
        format!(r"struct\s+{}\b", &q),
        format!(r"class\s+{}\b", &q),
        format!(r"enum\s+{}\b", &q),
        format!(r"trait\s+{}\b", &q),
        format!(r"type\s+{}\b", &q),
        format!(r"def\s+{}\b", &q),
        format!(r"pub\s+fn\s+{}\b", &q),
    ];
    let root = cwd_join(ctx, search_path);
    if !root.exists() {
        return format!("path not found: {}", root.display());
    }
    let files = walk_files(&root, SYMBOL_EXTENSIONS);
    let mut results = Vec::new();
    // Track which patterns matched which files for dedup
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f);
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for pat_str in &symbol_patterns {
            if let Ok(re) = regex::Regex::new(pat_str) {
                for (i, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        results.push(format!("{}:{}:{}", rel.display(), i + 1, line));
                        if results.len() >= 20 {
                            break;
                        }
                    }
                }
            }
            if results.len() >= 20 {
                break;
            }
        }
        if results.len() >= 20 {
            break;
        }
    }
    if results.is_empty() {
        format!("no definitions found for symbol '{}'", query)
    } else {
        format!("Definitions found for '{}':\n{}", query, results.join("\n"))
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

// ── Shared reqwest client ─────────────────────────────────────

fn http_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build().unwrap()
    })
}

// ── Web search (via DuckDuckGo lite) ─────────────────────────

pub(crate) async fn exec_web_search(_ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    if query.is_empty() {
        return "no query provided".to_string();
    }
    let encoded = urlencoding(query);
    let url = format!("https://lite.duckduckgo.com/lite/?q={}", encoded);
    match http_client().get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.text().await {
                Ok(html) => extract_duckduckgo_results(&html),
                Err(e) => format!("web search parse error: {e}"),
            }
        }
        Ok(resp) => format!("web search status: {}", resp.status()),
        Err(e) => format!("web search failed: {e}"),
    }
}

// ── Fetch URL (via reqwest) ──────────────────────────────────

pub(crate) async fn exec_fetch_url(_ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let url = v["url"].as_str().unwrap_or("");
    if url.is_empty() {
        return "no URL provided".to_string();
    }
    match http_client().get(url).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.text().await {
                Ok(body) => {
                    if body.len() > 10000 {
                        format!("{}... (truncated, {} total)", &body[..10000], body.len())
                    } else {
                        body
                    }
                }
                Err(e) => format!("fetch read error: {e}"),
            }
        }
        Ok(resp) => format!("fetch status: {} ({})", resp.status(), url),
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
