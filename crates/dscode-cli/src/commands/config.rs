use clap::Subcommand;
use codewhale_config::ConfigStore;
use std::io::{self, Write};

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Initialize configuration with interactive wizard
    Init,
    /// Show current configuration
    Show,
    /// Set a configuration key (model, base_url, api_key)
    Set { key: String, value: String },
    /// Get a configuration value
    Get { key: String },
}

pub async fn run(cmd: &ConfigCommands) {
    match cmd {
        ConfigCommands::Init => init(),
        ConfigCommands::Show => show(),
        ConfigCommands::Set { key, value } => set(key, value),
        ConfigCommands::Get { key } => get(key),
    }
}

fn config_path() -> std::path::PathBuf {
    crate::utils::dscode_dir().join("config.toml")
}

fn load_store() -> ConfigStore {
    let path = config_path();
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

fn read_line(prompt: &str) -> String {
    print!("{prompt}");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn init() {
    let path = config_path();
    if path.exists() {
        // Check if already configured
        let store = load_store();
        if store.config.api_key.is_some() {
            let r = read_line("Config already exists. Override? [y/N] ");
            if r.to_lowercase() != "y" {
                return;
            }
        }
    }

    println!("dscode configuration wizard");
    println!("────────────────────────────");
    println!();

    // Step 1: API Key
    println!("Step 1: DeepSeek API Key");
    println!("  Get your key at: https://platform.deepseek.com/api_keys");
    let key = loop {
        let input = read_line("  Enter API key (sk-...): ");
        if input.starts_with("sk-") && input.len() > 10 {
            break input;
        }
        if input.is_empty() {
            println!("  Skipping. Set later with: dscode auth login");
            break String::new();
        }
        println!("  Key must start with 'sk-' and be at least 10 chars");
    };

    // Step 2: Model preference
    println!();
    println!("Step 2: Default Model");
    println!("  1) deepseek-v4-pro   (best quality, slower)");
    println!("  2) deepseek-v4-flash (faster, good for chat)");
    println!("  3) deepseek-r1       (deep reasoning)");
    let model = loop {
        let input = read_line("  Choose [1-3, default=1]: ");
        break match input.trim() {
            "2" | "flash" => "deepseek-v4-flash".to_string(),
            "3" | "r1" => "deepseek-r1".to_string(),
            _ => "deepseek-v4-pro".to_string(),
        };
    };

    // Step 3: Base URL
    println!();
    println!("Step 3: API Base URL (press Enter for default)");
    let base_url = read_line("  Base URL [https://api.deepseek.com/beta]: ");
    let base_url = if base_url.is_empty() {
        "https://api.deepseek.com/beta".to_string()
    } else {
        base_url
    };

    // Save
    let mut store = load_store();
    if !key.is_empty() {
        store.config.api_key = Some(key);
    }
    store.config.providers.deepseek.model = Some(model.clone());
    store.config.providers.deepseek.base_url = Some(base_url);
    save_store(&store);

    println!();
    println!("✓ Configuration saved");
    println!("  File: {}", path.display());
    println!("  Model: {model}");
    println!();
    println!("  Next: dscode chat");
}

fn show() {
    let store = load_store();
    let cfg = &store.config;

    let path = config_path();
    println!("Configuration");
    println!("  path:     {}", path.display());
    println!("  version:  {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("DeepSeek API:");
    println!("  key:      {}", mask_key(cfg.api_key.as_deref()));
    println!("  model:    {}", cfg.providers.deepseek.model.as_deref().unwrap_or("deepseek-v4-pro"));
    println!("  base_url: {}", cfg.providers.deepseek.base_url.as_deref().unwrap_or("https://api.deepseek.com/beta"));
    println!();
    println!("Environment:");
    let env_key = std::env::var("DEEPSEEK_API_KEY").ok();
    println!("  DEEPSEEK_API_KEY: {}", mask_key(env_key.as_deref()));
}

fn set(key: &str, value: &str) {
    let mut store = load_store();

    match key {
        "model" => store.config.providers.deepseek.model = Some(value.to_string()),
        "base_url" | "base-url" => store.config.providers.deepseek.base_url = Some(value.to_string()),
        "api_key" | "api-key" | "key" => store.config.api_key = Some(value.to_string()),
        _ => {
            eprintln!("error: unknown config key '{key}'");
            eprintln!("  known keys: model, base_url, api_key");
            std::process::exit(1);
        }
    }

    save_store(&store);
    println!("✓ {key} set");
}

fn get(key: &str) {
    let store = load_store();

    let value = match key {
        "model" => store.config.providers.deepseek.model.clone(),
        "base_url" | "base-url" => store.config.providers.deepseek.base_url.clone(),
        "api_key" | "api-key" | "key" => store.config.api_key.as_deref().map(|k| mask_key(Some(k))),
        _ => {
            eprintln!("error: unknown config key '{key}'");
            std::process::exit(1);
        }
    };

    match value {
        Some(v) => println!("{v}"),
        None => println!("(not set)"),
    }
}

fn mask_key(key: Option<&str>) -> String {
    match key {
        Some(k) if k.len() > 8 => format!("...{} (set)", &k[k.len().saturating_sub(4)..]),
        Some(_) => "(set)".to_string(),
        None => "(not set)".to_string(),
    }
}
