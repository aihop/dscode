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
}

pub async fn run(cmd: &AuthCommands) {
    match cmd {
        AuthCommands::Login { api_key } => login(api_key.as_deref()),
        AuthCommands::Logout => logout(),
        AuthCommands::Status => status(),
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
    // ensure parent dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    ConfigStore::load(Some(path)).unwrap()
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
            print!("Enter your DeepSeek API key: ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
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

fn print_source(name: &str, present: bool, last4: Option<&str>) {
    let icon = if present { "✓" } else { " " };
    let details = match (present, last4) {
        (true, Some(l4)) => format!(" (last 4: {l4})"),
        (true, None) => " (present)".to_string(),
        (false, _) => String::new(),
    };
    println!("  {icon} {name}{details}");
}
