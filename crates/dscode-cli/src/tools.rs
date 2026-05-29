/// Tool definitions and execution using CodeWhale's tool framework.
///
/// Wraps codewhale-tools' ToolRegistry + ToolHandler with the same
/// sync-friendly interface that chat.rs expects: `tool_definitions()`
/// for API schema and `execute_tool()` for running a tool call.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codewhale_protocol::{ToolKind, ToolOutput, ToolPayload};
use codewhale_tools::{
    ConfiguredToolSpec, FunctionCallError, ToolCallSource, ToolHandler, ToolInvocation, ToolRegistry, ToolSpec,
};
use serde_json::{Value, json};

// ── Context for tool handlers ──────────────────────────────────

#[derive(Clone)]
struct ToolCtx {
    cwd: PathBuf,
}

// ── Tool handler: dispatches by name ───────────────────────────

struct DscHandler {
    ctx: ToolCtx,
}

#[async_trait]
impl ToolHandler for DscHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolOutput, FunctionCallError> {
        let args = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments.clone(),
            _ => {
                return Ok(ToolOutput::Function {
                    body: Some(json!("unsupported payload type")),
                    success: false,
                });
            }
        };

        let result = match invocation.tool_name.as_str() {
            "read_file" => exec_read_file(&self.ctx, &args),
            "write_file" => exec_write_file(&self.ctx, &args),
            "edit_file" => exec_edit_file(&self.ctx, &args),
            "run_shell" => exec_run_shell(&self.ctx, &args),
            "search_code" => exec_search_code(&self.ctx, &args),
            "list_files" => exec_list_files(&self.ctx, &args),
            "web_search" => exec_web_search(&self.ctx, &args),
            "fetch_url" => exec_fetch_url(&self.ctx, &args),
            _ => format!("unknown tool: {}", invocation.tool_name),
        };

        Ok(ToolOutput::Function {
            body: Some(Value::String(result)),
            success: true,
        })
    }
}

// ── Tool specs ────────────────────────────────────────────────

fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "read_file".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path relative to project root"}
                },
                "required": ["path"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: None,
        },
        ToolSpec {
            name: "write_file".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path relative to project root"},
                    "content": {"type": "string", "description": "Full file content"}
                },
                "required": ["path", "content"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: None,
        },
        ToolSpec {
            name: "edit_file".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path relative to project root"},
                    "old": {"type": "string", "description": "Existing text to find (exact match)"},
                    "new": {"type": "string", "description": "Replacement text"}
                },
                "required": ["path", "old", "new"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: None,
        },
        ToolSpec {
            name: "run_shell".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Shell command to run"}
                },
                "required": ["command"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(30_000),
        },
        ToolSpec {
            name: "search_code".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Search pattern (regex)"},
                    "path": {"type": "string", "description": "Optional subdirectory to search"}
                },
                "required": ["pattern"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: None,
        },
        ToolSpec {
            name: "list_files".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path relative to project root"}
                },
                "required": ["path"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: None,
        },
        ToolSpec {
            name: "web_search".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"}
                },
                "required": ["query"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "fetch_url".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "HTTP/HTTPS URL to fetch"}
                },
                "required": ["url"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
    ]
}

// ── Registry (lazy, thread-safe) ──────────────────────────────

use std::sync::OnceLock;

fn global_registry() -> &'static ToolRegistry {
    static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut registry = ToolRegistry::default();
        let ctx = ToolCtx { cwd };

        for spec in tool_specs() {
            let name = spec.name.clone();
            if let Err(e) = registry.register(
                spec,
                Arc::new(DscHandler { ctx: ctx.clone() }),
            ) {
                tracing::warn!("failed to register tool '{name}': {e}");
            }
        }
        registry
    })
}

// ── Public API ─────────────────────────────────────────────────

/// Returns tool definitions in the DeepSeek function-calling format.
pub fn tool_definitions() -> Vec<Value> {
    let registry = global_registry();
    let specs: Vec<ConfiguredToolSpec> = registry.list_specs();

    // Sort by name for deterministic order
    let mut specs = specs;
    specs.sort_by(|a, b| a.spec.name.cmp(&b.spec.name));

    specs
        .into_iter()
        .map(|cfg| {
            json!({
                "type": "function",
                "function": {
                    "name": cfg.spec.name,
                    "description": tool_description(&cfg.spec.name),
                    "parameters": cfg.spec.input_schema,
                }
            })
        })
        .collect()
}

/// Human-readable descriptions for each tool (DeepSeek API requires string).
fn tool_description(name: &str) -> &'static str {
    match name {
        "read_file"   => "Read the contents of a file. Path relative to project root.",
        "write_file"  => "Create or overwrite a file with content. Creates parent dirs if needed.",
        "edit_file"   => "Replace text in an existing file by searching for old text and replacing it.",
        "run_shell"   => "Execute a shell command in the project root directory. Blocks destructive commands.",
        "search_code" => "Search for a regex pattern in project files (grep). Returns matches with file names.",
        "list_files"  => "List files and directories in a given path.",
        "web_search"  => "Search the web using DuckDuckGo and return results.",
        "fetch_url"   => "Fetch a URL via HTTP GET and return its content (max 10s timeout).",
        _             => "Run a tool by name",
    }
}

/// Execute a tool call from the API response.
/// Async but called with block_on from synchronous contexts.
pub async fn execute_tool(
    tc: &super::api::ToolCall,
) -> String {
    let registry = global_registry();

    let cw_call = codewhale_tools::ToolCall {
        name: tc.name.clone(),
        payload: ToolPayload::Function {
            arguments: tc.arguments.clone(),
        },
        source: ToolCallSource::Direct,
        raw_tool_call_id: Some(tc.id.clone()),
    };

    match registry.dispatch(cw_call, true).await {
        Ok(output) => match output {
            ToolOutput::Function { body, success: _ } => match body {
                Some(Value::String(s)) => s,
                Some(other) => other.to_string(),
                None => "(empty)".to_string(),
            },
            ToolOutput::Mcp { result } => result.to_string(),
        },
        Err(e) => format!("tool error: {:?}", e),
    }
}

// ── Tool implementations (ported from api.rs) ──────────────────

fn cwd_join(ctx: &ToolCtx, path_str: &str) -> PathBuf {
    if path_str.starts_with('/') {
        PathBuf::from(path_str)
    } else {
        ctx.cwd.join(path_str)
    }
}

fn exec_read_file(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let max_lines = 500;
            if lines.len() > max_lines {
                let head: Vec<&str> = lines[..max_lines].to_vec();
                format!(
                    "{} (showing first {max_lines} of {} lines)\n{}",
                    full_path.display(),
                    lines.len(),
                    head.join("\n")
                )
            } else {
                format!("{} ({} lines)\n{}", full_path.display(), lines.len(), content)
            }
        }
        Err(e) => format!("error reading {}: {e}", full_path.display()),
    }
}

fn exec_write_file(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    let content = v["content"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    match std::fs::write(&full_path, content) {
        Ok(_) => format!("written {} ({} bytes)", full_path.display(), content.len()),
        Err(e) => format!("error writing {}: {e}", full_path.display()),
    }
}

fn exec_edit_file(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    let old = v["old"].as_str().unwrap_or("");
    let new = v["new"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
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

fn exec_run_shell(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let cmd_str = v["command"].as_str().unwrap_or("");
    if cmd_str.is_empty() {
        return "error: no command".to_string();
    }
    // Safety: block destructive commands
    let lower = cmd_str.to_lowercase();
    let blocked = [
        "rm -rf /",
        "rm -rf /*",
        "dd if=",
        "mkfs.",
        "format ",
        ":(){ :|:& };:",
    ];
    if blocked.iter().any(|b| lower.contains(b)) {
        return "blocked: destructive command not allowed".to_string();
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
            if out.len() > 10000 {
                out = format!("{}... (truncated, {} total)", &out[..10000], out.len());
            }
            if !output.status.success() {
                out = format!(
                    "exit code {}: {}",
                    output.status.code().unwrap_or(-1),
                    out
                );
            }
            out
        }
        Err(e) => format!("exec error: {e}"),
    }
}

fn exec_search_code(ctx: &ToolCtx, args: &str) -> String {
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
            "-rn",
            "--include=*.rs",
            "--include=*.toml",
            "--include=*.md",
            "--include=*.html",
            "--include=*.sh",
            "--include=*.yml",
            "--include=*.json",
            "--include=*.css",
            "--include=*.js",
            "--include=*.ts",
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

fn exec_list_files(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    match std::fs::read_dir(&full_path) {
        Ok(entries) => {
            let mut items: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ty = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        "dir"
                    } else {
                        "file"
                    };
                    format!("  {ty:4}  {name}")
                })
                .collect();
            items.sort();
            format!(
                "{} ({} entries):\n{}",
                full_path.display(),
                items.len(),
                items.join("\n")
            )
        }
        Err(e) => format!("error listing {}: {e}", full_path.display()),
    }
}

fn exec_web_search(_ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    if query.is_empty() {
        return "no query provided".to_string();
    }
    // Use curl + duckduckgo-lite as a lightweight fallback
    match std::process::Command::new("curl")
        .args([
            "-s",
            "-L",
            "-o",
            "-",
            "--max-time",
            "10",
            &format!(
                "https://lite.duckduckgo.com/lite/?q={}",
                urlencoding(query)
            ),
        ])
        .output()
    {
        Ok(output) if output.status.success() => {
            let html = String::from_utf8_lossy(&output.stdout);
            extract_duckduckgo_results(&html)
        }
        _ => {
            // fallback: try the API
            match std::process::Command::new("curl")
                .args([
                    "-s",
                    "-L",
                    "-o",
                    "-",
                    "--max-time",
                    "10",
                    &format!(
                        "https://api.duckduckgo.com/?q={}&format=json",
                        urlencoding(query)
                    ),
                ])
                .output()
            {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout)
                        .to_string()
                }
                _ => format!("web search unavailable (install curl)"),
            }
        }
    }
}

fn exec_fetch_url(_ctx: &ToolCtx, args: &str) -> String {
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
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            format!("fetch failed (exit {}): {err}", output.status.code().unwrap_or(-1))
        }
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
    // Simple extraction from DuckDuckGo lite HTML
    for line in html.lines() {
        let line = line.trim();
        // Look for result links
        if line.starts_with("<a") && line.contains("class=\"result-link\"") {
            if let Some(start) = line.find("href=\"") {
                let rest = &line[start + 6..];
                if let Some(end) = rest.find('"') {
                    let url = &rest[..end];
                    results.push(format!("  {url}"));
                }
            }
        }
        // Look for result snippets
        if line.starts_with("<td") && results.len() > 0 {
            let text = line
                .replace("<td>", "")
                .replace("</td>", "")
                .replace("<b>", "")
                .replace("</b>", "")
                .replace("&amp;", "&")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'")
                .trim()
                .to_string();
            if !text.is_empty() && results.len() > 0 {
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
