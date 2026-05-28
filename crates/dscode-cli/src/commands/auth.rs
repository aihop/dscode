use clap::Subcommand;
use codewhale_config::ConfigStore;
use std::io::{self, Write};

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    /// Log in by setting your DeepSeek API key
    Login {
        /// API key (omit for interactive prompt)
        api_key: Option<String>,
    },
    /// Log out by removing the stored API key
    Logout,
    /// Show authentication status
    Status,
    /// Test API key by calling DeepSeek API
    Test,
}

pub async fn run(cmd: &AuthCommands) {
    match cmd {
        AuthCommands::Login { api_key } => login(api_key.as_deref()),
        AuthCommands::Logout => logout(),
        AuthCommands::Status => status(),
        AuthCommands::Test => test().await,
    }
}

fn config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode")
        .join("config.toml")
}

fn load_store() -> ConfigStore {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    match ConfigStore::load(Some(path.clone())) {
        Ok(s) => s,
        Err(_) => {
            // Corrupt file: back it up and start fresh
            if path.exists() {
                let backup = path.with_extension("toml.bak");
                let _ = std::fs::rename(&path, &backup);
            }
            ConfigStore::load(Some(path)).expect("fresh config should load")
        }
    }
}

fn save_store(store: &ConfigStore) {
    if let Err(e) = store.save() {
        eprintln!("error: failed to save config: {e}");
        std::process::exit(1);
    }
}

fn login(api_key: Option<&str>) {
    let key = match api_key {
        Some(k) => k.trim().to_string(),
        None => {
            let input = rpassword::prompt_password("Enter your DeepSeek API key: ")
                .unwrap_or_else(|_| {
                    // fallback if rpassword fails (e.g. no tty)
                    print!("Enter your DeepSeek API key: ");
                    io::stdout().flush().unwrap();
                    let mut buf = String::new();
                    io::stdin().read_line(&mut buf).unwrap();
                    buf
                });
            input.trim().to_string()
        }
    };

    if key.is_empty() {
        eprintln!("error: API key cannot be empty");
        std::process::exit(1);
    }

    if !key.starts_with("sk-") {
        eprintln!("warning: API key should start with 'sk-'");
    }

    let mut store = load_store();
    store.config.api_key = Some(key.clone());
    save_store(&store);

    println!("✓ API key saved");
    println!("  config: {}", config_path().display());
    println!("  last 4 chars: ...{}", &key[key.len().saturating_sub(4)..]);
}

fn logout() {
    let mut store = load_store();
    store.config.api_key = None;
    save_store(&store);

    println!("✓ API key removed");
}

fn status() {
    let store = load_store();

    let config_key = store.config.api_key.as_deref();
    let config_has = config_key.is_some_and(|k| !k.trim().is_empty());

    let env_key = std::env::var("DEEPSEEK_API_KEY").ok();
    let env_has = env_key.as_deref().is_some_and(|k| !k.trim().is_empty());

    let active_source = match (config_has, env_has) {
        (true, _) => "config",
        (false, true) => "environment",
        (false, false) => "none",
    };

    println!("Authentication status");
    println!();
    print_source(
        "config file",
        config_has,
        config_key.map(|k| &k[k.len().saturating_sub(4)..]),
    );
    print_source(
        "environment (DEEPSEEK_API_KEY)",
        env_has,
        env_key.as_deref().map(|k| &k[k.len().saturating_sub(4)..]),
    );
    println!();

    if config_has || env_has {
        println!("✓ Active source: {active_source}");
    } else {
        println!("✗ No API key found");
        println!("  Run `dscode auth login` to set one up");
    }
}

async fn test() {
    let api_key = resolve_api_key_from_store();
    match &api_key {
        Some(k) if !k.trim().is_empty() => {
            println!("Testing API key: ...{}", &k[k.len().saturating_sub(4)..]);
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap();
            match client
                .get("https://api.deepseek.com/models")
                .header("Authorization", format!("Bearer {k}"))
                .header("Accept", "application/json")
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    println!("✓ API key is valid");
                    println!("  status: {}", resp.status());
                }
                Ok(resp) => {
                    println!("✗ API key rejected (status {})", resp.status());
                    println!("  The key may be invalid or expired");
                }
                Err(e) => {
                    println!("✗ Connection failed: {e}");
                }
            }
        }
        _ => {
            println!("No API key configured");
            println!("  Run: dscode auth login");
        }
    }
}

fn resolve_api_key_from_store() -> Option<String> {
    if let Ok(k) = std::env::var("DEEPSEEK_API_KEY") {
        if !k.trim().is_empty() { return Some(k); }
    }
    let mut path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(store) = ConfigStore::load(Some(path)) {
        if let Some(k) = store.config.api_key {
            if !k.trim().is_empty() { return Some(k); }
        }
    }
    None
}

fn print_source(name: &str, present: bool, last4: Option<&str>) {
    let icon = if present { "✓" } else { " " };
    let details = match (present, last4) {
        (true, Some(l4)) => format!(" (last 4: {l4})"),
        (true, None) => " (present)".to_string(),
        (false, _) => String::new(),
    };
    println!("  {icon} {name}{details}");
}
