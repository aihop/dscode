use clap::Subcommand;
use codewhale_config::ConfigStore;

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Initialize default configuration
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
    ConfigStore::load(Some(path)).unwrap()
}

fn save_store(store: &ConfigStore) {
    if let Err(e) = store.save() {
        eprintln!("error: failed to save config: {e}");
        std::process::exit(1);
    }
}

fn init() {
    let path = config_path();
    if path.exists() {
        println!("Config already exists at: {}", path.display());
        return;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let store = load_store();
    save_store(&store);
    println!("✓ Config initialized at: {}", path.display());
}

fn show() {
    let store = load_store();
    let cfg = &store.config;

    println!("Configuration");
    println!("  path:     {}", config_path().display());
    println!("  api_key:  {}", mask_key(cfg.api_key.as_deref()));
    println!();
    println!("DeepSeek provider:");
    println!("  model:    {}", cfg.providers.deepseek.model.as_deref().unwrap_or("deepseek-v4-pro"));
    println!("  base_url: {}", cfg.providers.deepseek.base_url.as_deref().unwrap_or("https://api.deepseek.com/beta"));
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
