/// One-shot prompt to DeepSeek with streaming.
///
/// Receives prompt from args or stdin pipe.
/// Uses shared api.rs for API calls.

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
    let client = reqwest::Client::new();

    let narrow = terminal_width() <= 80;
    let tw = terminal_width();

    let messages = vec![serde_json::json!({"role": "user", "content": prompt})];

    if stream {
        match api::call_stream(&client, &base_url, &api_key, &model, &messages, narrow, tw).await {
            Ok((_reply, _usage)) => {}
            Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
        }
    } else {
        match api::call_nonstream(&client, &base_url, &api_key, &model, &messages).await {
            Ok((_reply, _usage)) => {}
            Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
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
