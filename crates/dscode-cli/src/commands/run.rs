use clap::Args;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Prompt to send
    pub prompt: Vec<String>,

    /// Model to use
    #[arg(short = 'm', long)]
    pub model: Option<String>,
}

fn resolve_model(input: &str) -> String {
    match input {
        "v4-pro" | "v4pro"           => "deepseek-v4-pro",
        "v4-flash" | "v4flash" | "flash" => "deepseek-v4-flash",
        "v3"                          => "deepseek-v3",
        "v3.2" | "v32"               => "deepseek-v3.2",
        "r1"                          => "deepseek-r1",
        "chat"                        => "deepseek-chat",
        "reasoner"                    => "deepseek-reasoner",
        "coder"                       => "deepseek-coder",
        other                         => other,
    }.to_string()
}

pub async fn run(args: &RunArgs) {
    let prompt = args.prompt.join(" ");
    if prompt.is_empty() {
        eprintln!("error: prompt is required");
        eprintln!("  Usage: dscode run <prompt>");
        std::process::exit(1);
    }

    let model = resolve_model(
        &args.model.clone().unwrap_or_else(|| "deepseek-v4-flash".to_string())
    );

    // Resolve API key
    let api_key = resolve_api_key();
    if api_key.is_none() {
        eprintln!("error: no DeepSeek API key found");
        eprintln!("  Run `dscode auth login` or set DEEPSEEK_API_KEY");
        std::process::exit(1);
    }
    let api_key = api_key.unwrap();
    let base_url = resolve_base_url();

    let client = reqwest::Client::new();

    let messages = vec![serde_json::json!({
        "role": "user",
        "content": prompt
    })];

    match call_deepseek(&client, &base_url, &api_key, &model, &messages, true).await {
        Ok(_content) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode")
        .join("config.toml")
}

fn resolve_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.trim().is_empty() {
            return Some(key);
        }
    }
    if let Some(parent) = config_path().parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(config_path())) {
        if let Some(key) = store.config.api_key {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
    }
    None
}

fn resolve_base_url() -> String {
    if let Some(parent) = config_path().parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(config_path())) {
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
    use futures_util::StreamExt;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": stream,
        "max_tokens": 8192,
    });

    let response = client
        .post(&url)
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
        for line in String::from_utf8_lossy(&chunk).lines() {
            let line = line.trim();
            if line.is_empty() || line == "data: [DONE]" {
                continue;
            }
            let data = match line.strip_prefix("data: ") {
                Some(s) => s,
                None => continue,
            };
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                    full_content.push_str(delta);
                    print!("{delta}");
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            }
        }
    }

    println!();
    Ok(full_content)
}
