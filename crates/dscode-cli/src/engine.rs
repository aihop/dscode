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
        let mut api_msgs = vec![
            json!({"role": "system", "content": options.system_prompt}),
            json!({"role": "system", "content": format!(
                "## Environment\n- Current Working Directory: {}\n- Terminal: {}\n",
                options.cwd.display(),
                if options.narrow { "narrow/mobile" } else { "standard" }
            )}),
        ];
        api_msgs.extend(history);
        let mut total_usage = UsageInfo::default();
        let mut round = 0;
        let mut max_rounds_reached = false;
 let turn_start = std::time::Instant::now();
        let mut current_tools = options.tools.clone();
        let mut expanded = false;

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
                // Loop continues to let model see tool results
            } else {
                // No more tool calls, we're done with this turn
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
            // Format: "edited /path/file" or "edited /path/file\n<diff>"
            let first = result.lines().next().unwrap_or("");
            if let Some(rest) = first.strip_prefix("edited ") {
                rest.to_string()
            } else {
                format!("🔧  {}", result.chars().take(60).collect::<String>())
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
