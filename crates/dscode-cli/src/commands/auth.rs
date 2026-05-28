use anyhow::{Context, Result};
use clap::Subcommand;
use codewhale_config::{ConfigStore, ConfigToml, ProviderKind};
use codewhale_secrets::Secrets;
use std::io::{self, Write};

#[derive(Debug, Subcommand)]
pub enum AuthCommands {
    /// Log in by setting your DeepSeek API key
    #[command(alias = "add")]
    Login {
        /// API key (omit for interactive prompt)
        api_key: Option<String>,
    },

    /// Log out by removing the stored API key
    #[command(alias = "remove", alias = "rm")]
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

    // Validate format: sk-xxxx
    if !key.starts_with("sk-") {
        eprintln!("warning: API key should start with 'sk-'");
    }

    // Store in config file
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode");

    std::fs::create_dir_all(&config_dir).unwrap();

    let config_path = config_dir.join("config.toml");
    let mut store = ConfigStore::load_or_default(&config_path).unwrap_or_default();
    store.config.api_key = Some(key.clone());

    if let Err(e) = store.save(&config_path) {
        eprintln!("error: failed to save config: {e}");
        std::process::exit(1);
    }

    // Also store in secrets store
    let secrets_path = config_dir.join("secrets.json");
    let secrets = Secrets::new(&secrets_path).unwrap_or_default();
    if let Err(e) = secrets.set("deepseek", &key) {
        eprintln!("warning: failed to store in secret store: {e}");
    }

    println!("✓ API key saved");
    println!("  config: {}", config_path.display());
    println!("  last 4 chars: ...{}", &key[key.len().saturating_sub(4)..]);
}

fn logout() {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode");
    let config_path = config_dir.join("config.toml");

    let mut store = ConfigStore::load_or_default(&config_path).unwrap_or_default();
    store.config.api_key = None;

    if let Err(e) = store.save(&config_path) {
        eprintln!("error: failed to save config: {e}");
        std::process::exit(1);
    }

    // Also clear secrets
    let secrets_path = config_dir.join("secrets.json");
    let secrets = Secrets::new(&secrets_path).unwrap_or_default();
    let _ = secrets.delete("deepseek");

    println!("✓ API key removed");
}

fn status() {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode");
    let config_path = config_dir.join("config.toml");

    let store = ConfigStore::load_or_default(&config_path).unwrap_or_default();

    // Check config file
    let config_key = store.config.api_key.as_deref();
    let config_has = config_key.is_some_and(|k| !k.trim().is_empty());

    // Check env
    let env_key = std::env::var("DEEPSEEK_API_KEY").ok();
    let env_has = env_key.as_deref().is_some_and(|k| !k.trim().is_empty());

    // Check secrets store
    let secrets_path = config_dir.join("secrets.json");
    let secrets = Secrets::new(&secrets_path).unwrap_or_default();
    let secret_key = secrets.get("deepseek").ok().flatten();
    let secret_has = secret_key.as_deref().is_some_and(|k| !k.trim().is_empty());

    let total = [config_has, env_has, secret_has].iter().filter(|&&v| v).count();

    println!("Authentication status");
    println!();

    let active_source = if config_has {
        "config"
    } else if secret_has {
        "secret store"
    } else if env_has {
        "environment"
    } else {
        "none"
    };

    print_source("config file", config_has, config_key.map(|k| &k[k.len().saturating_sub(4)..]));
    print_source("secret store", secret_has, secret_key.as_deref().map(|k| &k[k.len().saturating_sub(4)..]));
    print_source("environment (DEEPSEEK_API_KEY)", env_has, env_key.as_deref().map(|k| &k[k.len().saturating_sub(4)..]));

    println!();
    if total > 0 {
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
