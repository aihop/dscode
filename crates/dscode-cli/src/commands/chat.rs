/// Mobile-first interactive chat with DeepSeek.
///
/// Architecture: thin UX layer on top of CodeWhale engine.
/// - Config/state: codewhale-config, codewhale-state
/// - API calls: lightweight direct HTTP (CodeWhale doesn't export a simple send API)
/// - Persistence: JSON session files (lightweight, no SQLite dependency for basic chat)

use chrono::Utc;
use clap::Args;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use uuid::Uuid;

/// Detect terminal width. Returns COLUMNS env var, or falls back to `stty size`, or 80.
fn terminal_width() -> u16 {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<u16>() {
            if w > 0 { return w; }
        }
    }
    // Fallback: try running `stty size`
    if let Ok(output) = std::process::Command::new("stty")
        .args(["size"])
        .stdin(std::process::Stdio::inherit())
        .output()
    {
        if let Ok(s) = String::from_utf8(output.stdout) {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if parts.len() == 2 {
                if let Ok(w) = parts[1].parse::<u16>() {
                    if w > 0 { return w; }
                }
            }
        }
    }
    80
}

fn is_narrow_terminal() -> bool {
    terminal_width() <= 80
}

#[derive(Debug, Args)]
pub struct ChatArgs {
    #[arg(short = 'm', long, help = "Model (default: deepseek-v4-pro)")]
    pub model: Option<String>,
    #[arg(short = 's', long, help = "Resume session by ID")]
    pub session: Option<String>,
    #[arg(long, help = "Disable streaming output")]
    pub no_stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Session {
    id: String,
    model: String,
    created_at: i64,
    updated_at: i64,
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
    created_at: i64,
}

/// Resolve DeepSeek model from short alias to full API name
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

pub async fn run(args: &ChatArgs) {
    let model = resolve_model(
        &args.model.clone().unwrap_or_else(|| "deepseek-v4-pro".to_string())
    );
    let stream = !args.no_stream;

    let api_key = resolve_api_key().unwrap_or_else(|| {
        eprintln!("error: no DeepSeek API key found");
        eprintln!("  Run `dscode auth login` or set DEEPSEEK_API_KEY");
        std::process::exit(1);
    });
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();

    // Session management
    let (_session_id, mut messages) = if let Some(sid) = &args.session {
        match load_session(sid) {
            Some(s) => {
                eprintln!("(resumed session {})", &s.id[..8]);
                (s.id, s.messages)
            }
            None => {
                eprintln!("Session '{sid}' not found, starting new");
                new_session(&model)
            }
        }
    } else {
        new_session(&model)
    };

    // Print minimal header — no decorations on narrow terminals
    let narrow = is_narrow_terminal();
    if !narrow {
        println!("dscode · {model}  (/help)");
        println!("{}", "─".repeat(std::cmp::min(usize::from(terminal_width().saturating_sub(1)), 50)));
    }

    loop {
        let prompt = if is_narrow_terminal() { "\n> " } else { "> " };
        print!("{prompt}");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break,
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

        match input.as_str() {
            "/exit" | "/quit" => break,
            "/clear" => {
                messages.clear();
                print!("\x1B[2J\x1B[H");
                if !is_narrow_terminal() {
                    println!("dscode · {model}");
                    println!("{}", "─".repeat(std::cmp::min(usize::from(terminal_width().saturating_sub(1)), 50)));
                }
                continue;
            }
            "/help" => {
                println!("Commands:");
                println!("  /exit, /quit   exit chat");
                println!("  /clear         clear screen and history");
                println!("  /help          show this help");
                println!("  /save          force-save session now");
                continue;
            }
            "/save" => {
                save_session(&model, &messages);
                println!("✓ session saved");
                continue;
            }
            _ => {}
        }

        messages.push(Message {
            role: "user".into(),
            content: input.clone(),
            created_at: Utc::now().timestamp(),
        });

        match call_deepseek(&client, &base_url, &api_key, &model, &messages, stream).await {
            Ok(reply) => {
                messages.push(Message {
                    role: "assistant".into(),
                    content: reply,
                    created_at: Utc::now().timestamp(),
                });
                // Auto-save every 5 message pairs
                if messages.len() % 10 == 0 {
                    save_session(&model, &messages);
                }
            }
            Err(e) => {
                eprintln!("\nerror: {e}");
                messages.pop();
            }
        }
        println!();
    }

    save_session(&model, &messages);
}

// ── Session persistence ──────────────────────────────────────────

fn session_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("dscode")
        .join("sessions")
}

fn new_session(_model: &str) -> (String, Vec<Message>) {
    let id = Uuid::new_v4().to_string();
    (id, Vec::new())
}

fn session_path(id: &str) -> PathBuf {
    session_dir().join(format!("{id}.json"))
}

fn save_session(model: &str, messages: &[Message]) {
    let dir = session_dir();
    std::fs::create_dir_all(&dir).ok();

    // Find existing session file by reading all files looking for a matching first message
    let id = find_session_for_messages(messages).unwrap_or_else(|| Uuid::new_v4().to_string());

    let session = Session {
        id: id.clone(),
        model: model.to_string(),
        created_at: messages.first().map(|m| m.created_at).unwrap_or_else(|| Utc::now().timestamp()),
        updated_at: Utc::now().timestamp(),
        messages: messages.to_vec(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&session) {
        std::fs::write(session_path(&id), &json).ok();
    }
}

fn find_session_for_messages(messages: &[Message]) -> Option<String> {
    let dir = session_dir();
    if !dir.exists() {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(session) = serde_json::from_str::<Session>(&content) {
                    if session.messages.len() == messages.len()
                        && session.messages.iter().zip(messages.iter()).all(|(a, b)| {
                            a.role == b.role && a.content == b.content
                        })
                    {
                        return Some(session.id);
                    }
                }
            }
        }
    }
    None
}

fn load_session(id: &str) -> Option<Session> {
    let path = session_path(id);
    if !path.exists() {
        // Try to find by prefix
        let dir = session_dir();
        if dir.exists() {
            for entry in std::fs::read_dir(dir).ok()? {
                let p = entry.ok()?.path();
                if p.extension().is_some_and(|e| e == "json") {
                    if p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.starts_with(id))
                    {
                        let content = std::fs::read_to_string(&p).ok()?;
                        return serde_json::from_str(&content).ok();
                    }
                }
            }
        }
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

// ── Config helpers ──────────────────────────────────────────────

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
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

// ── DeepSeek API calls ──────────────────────────────────────────
// Lightweight layer — CodeWhale doesn't expose a simple send/receive API.
// This is the connector, not the wheel.

async fn call_deepseek(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    messages: &[Message],
    stream: bool,
) -> Result<String, anyhow::Error> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let api_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content
            })
        })
        .collect();

    let body = serde_json::json!({
        "model": model,
        "messages": api_messages,
        "stream": stream,
        "max_tokens": 8192,
    });

    if stream {
        call_stream(client, &url, api_key, body).await
    } else {
        call_nonstream(client, &url, api_key, body).await
    }
}

async fn call_stream(
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

    let narrow = is_narrow_terminal();
    let mut full = String::new();
    let mut stream = response.bytes_stream();
    let mut col: u16 = 0;
    let max_col = terminal_width().saturating_sub(2);

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        for line in String::from_utf8_lossy(&chunk).lines() {
            let data = match line.trim() {
                l if l.is_empty() || l == "data: [DONE]" => continue,
                l => match l.strip_prefix("data: ") {
                    Some(s) => s,
                    None => continue,
                },
            };
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                    full.push_str(delta);
                    if narrow {
                        for ch in delta.chars() {
                            if ch == '\n' {
                                col = 0;
                            } else {
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
    Ok(full)
}

async fn call_nonstream(
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
