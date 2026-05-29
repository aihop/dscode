/// One-shot prompt to DeepSeek with streaming + agent tool support.
///
/// Receives prompt from args or stdin pipe.
/// Runs a full agent loop: model → tool_calls → execute → model → ...

use crate::api::{self, resolve_model_name, resolve_api_key, resolve_base_url};
use clap::Args;
use std::io::Read;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Prompt (omit to read from stdin pipe)
    pub prompt: Vec<String>,
    /// Model (default: v4-flash, or config value)
    #[arg(short = 'm', long)]
    pub model: Option<String>,
    /// Disable streaming
    #[arg(long)]
    pub no_stream: bool,
}

pub async fn run(args: &RunArgs) {
    let model = resolve_model_name(
        &args.model.clone().unwrap_or_else(|| api::default_model(true)),
    );
    let stream = !args.no_stream;

    // Resolve prompt: args first, then stdin pipe
    let prompt = if !args.prompt.is_empty() {
        args.prompt.join(" ")
    } else {
        let mut buf = String::new();
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        if handle.read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            buf.trim().to_string()
        } else {
            eprintln!("error: prompt required");
            eprintln!("  dscode run <prompt>");
            eprintln!("  echo 'hi' | dscode run");
            std::process::exit(1);
        }
    };

    let api_key = resolve_api_key().unwrap_or_else(|| {
        eprintln!("error: no DeepSeek API key found");
        eprintln!("  dscode auth login  or  export DEEPSEEK_API_KEY=sk-...");
        std::process::exit(1);
    });
    let base_url = resolve_base_url();
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .build().unwrap();

    let narrow = terminal_width() <= 80;
    let tw = terminal_width();

    // Build messages with optional system prompt from AGENT.md
    let tools_list = api::tool_definitions();
    let active_tools: Option<&[serde_json::Value]> = if tools_list.is_empty() { None } else { Some(&tools_list) };

    let mut api_msgs: Vec<serde_json::Value> = Vec::new();
    if let Some(ap) = api::load_agent_md() {
        api_msgs.push(serde_json::json!({"role": "system", "content": ap}));
    }
    api_msgs.push(serde_json::json!({"role": "user", "content": prompt}));

    // Agent loop: model → tool_calls → execute → model → ...
    let max_rounds = 10;
    for round in 1..=max_rounds {
        if stream {
            let result = api::call_stream(&client, &base_url, &api_key, &model, &api_msgs, active_tools, narrow, tw).await;
            match result {
                Ok(stream_res) => {
                    if !stream_res.tool_calls.is_empty() {
                        // Build assistant message with tool_calls
                        let mut assistant_msg = serde_json::json!({
                            "role": "assistant",
                            "content": stream_res.content,
                        });
                        if !stream_res.reasoning_content.is_empty() {
                            assistant_msg["reasoning_content"] = serde_json::Value::String(stream_res.reasoning_content.clone());
                        }
                        let tc_json: Vec<serde_json::Value> = stream_res.tool_calls.iter().map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": { "name": tc.name, "arguments": tc.arguments }
                            })
                        }).collect();
                        assistant_msg["tool_calls"] = serde_json::Value::Array(tc_json);
                        api_msgs.push(assistant_msg);

                        // Execute tools
                        for tc in &stream_res.tool_calls {
                            let mut result = api::execute_tool(tc).await;
                            if result.len() > 4000 {
                                result = format!("{}... (truncated, {} total)", &result[..4000], result.len());
                            }
                            api_msgs.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tc.id,
                                "content": result,
                            }));
                            if narrow {
                                eprintln!("─ tool: {}(..) → {} chars", tc.name, result.len());
                            }
                        }
                        if narrow { eprintln!("─ continuing..."); }
                    } else {
                        // Text response — already printed by call_stream
                        if narrow { eprintln!("─ {:.0} tok", stream_res.usage.tokens_out); }
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("\nerror: {e}");
                    break;
                }
            }
        } else {
            // Non-streaming: one shot (no tool loop for non-streaming)
            match api::call_nonstream(&client, &base_url, &api_key, &model, &api_msgs).await {
                Ok((_reply, usage)) => {
                    if usage.tokens_out > 0 {
                        eprintln!("─ {:.0} tok (reasoning: {:.0})", usage.tokens_out, usage.reasoning_tokens);
                    }
                    break;
                }
                Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
            }
        }
        if round == max_rounds {
            if narrow { eprintln!("─ max rounds reached"); }
        }
    }
}

fn terminal_width() -> u16 {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<u16>() { if w > 0 { return w; } }
    }
    if let Ok(o) = std::process::Command::new("stty")
        .args(["size"]).stdin(std::process::Stdio::inherit()).output()
    {
        if let Ok(s) = String::from_utf8(o.stdout) {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if parts.len() == 2 {
                if let Ok(w) = parts[1].parse::<u16>() { if w > 0 { return w; } }
            }
        }
    }
    80
}
