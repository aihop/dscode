pub mod commands;

use clap::{Parser, Subcommand};

/// dscode — mobile-first AI agent powered by DeepSeek
#[derive(Debug, Parser)]
#[command(
    name = "dscode",
    version = env!("CARGO_PKG_VERSION"),
    about = "Mobile-first AI agent powered by DeepSeek",
    long_about = "dscode is a mobile-first AI coding agent built on CodeWhale.\n\
                   It connects directly to DeepSeek's API and works entirely\n\
                   through the terminal — perfect for SSH or Termux on your phone.",
    override_usage = "dscode [COMMAND]\n       dscode chat [OPTIONS]\n       dscode run [OPTIONS] <PROMPT>"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Show version info
    #[arg(long, short = 'V')]
    pub version: bool,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start an interactive chat session with DeepSeek
    Chat(commands::chat::ChatArgs),

    /// Run a single prompt and print the response
    #[command(alias = "x")]
    Run(commands::run::RunArgs),

    /// Manage authentication and API keys
    #[command(subcommand)]
    Auth(commands::auth::AuthCommands),

    /// Manage configuration
    #[command(subcommand)]
    Config(commands::config::ConfigCommands),

    /// Manage chat sessions
    #[command(subcommand)]
    Session(commands::session::SessionCommands),

    /// List available models
    Model(commands::model::ModelArgs),

    /// Manage tools
    #[command(subcommand)]
    Tools(commands::tools::ToolCommands),
}

pub fn run() -> std::process::ExitCode {
    // init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    if cli.version {
        println!("dscode v{}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    match &cli.command {
        Some(Commands::Chat(args)) => {
            rt.block_on(commands::chat::run(args));
        }
        Some(Commands::Run(args)) => {
            rt.block_on(commands::run::run(args));
        }
        Some(Commands::Auth(cmd)) => {
            rt.block_on(commands::auth::run(cmd));
        }
        Some(Commands::Config(cmd)) => {
            rt.block_on(commands::config::run(cmd));
        }
        Some(Commands::Session(cmd)) => {
            rt.block_on(commands::session::run(cmd));
        }
        Some(Commands::Model(args)) => {
            rt.block_on(commands::model::run(args));
        }
        Some(Commands::Tools(cmd)) => {
            rt.block_on(commands::tools::run(cmd));
        }
        None => {
            // default: chat with default model
            rt.block_on(commands::chat::run(&commands::chat::ChatArgs {
                model: None,
                session: None,
                no_stream: false,
            }));
        }
    }

    std::process::ExitCode::SUCCESS
}
