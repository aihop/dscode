/// One-shot prompt to DeepSeek with streaming + agent tool support.
///
/// Receives prompt from args or stdin pipe.
/// Runs a full agent loop: model → tool_calls → execute → model → ...

use crate::api::{self, resolve_model_name, resolve_api_key, resolve_base_url};
use crate::engine::{AgentEngine, AgentOptions};
use crate::utils::{is_narrow_terminal, terminal_width};
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
            std::process::exit(1);
        }
    };

    let api_key = resolve_api_key().unwrap_or_else(|| {
        eprintln!("error: no DeepSeek API key found");
        std::process::exit(1);
    });
    let base_url = resolve_base_url();
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .build().unwrap();

    let narrow = is_narrow_terminal();
    let tw = terminal_width();

    let tools_list = Some(api::tool_definitions());
    
    let mut sys_content = "You are dscode, a mobile-first AI coding agent. You are running in the project root. Always use relative paths.".to_string();
    if let Some(ap) = api::load_agent_md() {
        sys_content = format!("{}\n\n{}", sys_content, ap);
    }
    let mut api_msgs: Vec<serde_json::Value> = Vec::new();
    api_msgs.push(serde_json::json!({"role": "user", "content": prompt}));

    let engine = AgentEngine::new(client, base_url, api_key);
    let options = AgentOptions {
        model,
        system_prompt: sys_content,
        tools: tools_list,
        max_rounds: 10,
        narrow,
        silent: false,
        terminal_width: tw,
        cwd: std::env::current_dir().unwrap_or_default(),
    };

    if stream {
        match engine.run_loop(&options, api_msgs).await {
            Ok((_msgs, usage)) => {
                if narrow && usage.tokens_out > 0 {
                    eprintln!("\x1B[90m─ usage: {:.0} tok (reasoning: {:.0})\x1B[0m", usage.tokens_out, usage.reasoning_tokens);
                }
            }
            Err(e) => {
                eprintln!("\nerror: {e}");
            }
        }
    } else {
        // Simple non-stream fallback (reusing call_nonstream from api)
        // But engine doesn't have a non-stream loop yet. 
        // For now, keep it simple or just use the engine's loop (which handles streaming internally)
        match engine.run_loop(&options, api_msgs).await {
            Ok((_msgs, usage)) => {
                if usage.tokens_out > 0 {
                    eprintln!("─ {:.0} tok (reasoning: {:.0})", usage.tokens_out, usage.reasoning_tokens);
                }
            }
            Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
        }
    }
}
