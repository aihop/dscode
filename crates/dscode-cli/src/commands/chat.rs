/// Mobile-first interactive chat with DeepSeek.
///
/// Thin UX layer on top of CodeWhale engine + shared api.rs.
/// Session persistence via JSON, narrow-terminal aware.

use crate::api::{self, UsageInfo, resolve_model_name, resolve_api_key, resolve_base_url};
use chrono::Utc;
use clap::Args;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Args)]
pub struct ChatArgs {
    #[arg(short = 'm', long, help = "Model (v4-pro, flash, r1, or full name)")]
    pub model: Option<String>,
    #[arg(short = 's', long, help = "Resume session by ID (prefix OK)")]
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

pub async fn run(args: &ChatArgs) {
    let model = resolve_model_name(
        &args.model.clone().unwrap_or_else(|| api::default_model(false)),
    );
    let stream = !args.no_stream;

    let api_key = resolve_api_key().unwrap_or_else(|| {
        eprintln!("error: no DeepSeek API key found");
        eprintln!("  Set one with:  dscode auth login");
        eprintln!("  Or export:     export DEEPSEEK_API_KEY=sk-...");
        std::process::exit(1);
    });
    let base_url = resolve_base_url();
    let client = reqwest::Client::new();

    // Session resume
    let (_session_id, mut messages) = if let Some(sid) = &args.session {
        match load_session(sid) {
            Some(s) => {
                eprintln!("(resumed session {})", &s.id[..8]);
                (s.id, s.messages)
            }
            None => {
                eprintln!("session '{sid}' not found, starting new");
                (Uuid::new_v4().to_string(), Vec::new())
            }
        }
    } else {
        (Uuid::new_v4().to_string(), Vec::new())
    };

    let narrow = is_narrow_terminal();
    let tw = terminal_width();
    let max_rounds: usize = 20; // trim oldest when exceeded

    let initial_msgs = messages.len();
    if !narrow {
        println!("dscode · {model}  (Ctrl+C /help) [{} msgs]", initial_msgs);
        println!("{}", "─".repeat(std::cmp::min(usize::from(tw.saturating_sub(1)), 50)));
    }

    loop {
        let prompt = if narrow { "\n> " } else { "> " };
        print!("{prompt}");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => { println!(); break; }
            Err(e) => { eprintln!("\nerror: {e}"); break; }
        }

        let mut input = input.trim_end().to_string();
        if input.is_empty() { continue; }

        // Multi-line input: line ends with \  or starts with ```
        if input.ends_with('\\') {
            input.pop(); // remove trailing backslash
            loop {
                let sub = read_line_raw();
                let sub = sub.trim_end().to_string();
                if sub.is_empty() { break; }
                let should_continue = sub.ends_with('\\');
                let sub = if should_continue { sub[..sub.len()-1].to_string() } else { sub };
                input.push('\n');
                input.push_str(&sub);
                if !should_continue { break; }
            }
        } else if input.starts_with("```") {
            let fence = input.clone();
            loop {
                let sub = read_line_raw();
                let sub = sub.trim_end().to_string();
                input.push('\n');
                input.push_str(&sub);
                if sub == fence || sub.trim() == "```" { break; }
            }
        }

        // Built-in commands (match on trimmed single-line)
        let cmd = input.trim();
        match cmd {
            "/exit" | "/quit" => break,
            "/clear" => {
                messages.clear();
                print!("\x1B[2J\x1B[H");
                if !narrow {
                    println!("dscode · {model}");
                    println!("{}", "─".repeat(std::cmp::min(usize::from(tw.saturating_sub(1)), 50)));
                }
                continue;
            }
            "/help" => {
                println!("/exit  quit    /clear  clear screen");
                println!("/save  save now");
                continue;
            }
            "/save" => { save_session(&model, &messages); println!("saved"); continue; }
            _ => {}
        }

        let ts = Utc::now().timestamp();
        messages.push(Message { role: "user".into(), content: input, created_at: ts });

        // Context window: keep last N rounds
        if messages.len() > max_rounds * 2 {
            let trimmed = messages.len() - max_rounds * 2;
            messages.drain(0..trimmed);
            if narrow {
                eprintln!("─ trimmed {trimmed} old msgs to save context");
            }
        }

        let api_msgs: Vec<serde_json::Value> = messages.iter().map(|m| {
            serde_json::json!({"role": m.role, "content": m.content})
        }).collect();

        match api::call_stream(&client, &base_url, &api_key, &model, &api_msgs, narrow, tw).await {
            Ok((reply, usage)) => {
                messages.push(Message { role: "assistant".into(), content: reply, created_at: Utc::now().timestamp() });
                // Compact usage line on narrow terminals
                if narrow {
                    eprintln!("─ {:.1}s {:.0} tok", usage.tokens_out as f64 / 30.0, usage.tokens_out);
                }
                // Auto-save every 4 rounds
                if messages.len() % 8 == 0 { save_session(&model, &messages); }
            }
            Err(e) => {
                eprintln!("\nerror: {e}");
                messages.pop();
            }
        }
    }

    save_session(&model, &messages);
}

// ── Session persistence ──────────────────────────────────────────

fn session_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("~/.local/share")).join("dscode").join("sessions")
}

fn session_path(id: &str) -> PathBuf { session_dir().join(format!("{id}.json")) }

fn save_session(model: &str, messages: &[Message]) {
    let dir = session_dir();
    std::fs::create_dir_all(&dir).ok();
    let id = find_matching_session(messages).unwrap_or_else(|| Uuid::new_v4().to_string());
    let s = Session {
        id: id.clone(), model: model.to_string(),
        created_at: messages.first().map(|m| m.created_at).unwrap_or_else(|| Utc::now().timestamp()),
        updated_at: Utc::now().timestamp(), messages: messages.to_vec(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&s) {
        std::fs::write(session_path(&id), &json).ok();
    }
}

fn find_matching_session(messages: &[Message]) -> Option<String> {
    let dir = session_dir();
    if !dir.exists() { return None; }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(c) = std::fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str::<Session>(&c) {
                    if s.messages.len() == messages.len()
                        && s.messages.iter().zip(messages.iter()).all(|(a, b)| a.role == b.role && a.content == b.content)
                    { return Some(s.id); }
                }
            }
        }
    }
    None
}

fn load_session(id: &str) -> Option<Session> {
    let p = session_path(id);
    if p.exists() {
        return serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok();
    }
    // Prefix match
    let dir = session_dir();
    if dir.exists() {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "json")
                && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.starts_with(id))
            {
                return serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok();
            }
        }
    }
    None
}

// ── Terminal helpers ─────────────────────────────────────────────

fn terminal_width() -> u16 {
    if let Ok(cols) = std::env::var("COLUMNS") { if let Ok(w) = cols.parse::<u16>() { if w > 0 { return w; } } }
    if let Ok(o) = std::process::Command::new("stty").args(["size"]).stdin(std::process::Stdio::inherit()).output() {
        if let Ok(s) = String::from_utf8(o.stdout) {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if parts.len() == 2 { if let Ok(w) = parts[1].parse::<u16>() { if w > 0 { return w; } } }
        }
    }
    80
}

fn is_narrow_terminal() -> bool { terminal_width() <= 80 }

/// Read one line from stdin without trimming
fn read_line_raw() -> String {
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).ok();
    buf
}
