/// One-shot prompt to DeepSeek with streaming + agent tool support.
///
/// Receives prompt from args or stdin pipe.
/// Runs a full agent loop: model -> tool_calls -> execute -> model -> ...

use crate::api::{self, resolve_model_name, resolve_api_key, resolve_base_url};
use crate::engine::{AgentEngine, AgentOptions};
use crate::tools;
use crate::utils::{is_narrow_terminal, terminal_width};
use clap::Args;
use std::io::Read;

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Prompt (omit to read from stdin pipe)
    pub prompt: Vec<String>,
    /// Model (default: v4-flash, or config value)
    #[arg(short = 'm', long)]
    pub model: Option<String>,
    /// Disable streaming
    #[arg(long)]
    pub no_stream: bool,
    /// Approval mode: confirm before writing files or running shell commands
    #[arg(long)]
      pub approve: bool}
pub async fn run(args: &RunArgs) {
    api::ensure_default_config();
    let model = resolve_model_name(
        &args.model.clone().unwrap_or_else(|| api::default_model(true)),
    );
    let _stream = !args.no_stream;

    let prompt = if !args.prompt.is_empty() {
        args.prompt.join(" ")
    } else {
        let mut buf = String::new();
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        if handle.read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            buf.trim().to_string()
        } else {
            eprintln!("error: prompt required");
            std::process::exit(1);
        }
    };

    let api_key = resolve_api_key().unwrap_or_else(|| {
        eprintln!("error: no DeepSeek API key found");
        std::process::exit(1);
    });
    let base_url = resolve_base_url();
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
        .build().unwrap();

    let narrow = is_narrow_terminal();
    let tw = terminal_width();

      let tools_list = Some(api::tool_definitions_filtered(tools::CORE_TOOL_NAMES));    
    let mut sys_content = "\
You are dscode, a mobile-first AI coding agent powered by DeepSeek.
You are running directly in the project root directory. Always use relative paths for tools.

## Reasoning & Planning (MANDATORY)
- **Plan-First**: Before using any tools for a new task, you MUST output a detailed plan. Break down the task into logical steps.
- **Checklist**: For complex multi-step tasks, use `checklist_write` to track your progress. Update it as you complete steps.
- **Investigate First**: If you are unsure about the codebase, always use `list_symbols` to see file structure, then `read_file` for specific code. Use `search_code` or `search_symbols` for global discovery.
- **Concise Reasoning**: Before each tool call, state your reasoning concisely in one sentence.

## Code Quality & Verification
- **Edit Strategy**: The most reliable path: read_file → write_file. Never fails.
- **Self-Correction**: If edit_file fails once, switch immediately to write_file. Do not retry.
- **Auto-Verification**: After every significant code change, run `cargo check` (or relevant linter) to catch errors.
- **Precision**: When using `edit_file`, always provide a `line` hint to avoid ambiguity and speed up matching.
- **Minimal Diffs**: Change only what is needed. Avoid unrelated reformatting.
- **Single-file limit**: Files exceeding 400 lines should be split into focused modules.

## Communication
- Be extremely concise. Use markdown code blocks for code, paths, and commands.
- Report actual output. Never claim success without evidence.
- If uncertain, say \"I don't know\" — never fabricate.\
".to_string();
    if let Some(ap) = api::load_agent_md() {
        sys_content = format!("{}\n\n{}", sys_content, ap);
    }
    let mut api_msgs: Vec<serde_json::Value> = Vec::new();
    api_msgs.push(serde_json::json!({"role": "user", "content": prompt}));

    let engine = AgentEngine::new(client, base_url, api_key);
      let options = AgentOptions {
          model,
          system_prompt: sys_content,
          tools: tools_list,
          max_rounds: 80,
          narrow,
          silent: false,
          approval_mode: args.approve,
          terminal_width: tw,
          cwd: std::env::current_dir().unwrap_or_default(),
          reasoning_effort: Some("medium".to_string()),
      };
    match engine.run_loop(&options, api_msgs).await {
        Ok(_) => {}
        Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
    }
}
