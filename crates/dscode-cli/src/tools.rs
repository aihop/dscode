/// Tool definitions and execution using CodeWhale's tool framework.
///
/// Wraps codewhale-tools' ToolRegistry + ToolHandler with the same
/// sync-friendly interface that chat.rs expects: `tool_definitions()`
/// for API schema and `execute_tool()` for running a tool call.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codewhale_execpolicy::{
    AskForApproval, ExecPolicyContext, ExecPolicyEngine, Ruleset, RulesetLayer,
};
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
            "git_log" => exec_git_log(&self.ctx, &args),
            "git_show" => exec_git_show(&self.ctx, &args),
            "git_blame" => exec_git_blame(&self.ctx, &args),
            "file_search" => exec_file_search(&self.ctx, &args),
            "apply_patch" => exec_apply_patch(&self.ctx, &args),
            "git_diff" => exec_git_diff(&self.ctx, &args),
            "git_add" => exec_git_add(&self.ctx, &args),
            "git_commit" => exec_git_commit(&self.ctx, &args),
            "git_push" => exec_git_push(&self.ctx, &args),
            "review" => exec_review(&self.ctx, &args).await,
            "fim_edit" => exec_fim_edit(&self.ctx, &args).await,
            "agent_open" => exec_agent_open(&self.ctx, &args).await,
            "agent_eval" => exec_agent_eval(&args).await,
            "agent_close" => exec_agent_close(&args).await,
            "checklist_write" => exec_checklist_write(&args),
            "checklist_add" => exec_checklist_add(&args),
            "checklist_update" => exec_checklist_update(&args),
            "checklist_list" => exec_checklist_list(),
            "test_runner" => exec_test_runner(&self.ctx, &args).await,
            "request_user_input" => exec_user_input(&args).await,
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
                    "path": {"type": "string", "description": "File path relative to project root"},
                    "start_line": {"type": "integer", "description": "First line to read (1-based, default 1)"},
                    "max_lines": {"type": "integer", "description": "Max lines to return (default 500)"}
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
        ToolSpec {
            name: "git_log".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "max_count": {"type": "integer", "description": "Max commits to show (default 20)"},
                    "path": {"type": "string", "description": "Optional file/subdirectory to scope history"}
                }
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "git_show".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "rev": {"type": "string", "description": "Revision to show (commit SHA, branch, tag)"}
                },
                "required": ["rev"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "git_blame".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path relative to project root"},
                    "start_line": {"type": "integer", "description": "First line (1-based, default 1)"},
                    "max_lines": {"type": "integer", "description": "Max lines to show (default 200)"}
                },
                "required": ["path"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "file_search".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Filename or path fragment to search for (fuzzy)"},
                    "path": {"type": "string", "description": "Optional subdirectory to search (default .)"},
                    "limit": {"type": "integer", "description": "Max results (default 20)"}
                },
                "required": ["query"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "apply_patch".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "patch": {"type": "string", "description": "Unified diff/patch content to apply"}
                },
                "required": ["patch"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(30_000),
        },
        ToolSpec {
            name: "git_diff".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Optional file/subdirectory to scope diff"},
                    "cached": {"type": "boolean", "description": "Show staged changes only (default false)"}
                }
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "git_add".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path or glob pattern to stage (default .)"}
                },
                "required": ["path"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "git_commit".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "Commit message"}
                },
                "required": ["message"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "git_push".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "remote": {"type": "string", "description": "Remote name (default: origin)"},
                    "branch": {"type": "string", "description": "Branch name (default: current branch)"}
                }
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(30_000),
        },
        ToolSpec {
            name: "review".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "File path, 'diff' for working tree, or 'staged' for staged changes"},
                    "kind": {"type": "string", "description": "Optional: 'file', 'diff', or 'staged' (auto-detected from target)"}
                },
                "required": ["target"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(60_000),
        },
        ToolSpec {
            name: "fim_edit".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to edit"},
                    "prefix_anchor": {"type": "string", "description": "Text marking the end of the prefix (kept as-is)"},
                    "suffix_anchor": {"type": "string", "description": "Text marking the start of the suffix (kept as-is)"},
                    "max_tokens": {"type": "integer", "description": "Max tokens for generated middle (default 512)"}
                },
                "required": ["path", "prefix_anchor", "suffix_anchor"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(30_000),
        },
        ToolSpec {
            name: "agent_open".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "Task description for the sub-agent"},
                    "name": {"type": "string", "description": "Optional session name"}
                },
                "required": ["prompt"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: true, timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "agent_eval".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {"type": "string", "description": "Agent id from agent_open"}
                },
                "required": ["agent_id"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: true, timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "agent_close".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": {"type": "string", "description": "Agent id from agent_open"}
                },
                "required": ["agent_id"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: false, timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "checklist_write".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "todos": {"type": "array", "items": {"type": "object", "properties": {
                        "content": {"type": "string"}, "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                    }, "required": ["content", "status"]}}
                },
                "required": ["todos"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: false, timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "checklist_add".into(),
            input_schema: json!({
                "type": "object", "properties": {
                    "content": {"type": "string"}, "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                }, "required": ["content"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: false, timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "checklist_update".into(),
            input_schema: json!({
                "type": "object", "properties": {
                    "id": {"type": "integer"}, "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                }, "required": ["id", "status"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: false, timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "checklist_list".into(),
            input_schema: json!({}), output_schema: json!({}),
            supports_parallel_tool_calls: true, timeout_ms: Some(5_000),
        },
        ToolSpec {
            name: "test_runner".into(),
            input_schema: json!({
                "type": "object", "properties": {
                    "command": {"type": "string", "description": "Test command to run (default: cargo test)"}
                }
            }),
            output_schema: json!({}), supports_parallel_tool_calls: false, timeout_ms: Some(120_000),
        },
        ToolSpec {
            name: "request_user_input".into(),
            input_schema: json!({
                "type": "object", "properties": {
                    "questions": {"type": "array", "items": {"type": "object", "properties": {
                        "header": {"type": "string"},
                        "id": {"type": "string"},
                        "question": {"type": "string"},
                        "options": {"type": "array", "items": {"type": "object", "properties": {
                            "label": {"type": "string"}, "description": {"type": "string"}
                        }, "required": ["label", "description"]}}
                    }, "required": ["header", "id", "question"]}}
                }, "required": ["questions"]
            }),
            output_schema: json!({}), supports_parallel_tool_calls: false, timeout_ms: Some(300_000),
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
        "git_log"     => "Show commit log history. Optional path and max_count to scope results.",
        "git_show"    => "Show details of a specific commit/revision: diff, metadata, and message.",
        "git_blame"   => "Show who last modified each line of a file. Optional line range.",
        "file_search" => "Search for files by name (fuzzy match). Returns matching file paths.",
        "apply_patch" => "Apply a unified-diff patch to the working tree (via git apply).",
        "git_diff"    => "Show working tree diff (unstaged or staged changes). Optional path scope.",
        "git_add"     => "Stage file(s) for commit. Path can be a file or glob pattern.",
        "git_commit"  => "Create a commit with a message. Requires prior git_add.",
        "git_push"    => "Push commits to remote repository.",
        "review"      => "Review code for issues, bugs, and improvements. Target a file, 'diff', or 'staged'.",
        "fim_edit"    => "Fill-in-the-Middle edit: replace content between two anchors via DeepSeek FIM API.",
        "agent_open"  => "Spawn a sub-agent to work on a task in the background. Returns an agent_id.",
        "agent_eval"  => "Check status and get results from a sub-agent by agent_id.",
        "agent_close" => "Close a sub-agent and return its final result.",
        "checklist_write" => "Create or replace a task checklist with items and statuses.",
        "checklist_add"   => "Add one item to the checklist.",
        "checklist_update" => "Update an item's status by id (pending/in_progress/completed).",
        "checklist_list"  => "List all checklist items with their current status.",
        "test_runner" => "Run tests (default: cargo test) and return structured results.",
        "request_user_input" => "Ask the user 1-3 short questions and return their selections. Use when you need clarification.",
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

// ── Command safety policy (codewhale-execpolicy) ───────────────

fn policy_engine() -> &'static ExecPolicyEngine {
    static ENGINE: OnceLock<ExecPolicyEngine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let denied = vec![
            "rm -rf /", "rm -rf /*", "dd if=", "mkfs.", "format ",
            ":(){ :|:& };:", "reboot", "shutdown", "poweroff", "halt",
            "init 0", "init 6", "chmod 777 /", "chown", "mv /*",
            "wget http://", "curl http://", "> /dev/", "< /dev/",
            "sudo ", "passwd",
        ];
        let trusted = vec![
            "ls", "cat", "head", "tail", "echo", "pwd", "which",
            "date", "whoami", "id", "uname", "env", "printenv",
            "git status", "git log", "git diff", "git show", "git branch",
            "git rev-parse", "git blame", "cargo check", "cargo build",
            "cargo test", "cargo run", "cargo fmt", "cargo clippy",
            "npm test", "npm run", "python", "python3", "node", "rustc",
            "grep", "find", "sort", "wc", "du -sh", "df -h", "mkdir",
            "touch", "cp", "mv", "rm", "cat ", "less ", "more ",
        ];
        ExecPolicyEngine::with_rulesets(vec![
            Ruleset {
                layer: RulesetLayer::BuiltinDefault,
                trusted_prefixes: trusted.iter().map(|s| s.to_string()).collect(),
                denied_prefixes: denied.iter().map(|s| s.to_string()).collect(),
            },
        ])
    })
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
            let start_line = v["start_line"].as_u64().unwrap_or(1).max(1) as usize - 1;
            let max_lines = v["max_lines"].as_u64().unwrap_or(500).min(500) as usize;
            let end_line = (start_line + max_lines).min(lines.len());
            let total = lines.len();
            let truncated = end_line < total;
            let shown: Vec<&str> = lines[start_line..end_line].to_vec();
            let mut out = format!("<file path=\"{}\" total_lines=\"{}\" start_line=\"{}\" end_line=\"{}\"",
                full_path.display(), total, start_line + 1, end_line);
            if truncated { out.push_str(" truncated=\"true\""); }
            out.push_str(">\n");
            for (i, line) in shown.iter().enumerate() {
                out.push_str(&format!("{:>6}  {}\n", start_line + i + 1, line));
            }
            if truncated { out.push_str(&format!("... ({} more lines)\n", total - end_line)); }
            out.push_str(&format!("</file>"));
            out
        }
        Err(e) => format!("<error reading {}: {e}>", full_path.display()),
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
            match content.find(old) {
                None => return format!("error: exact match not found in {}", full_path.display()),
                Some(pos) => {
                    // Reject ambiguous edits: old text must appear exactly once
                    if content[pos + old.len()..].find(old).is_some() {
                        return format!("error: '{}' appears multiple times — include more context lines to make the edit unambiguous", old);
                    }
                }
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

    // Safety check via codewhale-execpolicy
    let engine = policy_engine();
    let decision = engine.check(ExecPolicyContext {
        command: cmd_str,
        cwd: &ctx.cwd.to_string_lossy(),
        ask_for_approval: AskForApproval::UnlessTrusted,
        sandbox_mode: None,
    });
    match decision {
        Ok(d) if !d.allow => return format!("blocked: {}", d.reason()),
        Ok(_) => {} // allowed
        Err(e) => return format!("policy error: {e}"),
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

// ── Git tools ─────────────────────────────────────────────────

fn exec_git_log(ctx: &ToolCtx, args: &str) -> String {
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

fn exec_git_show(ctx: &ToolCtx, args: &str) -> String {
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

fn exec_git_blame(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    if path.is_empty() { return "error: no path provided".to_string(); }
    let full_path = cwd_join(ctx, path);
    let start = v["start_line"].as_u64().unwrap_or(1).max(1);
    let max_lines = v["max_lines"].as_u64().unwrap_or(200).min(1000);
    match std::process::Command::new("git")
        .args(["blame", &format!("-L{start},{end}", end = start + max_lines - 1), "--", &full_path.to_string_lossy()])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => format!("git blame failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git blame error: {e}"),
    }
}

// ── File search (fuzzy filename) ─────────────────────────────

fn exec_file_search(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let query = v["query"].as_str().unwrap_or("");
    let search_path = v["path"].as_str().unwrap_or(".");
    let limit = v["limit"].as_u64().unwrap_or(20).min(100) as usize;
    if query.is_empty() { return "no query provided".to_string(); }

    let root = cwd_join(ctx, search_path);
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        let entries = match std::fs::read_dir(&dir) { Ok(e) => e, Err(_) => continue };
        for entry in entries.flatten() {
            if results.len() >= limit { break; }
            let path = entry.path();
            if path.file_name().map_or(false, |n| n.to_string_lossy().to_lowercase().contains(&query_lower)) {
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
        if results.len() >= limit { break; }
    }

    if results.is_empty() { format!("no files matching '{query}'") }
    else { format!("{} files matching '{query}':\n{}", results.len(), results.join("\n")) }
}

// ── Apply patch ──────────────────────────────────────────────

fn exec_apply_patch(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let patch = v["patch"].as_str().unwrap_or("");
    if patch.is_empty() { return "error: no patch provided".to_string(); }
    // Write patch to a temp file and apply via git apply
    let tmp = std::env::temp_dir().join(format!("dscode-patch-{}.diff", std::process::id()));
    let write_ok = std::fs::write(&tmp, patch).is_ok();
    if !write_ok { return "error: could not write patch file".to_string(); }
    let result = std::process::Command::new("git")
        .args(["apply", &tmp.to_string_lossy()])
        .current_dir(&ctx.cwd)
        .output();
    let _ = std::fs::remove_file(&tmp);
    match result {
        Ok(o) if o.status.success() => "patch applied successfully".to_string(),
        Ok(o) => format!("patch failed:\n{}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git apply error: {e}"),
    }
}

// ── Git write tools ───────────────────────────────────────────

fn exec_git_diff(ctx: &ToolCtx, args: &str) -> String {
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
            if out.is_empty() { "no changes".to_string() }
            else { out }
        }
        Ok(o) => format!("git diff failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git diff error: {e}"),
    }
}

fn exec_git_add(ctx: &ToolCtx, args: &str) -> String {
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

fn exec_git_commit(ctx: &ToolCtx, args: &str) -> String {
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

fn exec_git_push(ctx: &ToolCtx, args: &str) -> String {
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
            if out.is_empty() { "pushed successfully".to_string() }
            else { out }
        }
        Ok(o) => format!("push failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("push error: {e}"),
    }
}

// ── Review tool (calls DeepSeek API for structured code review) ─

use crate::api::{resolve_api_key, resolve_base_url};

async fn exec_review(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let target = v["target"].as_str().unwrap_or("");
    if target.is_empty() { return "error: no target provided".to_string(); }

    // Read target content
    let code = if target == "diff" {
        let o = std::process::Command::new("git").args(["diff"]).current_dir(&ctx.cwd).output();
        match o { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(), _ => return "no diff".to_string() }
    } else if target == "staged" {
        let o = std::process::Command::new("git").args(["diff", "--cached"]).current_dir(&ctx.cwd).output();
        match o { Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(), _ => return "no staged changes".to_string() }
    } else {
        let full = if target.starts_with('/') { std::path::PathBuf::from(target) } else { ctx.cwd.join(target) };
        match std::fs::read_to_string(&full) { Ok(c) => c, Err(e) => return format!("error reading {target}: {e}") }
    };
    if code.is_empty() { return "nothing to review".to_string(); }
    let code = if code.len() > 32_000 { format!("{}... (truncated)", &code[..32_000]) } else { code };

    // Call DeepSeek API for review
    let Some(api_key) = resolve_api_key() else { return "error: no API key".to_string() };
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": "deepseek-v4-flash",
        "messages": [
            {"role": "system", "content": "You are a senior code reviewer. Return a concise review with: summary, issues (severity/title/description), and suggestions. Be direct and actionable."},
            {"role": "user", "content": format!("Review this code:\n```\n{}```", code)}
        ],
        "max_tokens": 2048,
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

// ── FIM edit tool (calls DeepSeek /beta/completions) ───────────

async fn exec_fim_edit(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    let prefix_anchor = v["prefix_anchor"].as_str().unwrap_or("");
    let suffix_anchor = v["suffix_anchor"].as_str().unwrap_or("");
    let max_tokens = v["max_tokens"].as_u64().unwrap_or(512).min(2048);
    if path.is_empty() || prefix_anchor.is_empty() || suffix_anchor.is_empty() {
        return "error: path, prefix_anchor, and suffix_anchor are required".to_string();
    }

    let full_path = if path.starts_with('/') { std::path::PathBuf::from(path) } else { ctx.cwd.join(path) };
    let content = match std::fs::read_to_string(&full_path) { Ok(c) => c, Err(e) => return format!("error reading {path}: {e}") };

    // Find anchors
    let pa_start = match content.find(prefix_anchor) { Some(p) => p, None => return format!("prefix_anchor not found in {path}") };
    let sa_start = match content[pa_start + 1..].find(suffix_anchor) { Some(p) => pa_start + 1 + p, None => return format!("suffix_anchor not found after prefix_anchor in {path}") };
    if sa_start <= pa_start + prefix_anchor.len() {
        return "error: suffix_anchor overlaps with prefix_anchor".to_string();
    }

    let prompt = &content[..pa_start + prefix_anchor.len()];
    let suffix = &content[sa_start..];

    // Call DeepSeek FIM API
    let Some(api_key) = resolve_api_key() else { return "error: no API key".to_string() };
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();
    let url = format!("{}/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
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
            // Reassemble: prefix (up to anchor end) + generated + suffix (from anchor start)
            let new_content = format!("{}{}{}", &content[..pa_start + prefix_anchor.len()], generated, &content[sa_start..]);
            match std::fs::write(&full_path, &new_content) {
                Ok(_) => format!("fim_edit applied to {} ({} chars generated)", path, generated.len()),
                Err(e) => format!("error writing {path}: {e}"),
            }
        }
        Err(e) => format!("FIM API error: {e}"),
    }
}

// ── Sub-agent system (with fork_context support) ────────────────

use std::collections::HashMap;
use std::sync::Mutex;
use chrono::Utc;
use uuid::Uuid;

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

async fn run_sub_agent(api_key: &str, base_url: &str, _cwd: &std::path::Path, prompt: &str, context_msgs: &[Value]) -> String {
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
            "model": "deepseek-v4-flash",
            "messages": api_msgs,
            "max_tokens": 4096,
            "stream": false,
        });
        let tools = crate::api::tool_definitions();
        if !tools.is_empty() { body["tools"] = Value::Array(tools); }

        let resp = match client.post(&url).header("Authorization", format!("Bearer {api_key}")).json(&body).send().await {
            Ok(r) => r, Err(e) => return format!("error: {e}"),
        };
        let data: Value = match resp.json().await { Ok(d) => d, Err(_) => return "parse error".to_string() };
        let msg = &data["choices"][0]["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let tool_calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();

        if tool_calls.is_empty() { return if content.is_empty() { "(empty)".to_string() } else { content }; }

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
            if result.len() > 4000 { result = format!("{}... (truncated)", &result[..4000]); }
            api_msgs.push(json!({"role": "tool", "tool_call_id": tool_call.id, "content": result}));
        }
    }
    "max rounds reached".to_string()
}

async fn exec_agent_open(ctx: &ToolCtx, args: &str) -> String {
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
    let ctx_msgs: Vec<Value> = Vec::new(); // fork_context from parent not available at tool level

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

async fn exec_agent_eval(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let id = v["agent_id"].as_str().unwrap_or("");
    let agents = global_agents().lock().unwrap();
    match agents.get(id) {
        Some(s) => json!({"status": s.status, "result": s.result, "created_at": s.created_at}).to_string(),
        None => "not found".to_string(),
    }
}

async fn exec_agent_close(args: &str) -> String {
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

fn exec_checklist_write(args: &str) -> String {
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

fn exec_checklist_add(args: &str) -> String {
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

fn exec_checklist_update(args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let id = v["id"].as_u64().unwrap_or(u64::MAX) as usize;
    let status = v["status"].as_str().unwrap_or("");
    if status.is_empty() { return "error: no status".to_string(); }
    let mut list = global_checklist().lock().unwrap();
    for item in &mut list.1 {
        if item.id == id { item.status = status.to_string(); return format!("item {id} → {status}"); }
    }
    format!("item {id} not found")
}

fn exec_checklist_list() -> String {
    let list = global_checklist().lock().unwrap();
    if list.1.is_empty() { return "checklist is empty".to_string(); }
    let lines: Vec<String> = list.1.iter().map(|i| format!("  {} [{}] {}", i.id, i.status, i.content)).collect();
    format!("Checklist ({} items):\n{}", list.1.len(), lines.join("\n"))
}

// ── Test runner ───────────────────────────────────────────────

async fn exec_test_runner(ctx: &ToolCtx, args: &str) -> String {
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
            if !o.status.success() { result.push_str("exit code: "); result.push_str(&o.status.code().unwrap_or(-1).to_string()); result.push('\n'); }
            if !stdout.is_empty() { result.push_str(&stdout); }
            if !stderr.is_empty() { if !result.is_empty() { result.push('\n'); } result.push_str(&stderr); }
            if result.len() > 16000 { result = format!("{}... (truncated, {} total)", &result[..16000], result.len()); }
            result
        }
        _ => format!("failed to run '{cmd}'"),
    }
}

// ── User input tool ────────────────────────────────────────────

async fn exec_user_input(_args: &str) -> String {
    // Simplified: print question to stderr, read from stdin
    eprintln!("\n\x1B[33m[Agent needs input]\x1B[0m Type your response and press Enter:");
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => input.trim().to_string(),
        Err(_) => "input cancelled".to_string(),
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
