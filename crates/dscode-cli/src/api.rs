/// Shared DeepSeek API client layer.
///
/// Thin connector — CodeWhale engine doesn't expose a simple
/// "send → stream" API, so this bridges the gap. ~80 lines.
/// Once CodeWhale's crate lib exposes such an API, replace this.

use std::io::{self, Write};
use std::path::PathBuf;

/// Resolve DeepSeek model from short alias to full API name
pub fn resolve_model_name(input: &str) -> String {
    match input {
        "v4-pro" | "v4pro"       => "deepseek-v4-pro",
        "v4-flash" | "v4flash" | "flash" => "deepseek-v4-flash",
        "v3"                      => "deepseek-v3",
        "v3.2" | "v32"           => "deepseek-v3.2",
        "r1"                      => "deepseek-r1",
        "chat"                    => "deepseek-chat",
        "reasoner"                => "deepseek-reasoner",
        "coder"                   => "deepseek-coder",
        other                     => other,
    }
    .to_string()
}

pub fn default_model(flash: bool) -> String {
    // 1. Config file has priority
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(path)) {
        if let Some(m) = store.config.providers.deepseek.model {
            if !m.is_empty() {
                return m;
            }
        }
    }
    // 2. Fallback
    if flash { "deepseek-v4-flash" } else { "deepseek-v4-pro" }.to_string()
}

pub fn resolve_api_key() -> Option<String> {
    // 1. Env
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.trim().is_empty() {
            return Some(key);
        }
    }
    // 2. Config file
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(path)) {
        if let Some(key) = store.config.api_key {
            if !key.trim().is_empty() {
                return Some(key);
            }
        }
    }
    None
}

pub fn resolve_base_url() -> String {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(store) = codewhale_config::ConfigStore::load(Some(path)) {
        if let Some(url) = store.config.providers.deepseek.base_url {
            return url;
        }
    }
    "https://api.deepseek.com/beta".to_string()
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("dscode")
        .join("config.toml")
}

/// Usage info to display after API calls
pub struct UsageInfo {
    pub model: String,
    pub tokens_out: u64,
    pub reasoning_tokens: u64,
}

/// Call DeepSeek chat completions with streaming, return full response
pub async fn call_stream(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
    narrow: bool,
    terminal_width: u16,
) -> Result<(String, UsageInfo), String> {
    use futures_util::StreamExt;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "max_tokens": 8192,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API {status}: {text}"));
    }

    let mut full = String::new();
    let mut stream = response.bytes_stream();
    let mut col: u16 = 0;
    let max_col = terminal_width.saturating_sub(2);
    let mut usage = UsageInfo { model: model.to_string(), tokens_out: 0, reasoning_tokens: 0 };

    loop {
        let chunk = match stream.next().await {
            Some(Ok(c)) => c,
            Some(Err(_)) => break,  // Ctrl+C or conn drop
            None => break,
        };
        for line in String::from_utf8_lossy(&chunk).lines() {
            let data = match line.trim() {
                l if l.is_empty() || l == "data: [DONE]" => continue,
                l => match l.strip_prefix("data: ") {
                    Some(s) => s,
                    None => continue,
                },
            };
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                // Track reasoning tokens (R1 model)
                if let Some(rt) = parsed["choices"][0]["delta"]["reasoning_content"].as_str() {
                    usage.reasoning_tokens += rt.len() as u64 / 4;
                }
                if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                    full.push_str(delta);
                    usage.tokens_out += delta.len() as u64 / 4; // rough est
                    if narrow {
                        for ch in delta.chars() {
                            if ch == '\n' { col = 0; }
                            else {
                                col += 1;
                                if col >= max_col && ch.is_whitespace() {
                                    print!("\n");
                                    col = 0;
                                }
                            }
                        }
                    }
                    print!("{delta}");
                    io::stdout().flush().ok();
                }
            }
        }
    }
    println!();
    Ok((full, usage))
}

/// Call DeepSeek chat completions without streaming
pub async fn call_nonstream(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
) -> Result<(String, UsageInfo), String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "max_tokens": 8192,
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("connection failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("API {status}: {text}"));
    }

    let data: serde_json::Value = response.json().await.map_err(|e| format!("parse error: {e}"))?;

    let usage = UsageInfo {
        model: model.to_string(),
        tokens_out: data["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        reasoning_tokens: data["usage"]["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap_or(0),
    };

    let content = data["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(no response)")
        .to_string();

    println!("{content}");
    Ok((content, usage))
}
