use clap::Args;
use std::io::{self, Write};

#[derive(Debug, Args)]
pub struct ChatArgs {
    /// Model to use (default: deepseek-v4-pro)
    #[arg(short = 'm', long)]
    pub model: Option<String>,

    /// Session ID to resume
    #[arg(short = 's', long)]
    pub session: Option<String>,

    /// Disable streaming output
    #[arg(long)]
    pub no_stream: bool,
}

pub async fn run(args: &ChatArgs) {
    let model = args
        .model
        .clone()
        .unwrap_or_else(|| "deepseek-v4-pro".to_string());
    let stream = !args.no_stream;

    // Resolve API key
    let api_key = resolve_api_key();
    if api_key.is_none() {
        eprintln!("error: no DeepSeek API key found");
        eprintln!("  Run `dscode auth login` or set DEEPSEEK_API_KEY");
        std::process::exit(1);
    }
    let api_key = api_key.unwrap();

    // Resolve base URL
    let base_url = resolve_base_url();

    let client = reqwest::Client::new();

    println!(" dscode chat · {model}");
    println!(" ──────────────────────────────────");
    println!(" (Ctrl+C to exit, Ctrl+L to clear)");
    println!();

    let mut messages: Vec<serde_json::Value> = Vec::new();

    loop {
        // Read user input
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("\nerror: {e}");
                break;
            }
        }

        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        // Handle special commands
        if input == "/exit" || input == "/quit" {
            break;
        }
        if input == "/clear" {
            messages.clear();
            print!("\x1B[2J\x1B[H"); // clear screen
            println!(" dscode chat · {model}");
            println!(" ──────────────────────────────────");
            continue;
        }
        if input == "/help" {
            println!("Commands:");
            println!("  /exit, /quit   exit chat");
            println!("  /clear         clear screen and history");
            println!("  /help          show this help");
            println!("  /model <name>  switch model");
            continue;
        }
        if input.starts_with("/model ") {
            let new_model = input.trim_start_matches("/model ").trim();
            if !new_model.is_empty() {
                println!("Switching to model: {new_model}");
                // We don't change model mid-stream for now
            }
            continue;
        }

        // Add user message
        messages.push(serde_json::json!({
            "role": "user",
            "content": input
        }));

        // Call DeepSeek API
        match call_deepseek(&client, &base_url, &api_key, &model, &messages, stream).await {
            Ok(assistant_content) => {
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": assistant_content
                }));
            }
            Err(e) => {
                eprintln!("\nerror: {e}");
                messages.pop(); // remove the user message that failed
            }
        }

        println!();
    }
}

fn resolve_api_key() -> Option<String> {
    // 1. Check environment first
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.trim().is_empty() {
            return Some(key);
        }
    }

    // 2. Check config file
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode");
    let config_path = config_dir.join("config.toml");

    if let Ok(store) = codewhale_config::ConfigStore::load_or_default(&config_path) {
        if let Some(key) = store.config.api_key {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
    }

    // 3. Check secrets store
    let secrets_path = config_dir.join("secrets.json");
    if let Ok(secrets) = codewhale_secrets::Secrets::new(&secrets_path) {
        if let Ok(Some(key)) = secrets.get("deepseek") {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
    }

    None
}

fn resolve_base_url() -> String {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode");
    let config_path = config_dir.join("config.toml");

    if let Ok(store) = codewhale_config::ConfigStore::load_or_default(&config_path) {
        if let Some(url) = store.config.providers.deepseek.base_url {
            return url;
        }
    }

    "https://api.deepseek.com/beta".to_string()
}

async fn call_deepseek(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
    stream: bool,
) -> Result<String, anyhow::Error> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": stream,
        "max_tokens": 8192,
    });

    if stream {
        call_deepseek_stream(client, &url, api_key, body).await
    } else {
        call_deepseek_nonstream(client, &url, api_key, body).await
    }
}

async fn call_deepseek_stream(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: serde_json::Value,
) -> Result<String, anyhow::Error> {
    use futures_util::StreamExt;

    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("API error {status}: {text}");
    }

    let mut full_content = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let chunk_str = String::from_utf8_lossy(&chunk);

        for line in chunk_str.lines() {
            let line = line.trim();
            if line.is_empty() || line == "data: [DONE]" {
                continue;
            }
            let data = if let Some(stripped) = line.strip_prefix("data: ") {
                stripped
            } else {
                continue;
            };

            match serde_json::from_str::<serde_json::Value>(data) {
                Ok(parsed) => {
                    if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                        full_content.push_str(delta);
                        print!("{delta}");
                        io::stdout().flush().unwrap();
                    }
                }
                Err(_) => {
                    // Skip non-JSON SSE lines
                }
            }
        }
    }

    println!(); // final newline
    Ok(full_content)
}

async fn call_deepseek_nonstream(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: serde_json::Value,
) -> Result<String, anyhow::Error> {
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("API error {status}: {text}");
    }

    let data: serde_json::Value = response.json().await?;
    let content = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(no response)")
        .to_string();

    println!("{content}");
    Ok(content)
}
