use clap::Args;
use codewhale_agent::ModelRegistry;

#[derive(Debug, Args)]
pub struct ModelArgs {
    /// Model name to inspect
    pub name: Option<String>,
}

pub async fn run(args: &ModelArgs) {
    let registry = ModelRegistry::default();

    match &args.name {
        Some(name) => info(&registry, name),
        None => list(&registry),
    }
}

fn list(registry: &ModelRegistry) {
    let models = registry.list();

    println!("Available models");
    println!();
    println!("  {:<30} tools  reasoning", "id");
    println!("  ─────────────────────────────────────────");

    for model in models {
        let tools = if model.supports_tools { "✓" } else { " " };
        let reasoning = if model.supports_reasoning { "✓" } else { " " };
        println!("  {:<30} {tools}      {reasoning}", model.id);
    }

    println!();
    println!("  DeepSeek models are available via api.deepseek.com");
    println!("  Set your API key with `dscode auth login`");
}

fn info(registry: &ModelRegistry, name: &str) {
    match registry.resolve(name) {
        Some(info) => {
            println!("Model: {}", info.id);
            println!("  provider: {:?}", info.provider);
            println!("  tools:    {}", info.supports_tools);
            println!("  reasoning: {}", info.supports_reasoning);
            if !info.aliases.is_empty() {
                println!("  aliases:  {}", info.aliases.join(", "));
            }
        }
        None => {
            eprintln!("Model '{name}' not found in registry.");
            eprintln!("  Run `dscode model` to see available models.");
        }
    }
}
