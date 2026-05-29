use crate::api::{self, UsageInfo, MAX_TOOL_OUTPUT_CHARS};
use serde_json::{json, Value};

pub struct AgentOptions {
    pub model: String,
    pub system_prompt: String,
    pub tools: Option<Vec<Value>>,
    pub max_rounds: usize,
    pub narrow: bool,
    pub silent: bool,
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
            let turn_start = std::time::Instant::now();          // Start with core tools, auto-expand to full after first tool round
          let mut current_tools = options.tools.clone();
          let mut expanded = false;
        loop {
            round += 1;
            if round > options.max_rounds {
                eprintln!("\x1B[33m─ max rounds reached ({})\x1B[0m", options.max_rounds);
                    max_rounds_reached = true;
                    break;
                }

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
                if options.narrow && !options.silent {
                    eprintln!("\x1B[90m─ continuing truncated response...\x1B[0m");
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

                // Execute tools
                for tc in &stream_res.tool_calls {
                    let mut result = api::execute_tool(tc).await;
                    if result.len() > MAX_TOOL_OUTPUT_CHARS {
                        result = format!("{}... (truncated, {} total)", &result[..MAX_TOOL_OUTPUT_CHARS], result.len());
                    }
                    api_msgs.push(json!({
                        "role": "tool",
                        "tool_call_id": tc.id,
                        "content": result,
                    }));
                    if !options.silent {
                        if options.narrow {
                            eprintln!("\x1B[90m─ tool: {}(..) → {} chars\x1B[0m", tc.name, result.len());
                        } else {
                            // On desktop/non-narrow, show a bit more context
                            println!("\x1B[90m─ tool: {} ───────────────────\x1B[0m", tc.name);
                            if result.lines().count() > 15 {
                                // Show first few lines and last few lines if long
                                let lines: Vec<&str> = result.lines().collect();
                                for l in lines.iter().take(5) { println!("\x1B[90m  {}\x1B[0m", l); }
                                println!("\x1B[90m  ... ({} lines total) ...\x1B[0m", lines.len());
                                for l in lines.iter().rev().take(5).collect::<Vec<_>>().into_iter().rev() { println!("\x1B[90m  {}\x1B[0m", l); }
                            } else {
                                for l in result.lines() { println!("\x1B[90m  {}\x1B[0m", l); }
                            }
                            println!("\x1B[90m──────────────────────────────────────\x1B[0m");
                      }
                      }
                  }
                  // Auto-expand: after first tool round, give model full tools
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

            // Show completion footer with timing and token usage
            if !options.silent {
                let elapsed = turn_start.elapsed();
                let total_tok = total_usage.tokens_out + total_usage.reasoning_tokens;
                if max_rounds_reached {
                    if options.narrow {
                        eprintln!("\x1B[90m│\x1B[33m⚠\x1B[0m \x1B[90m{:.0}s {:.0} tok\x1B[0m│", elapsed.as_secs_f64(), total_tok);
                    } else {
                        eprintln!("\x1B[90m── \x1B[33m用尽回合({}/{})\x1B[0m · {:.1}s · {} tok\x1B[90m ──\x1B[m",
                            round - 1, options.max_rounds, elapsed.as_secs_f64(), total_tok);
                    }
                } else {
                    if options.narrow {
                        eprintln!("\x1B[90m│\x1B[32m✓\x1B[0m \x1B[90m{:.0}s {:.0} tok\x1B[0m│", elapsed.as_secs_f64(), total_tok);
                    } else {
                        eprintln!("\x1B[90m── \x1B[32m完成\x1B[0m · {:.1}s · {} tok\x1B[90m ──\x1B[0m",
                            elapsed.as_secs_f64(), total_tok);
                    }
                }
            }
            Ok((api_msgs, total_usage))
        }
}