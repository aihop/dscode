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

/// DeepSeek model families with usage recommendations
static DS_MODELS: &[(&str, &str, &str, bool, bool)] = &[
    ("deepseek-v4-pro",  "Main",  "Best quality, reasoning + tools",  true,  true),
    ("deepseek-v4-flash", "Fast",  "Fast responses, good for chat",    true,  true),
    ("deepseek-v3",       "Legacy","Previous gen, stable",             true,  true),
    ("deepseek-v3.2",     "Legacy","V3 update",                        true,  true),
    ("deepseek-r1",       "Reason","Deep reasoning, math/code",        false, true),
    ("deepseek-chat",     "Alias","Points to v4-flash",                true,  true),
    ("deepseek-reasoner", "Alias","Points to v4-pro (reasoning)",      true,  true),
    ("deepseek-coder",    "Legacy","Code-focused (deprecated)",        true,  false),
];

fn list(registry: &ModelRegistry) {
    // dscode is DeepSeek-only — filter out other providers
    let models: Vec<_> = registry.list()
        .into_iter()
        .filter(|m| m.provider == codewhale_config::ProviderKind::Deepseek)
        .collect();

    println!("Available DeepSeek models");
    println!();
    println!("  {:<26} tools reasoning", "id");
    println!("  {}", "─".repeat(42));

    for m in &models {
        let t = if m.supports_tools { "✓" } else { " " };
        let r = if m.supports_reasoning { "✓" } else { " " };
        println!("  {:<26} {t}      {r}", m.id);
    }

    // Show usage guide
    println!();
    println!("  Usage guide:");
    println!("    dscode chat                 # default: v4-pro");
    println!("    dscode chat -m v4-flash     # faster responses");
    println!("    dscode chat -m r1           # deep reasoning");
    println!("    dscode run -m v4-flash ...  # quick one-shot");
    println!();
    println!("  Short names: v4-pro, v4-flash, v3, r1, chat, coder");
}

fn info(registry: &ModelRegistry, name: &str) {
    // Try unified short name first
    let full_name = match name {
        "v4-pro" | "v4pro"   => "deepseek-v4-pro",
        "v4-flash" | "v4flash" | "flash" => "deepseek-v4-flash",
        "v3"                  => "deepseek-v3",
        "v3.2" | "v32"       => "deepseek-v3.2",
        "r1"                  => "deepseek-r1",
        "chat"                => "deepseek-chat",
        "coder"               => "deepseek-coder",
        "reasoner"            => "deepseek-reasoner",
        other                 => other,
    };

    let resolution = registry.resolve(Some(full_name), Some(ProviderKind::Deepseek));

    println!("Model: {}", resolution.resolved.id);
    println!("  provider:   {:?}", resolution.resolved.provider);
    println!("  tools:      {}", if resolution.resolved.supports_tools { "yes" } else { "no" });
    println!("  reasoning:  {}", if resolution.resolved.supports_reasoning { "yes" } else { "no" });

    if !resolution.resolved.aliases.is_empty() {
        println!("  aliases:    {}", resolution.resolved.aliases.join(", "));
    }
    if resolution.used_fallback {
        println!("  (fallback: {})", resolution.fallback_chain.join(" → "));
    }

    // Show usage recommendation
    for &(id, cat, desc, _, _) in DS_MODELS {
        if id == resolution.resolved.id {
            println!("  category:   {cat}");
            println!("  note:       {desc}");
            break;
        }
    }
}
