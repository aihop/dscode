use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum ToolCommands {
    /// List all available tools
    List,
}

pub async fn run(cmd: &ToolCommands) {
    match cmd {
        ToolCommands::List => list(),
    }
}

fn list() {
    println!("Tools (coming in Phase 2)");
    println!();
    println!("  CodeWhale's tool system will be wired in the next phase.");
    println!("  Planned tools:");
    println!("    - file_read      Read file contents");
    println!("    - file_write     Write to files");
    println!("    - file_edit      Edit files in place");
    println!("    - shell          Execute shell commands");
    println!("    - web_search     Search the web");
    println!("    - fetch_url      Fetch a URL");
}
