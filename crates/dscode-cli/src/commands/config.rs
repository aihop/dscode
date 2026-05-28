use clap::{Args, Subcommand};
use codewhale_config::{ConfigStore, ConfigToml};

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Initialize default configuration
    Init,

    /// Show current configuration
    Show,

    /// Set a configuration key (e.g. model, api_key)
    Set {
        /// Configuration key name
        key: String,
        /// Configuration value
        value: String,
    },

    /// Get a configuration value
    Get {
        /// Configuration key name
        key: String,
    },
}

pub async fn run(cmd: &ConfigCommands) {
    match cmd {
        ConfigCommands::Init => init(),
        ConfigCommands::Show => show(),
        ConfigCommands::Set { key, value } => set(key, value),
        ConfigCommands::Get { key } => get(key),
    }
}

fn config_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~/.config"))
        .join("dscode")
}

fn config_path() -> std::path::PathBuf {
    config_dir().join("config.toml")
}

fn init() {
    let path = config_path();
    if path.exists() {
        println!("Config already exists at: {}", path.display());
        return;
    }

    std::fs::create_dir_all(config_dir()).unwrap();

    let config = ConfigToml::default();
    let toml_str = toml::to_string_pretty(&config).unwrap();
    std::fs::write(&path, &toml_str).unwrap();

    println!("✓ Config initialized at: {}", path.display());
}

fn show() {
    let path = config_path();
    let store = ConfigStore::load_or_default(&path).unwrap_or_default();

    println!("Configuration ({})", path.display());
    println!();

    // Print non-sensitive config
    println!("  provider: {:?}", store.config.provider);
    println!("  model:     {}", store.config.providers.deepseek.model.as_deref().unwrap_or("(default)"));
    println!("  base_url:  {}", store.config.providers.deepseek.base_url.as_deref().unwrap_or("https://api.deepseek.com/beta"));

    let has_key = store.config.api_key.as_deref().is_some_and(|k| !k.trim().is_empty());
    if has_key {
        let key = store.config.api_key.as_deref().unwrap();
        println!("  api_key:   ...{} (set)", &key[key.len().saturating_sub(4)..]);
    } else {
        println!("  api_key:   (not set)");
    }
}

fn set(key: &str, value: &str) {
    let path = config_path();
    let mut store = ConfigStore::load_or_default(&path).unwrap_or_default();

    match key {
        "model" => {
            store.config.providers.deepseek.model = Some(value.to_string());
        }
        "base_url" | "base-url" => {
            store.config.providers.deepseek.base_url = Some(value.to_string());
        }
        "api_key" | "api-key" | "key" => {
            store.config.api_key = Some(value.to_string());
        }
        _ => {
            eprintln!("error: unknown config key '{key}'");
            eprintln!("  known keys: model, base_url, api_key");
            std::process::exit(1);
        }
    }

    if let Err(e) = store.save(&path) {
        eprintln!("error: failed to save config: {e}");
        std::process::exit(1);
    }

    println!("✓ {key} set");
}

fn get(key: &str) {
    let path = config_path();
    let store = ConfigStore::load_or_default(&path).unwrap_or_default();

    let value = match key {
        "model" => store.config.providers.deepseek.model.clone(),
        "base_url" | "base-url" => store.config.providers.deepseek.base_url.clone(),
        "api_key" | "api-key" | "key" => {
            store.config.api_key.as_deref().map(|k| {
                if k.len() > 8 {
                    format!("...{}", &k[k.len().saturating_sub(4)..])
                } else {
                    "(set)".to_string()
                }
            })
        }
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
