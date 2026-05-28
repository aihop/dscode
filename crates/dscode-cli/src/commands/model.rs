use clap::Args;
use codewhale_agent::ModelRegistry;
use codewhale_config::ProviderKind;

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
    println!("  {:<32} tools  reasoning", "id");
    println!("  {}───────────────", "─".repeat(32));

    for model in models {
        let tools = if model.supports_tools { "✓" } else { " " };
        let reasoning = if model.supports_reasoning { "✓" } else { " " };
        println!("  {:<32} {tools}      {reasoning}", model.id);
    }

    println!();
    println!("  DeepSeek models available via api.deepseek.com");
    println!("  Set your API key with `dscode auth login`");
}

fn info(registry: &ModelRegistry, name: &str) {
    let resolution = registry.resolve(Some(name), Some(ProviderKind::Deepseek));

    println!("Model: {}", resolution.resolved.id);
    println!("  provider: {:?}", resolution.resolved.provider);
    println!("  tools:    {}", resolution.resolved.supports_tools);
    println!("  reasoning: {}", resolution.resolved.supports_reasoning);
    if !resolution.resolved.aliases.is_empty() {
        println!("  aliases:  {}", resolution.resolved.aliases.join(", "));
    }
    if resolution.used_fallback {
        println!("  (fallback: {})", resolution.fallback_chain.join(" → "));
    }
}
