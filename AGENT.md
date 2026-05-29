# dscode — AI Agent Guide

## Project

dscode is a mobile-first AI coding agent, powered by DeepSeek.
It is a thin CLI frontend over the CodeWhale engine.

## Core principles

1. **CodeWhale first** — reuse codewhale-* crates from crates.io. Do not reimplement
   what CodeWhale already provides (config, state, agent, tools).
2. **DeepSeek-only** — no other providers. All API calls go to api.deepseek.com.
3. **Pure CLI** — no TUI, no web UI. stdin/stdout only.
4. **Mobile-optimized** — narrow terminal (≤80 cols), SSH/Termux/iSH friendly.
5. **Minimal dependencies** — prefer Rust standard library over heavy crates.

## Architecture

```
crates/dscode-cli/src/
├── api.rs          ← Shared API layer (calls DeepSeek API, tool definitions)
├── lib.rs          ← CLI routing (clap -> subcommand dispatch)
├── main.rs         ← Entry point
├── bin/dsc.rs      ← `dsc` alias entry
└── commands/
    ├── chat.rs     ← Interactive chat with agent mode (tools)
    ├── run.rs      ← One-shot prompt
    ├── auth.rs     ← API key management
    ├── config.rs   ← Configuration
    ├── session.rs  ← Session list/show/rename/delete
    ├── model.rs    ← Model list/info
    ├── tools.rs    ← Tool management
    └── completion.rs ← Shell completion generation
```

## Build & test

```bash
cargo build --release -p dscode   # release build
cargo check -p dscode              # check without building
cargo test -p dscode               # run unit tests
```

## Code conventions

- Use `crate::api::*` for all DeepSeek API calls
- Keep files under 500 lines; extract to api.rs for shared logic
- Zero compiler warnings
- Every logical change gets its own git commit
- Commit messages: `type: description` (feat/fix/refactor/docs/test/chore/perf)

## Tools (agent mode)

When adding a new tool:
1. Add definition in `api.rs` `tool_definitions()` as a JSON object
2. Add execution handler in `api.rs` `execute_tool()` match arm
3. Test with `dscode` (agent mode is now default)

Existing tools: read_file, write_file, edit_file, run_shell, search_code,
list_files, git_status, git_diff, git_commit, git_log, web_search, fetch_url.
