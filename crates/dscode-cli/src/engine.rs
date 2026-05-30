use crate::api::{self, UsageInfo, MAX_TOOL_OUTPUT_CHARS};
use serde_json::{json, Value};
use std::io::{self, Write};

pub struct AgentOptions {
    pub model: String,
    pub system_prompt: String,
    pub tools: Option<Vec<Value>>,
    pub max_rounds: usize,
    pub narrow: bool,
    pub silent: bool,
    pub approval_mode: bool,
    pub terminal_width: u16,
    pub cwd: std::path::PathBuf,
    pub reasoning_effort: Option<String>,
}

pub struct AgentEngine {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AgentEngine {
    pub fn new(client: reqwest::Client, base_url: String, api_key: String) -> Self {
        Self { client, base_url, api_key }
    }

    /// Run a full agent loop: call model, execute tools, handle truncation, and repeat.
    /// Returns the updated list of messages and total usage.
    pub async fn run_loop(
        &self,
        options: &AgentOptions,
        history: Vec<Value>,
    ) -> Result<(Vec<Value>, UsageInfo), String> {
        // Split into two system messages for DeepSeek prompt caching:
        // First message is the fixed prompt (cacheable across conversations).
        // Second message is dynamic environment info (not cacheable).
        let env_content = format!(
            "## Environment\n\
             - Current Working Directory: {}\n\
             - Terminal: {}\n\
             - Language: Rust (edition 2021, MSRV 1.75)\n\
             - Tools Available: {} tools (files, git, shell, search, web, agent)\n\
             - Context Window: 1M tokens\n",
            options.cwd.display(),
            if options.narrow { "narrow/mobile" } else { "standard" },
            crate::tools::ALL_TOOL_NAMES.len(),
        );
        let memory = crate::tools::agent::load_memory();
        let mut api_msgs = vec![
            json!({"role": "system", "content": options.system_prompt}),
            json!({"role": "system", "content": format!("{}{}", env_content, memory)}),
        ];
        api_msgs.extend(history);
        let mut total_usage = UsageInfo::default();
        let mut round = 0;
        let mut max_rounds_reached = false;
 let turn_start = std::time::Instant::now();
        let mut current_tools = options.tools.clone();
        let mut expanded = false;
        let mut edit_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        loop {
            round += 1;
            if round > options.max_rounds {
                max_rounds_reached = true;
                break;
            }

              // Start on a fresh line (separate from user input previous status)
                if !options.silent { println!(); }

                  let mut stream_res = api::call_stream(
                    &self.client,
                    &self.base_url,
                &self.api_key,
                &options.model,
                &api_msgs,
                current_tools.as_deref(),
                options.narrow,
                options.silent,
                options.terminal_width,
                    options.reasoning_effort.as_deref(),
            ).await?;

            // Accumulate usage
            total_usage.tokens_out += stream_res.usage.tokens_out;
            total_usage.reasoning_tokens += stream_res.usage.reasoning_tokens;
            total_usage.prompt_tokens = stream_res.usage.prompt_tokens; // latest round
            total_usage.cache_hit_tokens = stream_res.usage.cache_hit_tokens;

            // Handle auto-continuation for truncated text
            let mut accumulated_content = stream_res.content.clone();
            let mut accumulated_reasoning = stream_res.reasoning_content.clone();

              while stream_res.finish_reason.as_deref() == Some("length") && stream_res.tool_calls.is_empty() {
                  if !options.silent {
                      eprintln!("\x1B[90m─ continuing...\x1B[0m");
                  }
                let mut partial_assistant = json!({
                    "role": "assistant",
                    "content": accumulated_content.clone(),
                });
                if !accumulated_reasoning.is_empty() {
                    partial_assistant["reasoning_content"] = Value::String(accumulated_reasoning.clone());
                }

                let mut temp_msgs = api_msgs.clone();
                temp_msgs.push(partial_assistant);

                let next_res = api::call_stream(
                    &self.client,
                    &self.base_url,
                    &self.api_key,
                    &options.model,
                    &temp_msgs,
                    current_tools.as_deref(),
                    options.narrow,
                    options.silent,
                    options.terminal_width,
                    options.reasoning_effort.as_deref(),
                ).await?;

                accumulated_content.push_str(&next_res.content);
                accumulated_reasoning.push_str(&next_res.reasoning_content);
                
                // Accumulate usage for continuation
                total_usage.tokens_out += next_res.usage.tokens_out;
                total_usage.reasoning_tokens += next_res.usage.reasoning_tokens;
                
                stream_res = next_res;
            }

            stream_res.content = accumulated_content;
            stream_res.reasoning_content = accumulated_reasoning;

            // Prepare assistant message for context
            let mut assistant_msg = json!({
                "role": "assistant",
                "content": stream_res.content,
            });
            if !stream_res.reasoning_content.is_empty() {
                assistant_msg["reasoning_content"] = Value::String(stream_res.reasoning_content.clone());
            }

            if !stream_res.tool_calls.is_empty() {
                // Add tool calls to assistant message
                let tc_json: Vec<Value> = stream_res.tool_calls.iter().map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": { "name": tc.name, "arguments": tc.arguments }
                    })
                }).collect();
                assistant_msg["tool_calls"] = Value::Array(tc_json);
                api_msgs.push(assistant_msg);

                    // Track file edits to detect infinite loops
                    let edit_tools = ["edit_file", "write_file", "fim_edit", "apply_patch"];
                    for tc in &stream_res.tool_calls {
                        if tc.name == "finish" || tc.name == "/done" {
                            // Model explicitly signals completion
                            if !options.silent { eprintln!("\x1B[90m─ finish\x1B[0m"); }
                            // Don't push anything, just break out of the outer loop
                            // We'll handle this by modifying the loop state
                            // Actually, let me use a different approach - set a flag
                        }
                        if edit_tools.contains(&tc.name.as_str()) {
                            if let Ok(v) = serde_json::from_str::<Value>(&tc.arguments) {
                                if let Some(p) = v["path"].as_str() {
                                    *edit_counts.entry(p.to_string()).or_insert(0) += 1;
                                }
                            }
                        }
                    }
                    // Check if finish was called
                    let has_finish = stream_res.tool_calls.iter().any(|tc| tc.name == "finish" || tc.name == "/done");
                    if has_finish {
                        // Model said it's done - break the loop
                        if !options.silent { eprintln!("\x1B[90m─ task complete\x1B[0m"); }
                        break;
                    }
                    // Check for repeated edits on the same file
                    for (path, count) in edit_counts.iter() {
                        if *count >= 4 {
                            api_msgs.push(json!({
                                "role": "system",
                                "content": format!("[NOTE] You have edited '{}' {} times. If you are stuck, call finish() to stop or use write_file with the full file content.", path, count)
                            }));
                            break;
                        }
                    }
                    // Execute tools (with optional approval gate)
                    for tc in &stream_res.tool_calls {
                        let skip = if options.approval_mode && needs_approval(&tc.name) {
                            confirm_tool(&tc.name, &tc.arguments)
                        } else {
                            false
                        };
                        let mut result = if skip {
                            format!("[skipped by user] tool {} was not executed", tc.name)
                        } else {
                            api::execute_tool(tc).await
                        };

                        // Auto-Verification Hook: after editing a file, try to run a linter/checker
                        if !skip && (tc.name == "edit_file" || tc.name == "write_file" || tc.name == "fim_edit" || tc.name == "apply_patch") && !result.starts_with("error:") {
                            let path = serde_json::from_str::<Value>(&tc.arguments).ok()
                                .and_then(|v| v["path"].as_str().map(|s| s.to_string()));
                            if let Some(p) = path {
                                let ext = std::path::Path::new(&p).extension().and_then(|e| e.to_str()).unwrap_or("");
                                let check_result = match ext {
                                    "rs" => {
                                        if !options.silent { eprintln!("\x1B[90m─ cargo check...\x1B[0m", ); }
                                        let output = std::process::Command::new("cargo")
                                            .args(["check", "--message-format=json"])
                                            .current_dir(&options.cwd)
                                            .output();
                                        if let Ok(output) = output {
                                            let stdout = String::from_utf8_lossy(&output.stdout);
                                            let mut errors = Vec::new();
                                            for line in stdout.lines() {
                                                if let Ok(msg) = serde_json::from_str::<Value>(line) {
                                                    if msg["reason"] == "compiler-message" {
                                                        let message = &msg["message"];
                                                        if message["level"] == "error" {
                                                            let rendered = message["rendered"].as_str().unwrap_or("");
                                                            if !rendered.is_empty() {
                                                                errors.push(rendered.to_string());
                                                            }
                                                        }
                                                    }
                                                }
                                                if errors.len() > 3 { break; }
                                            }
                                            if !errors.is_empty() {
                                                Some((false, errors.join("\n")))
                                            } else {
                                                Some((true, String::new()))
                                            }
                                        } else { None }
                                    }
                                    "py" => {
                                        if !options.silent { eprintln!("\x1B[90m─ python check...\x1B[0m"); }
                                        let full = if p.starts_with('/') { std::path::PathBuf::from(&p) } else { options.cwd.join(&p) };
                                        let output = std::process::Command::new("python3")
                                            .args(["-m", "py_compile", &full.to_string_lossy()])
                                            .output().or_else(|_| std::process::Command::new("python")
                                                .args(["-m", "py_compile", &full.to_string_lossy()]).output());
                                        run_simple_check(output)
                                    }
                                    "js" => {
                                        if !options.silent { eprintln!("\x1B[90m─ node check...\x1B[0m"); }
                                        let full = if p.starts_with('/') { std::path::PathBuf::from(&p) } else { options.cwd.join(&p) };
                                        run_simple_check(std::process::Command::new("node")
                                            .args(["--check", &full.to_string_lossy()]).output())
                                    }
                                    "ts" => {
                                        if !options.silent { eprintln!("\x1B[90m─ tsc check...\x1B[0m"); }
                                        let full = if p.starts_with('/') { std::path::PathBuf::from(&p) } else { options.cwd.join(&p) };
                                        run_simple_check(std::process::Command::new("npx")
                                            .args(["-p", "typescript", "tsc", "--noEmit", &full.to_string_lossy()]).output())
                                    }
                                    "go" => {
                                        if !options.silent { eprintln!("\x1B[90m─ go vet...\x1B[0m"); }
                                        let full = if p.starts_with('/') { std::path::PathBuf::from(&p) } else { options.cwd.join(&p) };
                                        run_simple_check(std::process::Command::new("go")
                                            .args(["vet", &full.to_string_lossy()]).output())
                                    }
                                    "c" | "h" => {
                                        if !options.silent { eprintln!("\x1B[90m─ gcc check...\x1B[0m"); }
                                        let full = if p.starts_with('/') { std::path::PathBuf::from(&p) } else { options.cwd.join(&p) };
                                        run_simple_check(std::process::Command::new("gcc")
                                            .args(["-fsyntax-only", &full.to_string_lossy()]).output())
                                    }
                                    "cpp" | "hpp" => {
                                        if !options.silent { eprintln!("\x1B[90m─ g++ check...\x1B[0m"); }
                                        let full = if p.starts_with('/') { std::path::PathBuf::from(&p) } else { options.cwd.join(&p) };
                                        run_simple_check(std::process::Command::new("g++")
                                            .args(["-fsyntax-only", &full.to_string_lossy()]).output())
                                    }
                                    _ => None,
                                };
                                if let Some((pass, msg)) = check_result {
                                    if pass {
                                        result.push_str("\n[VERIFY PASS] Syntax OK");
                                    } else {
                                        result.push_str(&format!("\n\n[VERIFY FAIL]\n{}\nTIP: Fix the errors above.", msg));
                                    }
                                }
                            }
                        }

                        if result.len() > MAX_TOOL_OUTPUT_CHARS {
                            result = format!("{}... (truncated, {} total)", &result[..MAX_TOOL_OUTPUT_CHARS], result.len());
                        }
                        api_msgs.push(json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": result,
                        }));
                        if !options.silent {
                            let line = format_tool_line(&tc.name, &result);
                            eprintln!("\x1B[90m{}\x1B[0m", line);
                        }
                    }                // Auto-expand: after first tool round, give model full tools
                if !expanded && options.tools.is_some() {
                    current_tools = Some(crate::tools::tool_definitions());
                    expanded = true;
                    if !options.silent && !options.narrow {
                        println!("\x1B[90m─ exp: {}→full tools\x1B[0m", crate::tools::CORE_TOOL_NAMES.len());
                    }
                  }
                  // Tools executed, loop continues to let model see results
                  // Loop continues to let model see tool results
              } else {                // No more tool calls, we're done with this turn
                api_msgs.push(assistant_msg);
                break;
            }
        }

          // Show final status at bottom
          {
              let elapsed = turn_start.elapsed();
              let total_tok = total_usage.tokens_out + total_usage.reasoning_tokens;
              let (symbol, desc) = if max_rounds_reached {
                  ("\x1B[33m⚠\x1B[0m", format!("用尽回合({}/{})", round - 1, options.max_rounds))
              } else {
                  ("\x1B[32m✓\x1B[0m", "完成".to_string())
              };
              if !options.silent {
                  if options.narrow {
                      eprintln!("\x1B[90m│{} \x1B[90m{:.0}s {:.0} tok\x1B[0m│", symbol, elapsed.as_secs_f64(), total_tok);
                  } else {
                      eprintln!("\x1B[90m── {} {} · {:.1}s · {} tok\x1B[90m ──\x1B[0m",
                          symbol, desc, elapsed.as_secs_f64(), total_tok);
                  }
              }
          }
        Ok((api_msgs, total_usage))
    }
}

/// Run a simple checker command and return (pass, error_message).
/// Used for non-Rust language checks (python, node, gcc, etc.)
fn run_simple_check(result: std::io::Result<std::process::Output>) -> Option<(bool, String)> {
    match result {
        Ok(output) => {
            if output.status.success() {
                Some((true, String::new()))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut msg = String::new();
                if !stderr.is_empty() { msg.push_str(&stderr); }
                if !stdout.is_empty() {
                    if !msg.is_empty() { msg.push('\n'); }
                    msg.push_str(&stdout);
                }
                if msg.len() > 2000 { msg = format!("{}... (truncated)", &msg[..2000]); }
                Some((false, msg))
            }
        }
        Err(_) => None,
    }
}

/// Tools that modify state — these need approval in approval_mode
fn needs_approval(name: &str) -> bool {
    matches!(name,
        "write_file" | "edit_file" | "fim_edit" | "run_shell"
        | "apply_patch"
        | "git_add" | "git_commit" | "git_push"
    )
}

/// Show a confirmation prompt on stderr and return true if user says "no" (skip).
fn confirm_tool(name: &str, args: &str) -> bool {
    // Parse a brief description from arguments
    let brief = if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        if let Some(p) = v["path"].as_str() {
 format!("{}({})", name, p)
        } else if let Some(cmd) = v["command"].as_str() {
            format!("{}({})", name, cmd.chars().take(60).collect::<String>())
        } else {
            format!("{}()", name)
        }
    } else {
        format!("{}()", name)
    };

    if crate::utils::is_narrow_terminal() {
        eprint!("\x1B[33m? {}\x1B[0m [Y/n] ", brief);
    } else {
        eprint!("\x1B[33m┌─ 确认: {} ───┐\n│ 确定执行？[Y/n] \x1B[0m", brief);
    }
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_lowercase();
    trimmed == "n" || trimmed == "no"
}

/// Generate a one-line summary of a tool execution for user display.
/// The full `result` is still sent to the model.
fn format_tool_line(name: &str, result: &str) -> String {
    match name {
        "read_file" => {
            // Parse: <file path="..." total_lines="N" start_line="L" end_line="R" ...>
            let path = result.lines().next()
                .and_then(|l| l.split_once("path=\""))
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(p, _)| p)
                .unwrap_or("");
            let total = result.lines().next()
                .and_then(|l| l.split_once("total_lines=\""))
                .and_then(|(_, rest)| rest.split_once('"'))
                .and_then(|(n, _)| n.parse::<usize>().ok())
                .unwrap_or(0);
            let start = result.lines().next()
                .and_then(|l| l.split_once("start_line=\""))
                .and_then(|(_, rest)| rest.split_once('"'))
                .and_then(|(n, _)| n.parse::<usize>().ok())
                .unwrap_or(0);
            let end = result.lines().next()
                .and_then(|l| l.split_once("end_line=\""))
                .and_then(|(_, rest)| rest.split_once('"'))
                .and_then(|(n, _)| n.parse::<usize>().ok())
                .unwrap_or(0);
            let short = path.rsplit('/').next().unwrap_or(path);
            if total > 0 {
                format!("➤ {}:{}:{}/{}", short, start, end, total)
            } else {
                format!("➤ {}", short)
            }
        }
        "write_file" => {
            // Format: "written /path/file (N bytes)" or "written /path/file (N bytes)\n<diff>"
            let first = result.lines().next().unwrap_or("");
            if let Some(rest) = first.strip_prefix("written ") {
                rest.to_string()
            } else {
                format!("✍  {}", result.chars().take(60).collect::<String>())
            }
        }
        "edit_file" => {
            // Format: "edited /path/file\n<diff>"
            let lines: Vec<&str> = result.lines().collect();
            let first = lines.first().unwrap_or(&"");
            let path = if let Some(rest) = first.strip_prefix("edited ") {
                rest.to_string()
            } else {
                format!("🔧  {}", result.chars().take(40).collect::<String>())
            };
            
            // Extract a mini-diff (first few added/removed lines)
            let mut diff_summary = Vec::new();
            for line in lines.iter().skip(1).take(10) {
                if line.starts_with('+') || line.starts_with('-') {
                    diff_summary.push(*line);
                }
                if diff_summary.len() >= 3 { break; }
            }
            if diff_summary.is_empty() {
                path
            } else {
                format!("{} ({})", path, diff_summary.join(" ").chars().take(40).collect::<String>())
            }
        }
        "run_shell" => {
            let first = result.lines().next().unwrap_or("");
            let preview = first.chars().take(40).collect::<String>();
            format!("> {}", preview)
        }
        "search_code" | "web_search" | "file_search" => {
            let lines = result.lines().count();
            let first = result.lines().next().unwrap_or("");
            let preview = first.chars().take(60).collect::<String>();
            format!("🔍 {} ({} lines)", preview, lines)
        }
        "git_status" | "git_diff" | "git_log" | "git_show" | "git_blame" => {
            let lines = result.lines().count();
            let first = result.lines().next().unwrap_or("");
            let preview = first.chars().take(60).collect::<String>();
            format!("{} {} ({} lines)", name, preview, lines)
        }
        "git_add" | "git_commit" | "git_push" => {
            result.chars().take(80).collect::<String>()
        }
        "list_files" => {
            let first = result.lines().next().unwrap_or("");
            let preview = first.chars().take(60).collect::<String>();
            format!("📂 {} ({} total)", preview, result.lines().count().saturating_sub(1))
        }
        "list_tree" => {
            let lines = result.lines().count();
            format!("📂 {} entries", lines)
        }
        "get_file_info" => {
            let chars = result.len();
            format!("📄 {} chars", chars)
        }
        "fetch_url" => {
            let chars = result.len();
            format!("🌐 {} chars", chars)
        }
        "test_runner" => {
            let lines = result.lines().count();
            let pass = result.lines().filter(|l| l.contains("ok") || l.contains("test result")).count();
            format!("🧪 {} tests ({} lines)", if pass > 0 { format!("{} pass", pass) } else { "?".to_string() }, lines)
        }
        "fim_edit" => {
            result.chars().take(80).collect::<String>()
        }
        "review" => {
            let lines = result.lines().count();
            format!("✅ review ({} lines)", lines)
        }
        "apply_patch" => {
            result.chars().take(60).collect::<String>()
        }
        _ => {
            // fallback: show first line of result
            let first = result.lines().next().unwrap_or("");
            format!("{} {}", name, first.chars().take(60).collect::<String>())
        }
    }
}
