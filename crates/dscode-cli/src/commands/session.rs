use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum SessionCommands {
    /// List all sessions
    List,
    /// Show session details
    Show {
        /// Session ID
        id: String,
    },
    /// Delete a session
    #[command(alias = "rm")]
    Delete {
        /// Session ID
        id: String,
    },
    /// Export a session as JSON
    Export {
        /// Session ID
        id: String,
    },
}

pub async fn run(cmd: &SessionCommands) {
    match cmd {
        SessionCommands::List => list(),
        SessionCommands::Show { id } => show(id),
        SessionCommands::Delete { id } => delete(id),
        SessionCommands::Export { id } => export(id),
    }
}

fn state_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.local/share"))
        .join("dscode")
}

fn list() {
    let dir = state_dir().join("sessions");
    if !dir.exists() {
        println!("No sessions found.");
        return;
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => {
            println!("No sessions found.");
            return;
        }
    };

    let mut sessions: Vec<_> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                    let id = path.file_stem().unwrap().to_string_lossy().to_string();
                    let preview = data["preview"].as_str().unwrap_or("(no preview)");
                    let created = data["created_at"].as_i64().unwrap_or(0);
                    let msg_count = data["messages"].as_array().map(|a| a.len()).unwrap_or(0);
                    sessions.push((id, preview.to_string(), created, msg_count));
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.2.cmp(&a.2)); // newest first

    if sessions.is_empty() {
        println!("No sessions found.");
        return;
    }

    println!("Sessions");
    println!();
    for (id, preview, _created, msg_count) in &sessions {
        let short_id = if id.len() > 8 {
            &id[..8]
        } else {
            id.as_str()
        };
        let preview_short = if preview.len() > 50 {
            format!("{}...", &preview[..47])
        } else {
            preview.clone()
        };
        println!("  {short_id}  [{msg_count} msgs] {preview_short}");
    }
}

fn show(id: &str) {
    let path = state_dir().join("sessions").join(format!("{id}.json"));
    if !path.exists() {
        eprintln!("Session '{id}' not found.");
        return;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                println!("Session: {id}");
                println!("  created: {}", data["created_at"].as_i64().unwrap_or(0));
                println!("  model:   {}", data["model"].as_str().unwrap_or("(unknown)"));
                println!("  messages: {}", data["messages"].as_array().map(|a| a.len()).unwrap_or(0));
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
            } else {
                eprintln!("Session '{id}' is corrupted.");
            }
        }
        Err(e) => eprintln!("Error reading session: {e}"),
    }
}

fn delete(id: &str) {
    let path = state_dir().join("sessions").join(format!("{id}.json"));
    if !path.exists() {
        eprintln!("Session '{id}' not found.");
        return;
    }
    match std::fs::remove_file(&path) {
        Ok(_) => println!("✓ Session '{id}' deleted."),
        Err(e) => eprintln!("Error deleting session: {e}"),
    }
}

fn export(id: &str) {
    let path = state_dir().join("sessions").join(format!("{id}.json"));
    if !path.exists() {
        eprintln!("Session '{id}' not found.");
        return;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            println!("{content}");
        }
        Err(e) => eprintln!("Error reading session: {e}"),
    }
}
