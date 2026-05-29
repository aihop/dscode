/// Tool definitions and execution using CodeWhale's tool framework.
///
/// Core module: dispatcher, specs, registry, public API, and security policy.
/// Sub-modules: file, git, search, agent.

pub mod file;
pub mod git;
pub mod search;
pub mod agent;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codewhale_execpolicy::{
    ExecPolicyEngine, Ruleset, RulesetLayer,
};
use codewhale_protocol::{ToolKind, ToolOutput, ToolPayload};
use codewhale_tools::{
    FunctionCallError, ToolCallSource, ToolHandler, ToolInvocation, ToolRegistry, ToolSpec,
};
use serde_json::{Value, json};

// ── Context for tool handlers ──────────────────────────────────

#[derive(Clone)]
pub(super) struct ToolCtx {
    pub(super) cwd: PathBuf,
}

/// Join a user-provided path (relative or absolute) against the tool working directory.
pub(super) fn cwd_join(ctx: &ToolCtx, path_str: &str) -> PathBuf {
    if path_str.starts_with('/') {
        PathBuf::from(path_str)
    } else {
        ctx.cwd.join(path_str)
    }
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
            // File tools
            "read_file"        => file::exec_read_file(&self.ctx, &args),
            "write_file"       => file::exec_write_file(&self.ctx, &args),
            "edit_file"        => file::exec_edit_file(&self.ctx, &args),
            "list_files"       => file::exec_list_files(&self.ctx, &args),
            "list_tree"        => file::exec_list_tree(&self.ctx, &args),
            "get_file_info"    => file::exec_get_file_info(&self.ctx, &args),
            "apply_patch"      => file::exec_apply_patch(&self.ctx, &args),

            // Git tools
            "git_log"          => git::exec_git_log(&self.ctx, &args),
            "git_show"         => git::exec_git_show(&self.ctx, &args),
            "git_blame"        => git::exec_git_blame(&self.ctx, &args),
            "git_status"       => git::exec_git_status(&self.ctx, &args),
            "git_diff"         => git::exec_git_diff(&self.ctx, &args),
            "git_add"          => git::exec_git_add(&self.ctx, &args),
            "git_commit"       => git::exec_git_commit(&self.ctx, &args),
            "git_push"         => git::exec_git_push(&self.ctx, &args),

            // Search tools
            "search_code"      => search::exec_search_code(&self.ctx, &args),
            "file_search"      => search::exec_file_search(&self.ctx, &args),
            "web_search"       => search::exec_web_search(&self.ctx, &args),
            "fetch_url"        => search::exec_fetch_url(&self.ctx, &args),

            // Agent & System tools
            "review"           => agent::exec_review(&self.ctx, &args).await,
            "fim_edit"         => agent::exec_fim_edit(&self.ctx, &args).await,
            "agent_open"       => agent::exec_agent_open(&self.ctx, &args).await,
            "agent_eval"       => agent::exec_agent_eval(&args).await,
            "agent_close"      => agent::exec_agent_close(&args).await,
            "checklist_write"  => agent::exec_checklist_write(&args),
            "checklist_add"    => agent::exec_checklist_add(&args),
            "checklist_update" => agent::exec_checklist_update(&args),
            "checklist_list"   => agent::exec_checklist_list(),
            "test_runner"      => agent::exec_test_runner(&self.ctx, &args).await,
            "request_user_input" => agent::exec_request_user_input(&args).await,
            "run_shell"        => agent::exec_run_shell(&self.ctx, &args),

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
                    "new": {"type": "string", "description": "Replacement text"},
                    "line": {"type": "integer", "description": "Optional 1-based line hint"}
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
            name: "list_tree".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path (default .)"},
                    "depth": {"type": "integer", "description": "Max depth (default 2)"}
                }
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: None,
        },
        ToolSpec {
            name: "get_file_info".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
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
            name: "file_search".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Filename or path fragment (fuzzy)"},
                    "path": {"type": "string", "description": "Optional subdirectory (default .)"},
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
                    "patch": {"type": "string", "description": "Unified diff/patch content"}
                },
                "required": ["patch"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: false,
            timeout_ms: Some(30_000),
        },
        ToolSpec {
            name: "git_log".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "max_count": {"type": "integer", "description": "Max commits (default 20)"},
                    "path": {"type": "string", "description": "Optional file/subdirectory to scope"}
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
                    "rev": {"type": "string", "description": "Revision (commit SHA, branch, tag)"}
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
                    "path": {"type": "string", "description": "File path"},
                    "start_line": {"type": "integer", "description": "First line (1-based, default 1)"},
                    "max_lines": {"type": "integer", "description": "Max lines (default 200)"}
                },
                "required": ["path"]
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(15_000),
        },
        ToolSpec {
            name: "git_status".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Optional subdirectory"}
                }
            }),
            output_schema: json!({}),
            supports_parallel_tool_calls: true,
            timeout_ms: Some(10_000),
        },
        ToolSpec {
            name: "git_diff".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Optional file/subdirectory"},
                    "cached": {"type": "boolean", "description": "Show staged changes only"}
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
                    "path": {"type": "string", "description": "File path or glob to stage"}
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
                    "branch": {"type": "string", "description": "Branch name (default: current)"}
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
                    "target": {"type": "string", "description": "File, 'diff', or 'staged'"},
                    "kind": {"type": "string", "description": "Optional: file, diff, staged"}
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
                    "prefix_anchor": {"type": "string", "description": "End of prefix (kept)"},
                    "suffix_anchor": {"type": "string", "description": "Start of suffix (kept)"},
                    "max_tokens": {"type": "integer", "description": "Max tokens (default 512)"}
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
                    "prompt": {"type": "string", "description": "Task for the sub-agent"},
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
            name: "test_runner".into(),
            input_schema: json!({
                "type": "object", "properties": {
                    "command": {"type": "string", "description": "Test command (default: cargo test)"}
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
            input_schema: json!({"type": "object", "properties": {}}), output_schema: json!({}),
            supports_parallel_tool_calls: true, timeout_ms: Some(5_000),
        },
    ]
}

// ── Registry (lazy, thread-safe) ──────────────────────────────

pub(super) use std::sync::OnceLock;

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
    let specs: Vec<codewhale_tools::ConfiguredToolSpec> = registry.list_specs();

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

/// Human-readable descriptions for each tool.
fn tool_description(name: &str) -> &'static str {
    match name {
        "read_file"   => "Read the contents of a file. Path relative to project root.",
        "write_file"  => "Create or overwrite a file with content. Creates parent dirs if needed.",
        "edit_file"   => "Replace text in an existing file by searching for old text and replacing it.",
        "get_file_info" => "Get file metadata (size, mtime, line count) and a tiny preview without reading full content.",
        "list_tree"   => "List directory structure in a tree format with a depth limit.",
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
        "git_status"  => "Show working tree status (modified, staged, untracked files).",
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
        "request_user_input" => "Ask the user 1-3 short questions and return their selections.",
        _             => "Run a tool by name",
    }
}

/// Execute a tool call from the API response.
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

// ── Command safety policy ─────────────────────────────────────

pub(crate) fn policy_engine() -> &'static ExecPolicyEngine {
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
            "touch", "cp", "mv", "cat ", "less ", "more ",
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
