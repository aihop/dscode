use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum SessionCommands {
    /// List all sessions
    List,
    /// Show session details
    Show { id: String },
    /// Rename a session
    Rename { id: String, name: String },
    /// Delete a session
    #[command(alias = "rm")]
    Delete { id: String },
    /// Export a session as JSON
    Export { id: String },
}

pub async fn run(cmd: &SessionCommands) {
    match cmd {
        SessionCommands::List => list(),
        SessionCommands::Show { id } => show(id),
        SessionCommands::Rename { id, name } => rename(id, name),
        SessionCommands::Delete { id } => delete(id),
        SessionCommands::Export { id } => export(id),
    }
}

fn sessions_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"))
        .join("dscode")
        .join("sessions")
}

fn list() {
    let dir = sessions_dir();
    if !dir.exists() {
        println!("No sessions found.");
        return;
    }

    let mut sessions: Vec<(String, String, i64, usize)> = Vec::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    let id = path.file_stem().unwrap().to_string_lossy().to_string();
                    let created = data["created_at"].as_i64().unwrap_or(0);
                    let updated = data["updated_at"].as_i64().unwrap_or(created);
                    let model = data["model"].as_str().unwrap_or("?");
                    let msg_count = data["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                    // Preview: first user message
                    let preview = data["messages"]
                        .as_array()
                        .and_then(|msgs| {
                            msgs.iter().find(|m| m["role"] == "user")
                        })
                        .and_then(|m| m["content"].as_str())
                        .map(|s| if s.len() > 50 {
                            format!("{}...", &s[..47])
                        } else {
                            s.to_string()
                        })
                        .unwrap_or_else(|| "(empty)".to_string());

                    sessions.push((id, format!("{} [{msg_count}] {preview}", model), updated, msg_count));
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.2.cmp(&a.2));

    if sessions.is_empty() {
        println!("No sessions found.");
        return;
    }

    println!("Sessions");
    println!();
    for (id, label, _ts, _n) in &sessions {
        let short = if id.len() > 8 { &id[..8] } else { id };
        println!("  {short}  {label}");
    }
    println!();
    println!("  Resume: dscode chat -s <id>");
}

fn rename(id: &str, name: &str) {
    let path = session_path(id);
    if !path.exists() {
        eprintln!("session '{id}' not found");
        return;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("error: {e}"); return; }
    };
    let mut data: serde_json::Value = match serde_json::from_str(&content) {
        Ok(d) => d,
        Err(_) => { eprintln!("session '{id}' is corrupted"); return; }
    };
    data["name"] = serde_json::Value::String(name.to_string());
    match std::fs::write(&path, serde_json::to_string_pretty(&data).unwrap()) {
        Ok(_) => println!("✓ renamed to '{name}'"),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn show(id: &str) {
    let path = sessions_dir().join(format!("{id}.json"));
    if !path.exists() {
        eprintln!("Session '{id}' not found.");
        return;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("Error: {e}"); return; }
    };

    let data = match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(d) => d,
        Err(_) => { eprintln!("Session '{id}' is corrupted."); return; }
    };

    let sid = data["id"].as_str().unwrap_or(id);
    let model = data["model"].as_str().unwrap_or("?");
    let created = data["created_at"].as_i64().unwrap_or(0);
    let updated = data["updated_at"].as_i64().unwrap_or(0);

    println!("Session: {}", if sid.len() > 8 { &sid[..8] } else { sid });
    println!("  model:    {model}");
    println!("  created:  {created}");
    println!("  updated:  {updated}");
    println!();

    if let Some(msgs) = data["messages"].as_array() {
        for msg in msgs {
            let role = msg["role"].as_str().unwrap_or("?");
            let content = msg["content"].as_str().unwrap_or("");
            let preview = if content.len() > 80 {
                format!("{}...", &content[..77])
            } else {
                content.to_string()
            };
            println!("  [{role}] {preview}");
        }
    }
}

fn session_path(id: &str) -> std::path::PathBuf {
    let dir = sessions_dir();
    let path = dir.join(format!("{id}.json"));
    if path.exists() {
        return path;
    }
    // prefix match
    if dir.exists() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "json")
                && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.starts_with(id))
            {
                return p;
            }
        }
    }
    path
}

fn delete(id: &str) {
    let path = session_path(id);
    if !path.exists() {
        eprintln!("Session '{id}' not found.");
        return;
    }
    match std::fs::remove_file(&path) {
        Ok(_) => println!("✓ Session '{id}' deleted."),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn export(id: &str) {
    let path = session_path(id);
    if !path.exists() {
        eprintln!("Session '{id}' not found.");
        return;
    }
    match std::fs::read_to_string(&path) {
        Ok(c) => println!("{c}"),
        Err(e) => eprintln!("Error: {e}"),
    }
}
