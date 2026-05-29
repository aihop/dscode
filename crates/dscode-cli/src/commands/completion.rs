/// Generate shell completion scripts
///
/// Usage: dscode completion <SHELL>
///        dscode completion bash  >  /etc/bash_completion.d/dscode
///        dscode completion zsh   >> ~/.zshrc
///        dscode completion fish  >  ~/.config/fish/completions/dscode.fish

use clap::CommandFactory;
use clap_complete::Shell;
use std::io;

#[derive(Debug, clap::Args)]
pub struct CompletionArgs {
    /// Shell to generate completion for
    #[arg(value_enum)]
    pub shell: Shell,
}

pub fn run(args: &CompletionArgs) {
    let mut cmd = crate::Cli::command();
    let name = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, name, &mut io::stdout());
}
