use clap::Subcommand;
use codewhale_state::{StateStore, ThreadListFilters};

#[derive(Debug, Subcommand)]
pub enum SessionCommands {
    /// List sessions (optionally filtered by current project)
    List {
        #[arg(long, short = 'p', help = "Show only sessions for current project directory")]
        project: bool,
    },
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

fn open_store() -> Option<StateStore> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    StateStore::open(Some(path)).ok()
}

fn db_path() -> std::path::PathBuf {
    crate::utils::dscode_dir().join("state.db")
}

/// Find thread by exact or prefix id.
fn find_thread(store: &StateStore, id: &str) -> Option<(codewhale_state::ThreadMetadata, Vec<codewhale_state::MessageRecord>)> {
    // Exact match
    if let Ok(Some(t)) = store.get_thread(id) {
        let msgs = store.list_messages(&t.id, None).unwrap_or_default();
        return Some((t, msgs));
    }
    // Prefix match
    let threads = store.list_threads(ThreadListFilters { include_archived: false, limit: Some(100) }).ok()?;
    for t in threads {
        if t.id.starts_with(id) {
            let msgs = store.list_messages(&t.id, None).unwrap_or_default();
            return Some((t, msgs));
        }
    }
    None
}

pub async fn run(cmd: &SessionCommands) {
    let Some(store) = open_store() else {
        eprintln!("error: could not open session store");
        return;
    };

    match cmd {
        SessionCommands::List { project } => list(&store, *project),
        SessionCommands::Show { id } => show(&store, id),
        SessionCommands::Rename { id, name } => rename(&store, id, name),
        SessionCommands::Delete { id } => delete(&store, id),
        SessionCommands::Export { id } => export(&store, id),
    }
}

fn list(store: &StateStore, project: bool) {
    let threads = match store.list_threads(ThreadListFilters { include_archived: false, limit: Some(100) }) {
        Ok(t) => t,
        Err(_) => { println!("No sessions found."); return; }
    };

    if threads.is_empty() {
        println!("No sessions found.");
        return;
    }

    let cwd = if project { Some(std::env::current_dir().unwrap_or_default()) } else { None };

    let header = if project { "Sessions (current project)" } else { "Sessions" };
    println!("{header}");

    for t in &threads {
        // Filter by project if --project flag is set
        if let Some(ref cwd) = cwd {
            if t.cwd != *cwd { continue; }
        }

        let short = if t.id.len() > 8 { &t.id[..8] } else { &t.id };
        let name_tag = t.name.as_deref().map(|n| format!(" ({n})")).unwrap_or_default();

        // Show short project path
        let project_tag = t.cwd.file_name()
            .map(|n| format!(" \x1B[36m[{}]\x1B[0m", n.to_string_lossy()))
            .unwrap_or_default();

        let preview = if t.preview.len() > 40 {
            format!("{}...", &t.preview[..37])
        } else {
            t.preview.clone()
        };
        let msgs = store.list_messages(&t.id, Some(1)).unwrap_or_default();
        let count_hint = if msgs.is_empty() { "0".to_string() }
            else { format!("~{}", msgs.len() * 2) };
        println!("  {short}{name_tag}{project_tag}  {} [{}] {}", t.model_provider, count_hint, preview);
    }
    println!();
    if project {
        println!("  All sessions: dscode session list");
    } else {
        println!("  Current project: dscode session list -p");
    }
    println!("  Resume: dscode chat -s <id>");
}

fn show(store: &StateStore, id: &str) {
    let (thread, msgs) = match find_thread(store, id) {
        Some(v) => v,
        None => { eprintln!("Session '{id}' not found."); return; }
    };

    let sid = if thread.id.len() > 8 { &thread.id[..8] } else { &thread.id };
    println!("Session: {sid}");
    if let Some(ref name) = thread.name { println!("  name:     {name}"); }
    println!("  model:    {}", thread.model_provider);
    println!("  project:  {}", thread.cwd.display());
    println!("  created:  {}", thread.created_at);
    println!("  updated:  {}", thread.updated_at);
    println!();

    for msg in &msgs {
        let preview = if msg.content.len() > 80 {
            format!("{}...", &msg.content[..77])
        } else {
            msg.content.clone()
        };
        println!("  [{}] {preview}", msg.role);
    }
}

fn rename(store: &StateStore, id: &str, name: &str) {
    let (mut thread, _) = match find_thread(store, id) {
        Some(v) => v,
        None => { eprintln!("session '{id}' not found"); return; }
    };
    thread.name = Some(name.to_string());
    thread.updated_at = chrono::Utc::now().timestamp();
    match store.upsert_thread(&thread) {
        Ok(_) => println!("✓ renamed to '{name}'"),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn delete(store: &StateStore, id: &str) {
    let (thread, _) = match find_thread(store, id) {
        Some(v) => v,
        None => { eprintln!("Session '{id}' not found."); return; }
    };
    match store.delete_thread(&thread.id) {
        Ok(_) => println!("✓ Session '{}' deleted.", &thread.id[..8.min(thread.id.len())]),
        Err(e) => eprintln!("Error: {e}"),
    }
}

fn export(store: &StateStore, id: &str) {
    let (thread, msgs) = match find_thread(store, id) {
        Some(v) => v,
        None => { eprintln!("Session '{id}' not found."); return; }
    };

    let json = serde_json::json!({
        "id": thread.id,
        "model": thread.model_provider,
        "name": thread.name,
        "created_at": thread.created_at,
        "updated_at": thread.updated_at,
        "messages": msgs.iter().map(|m| serde_json::json!({
            "role": m.role,
            "content": m.content,
            "created_at": m.created_at,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
}
