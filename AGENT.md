# dscode

**dscode** is a mobile-first DeepSeek AI coding agent (CLI-only).

## Project Structure

```
crates/dscode-cli/src/
├── api.rs              ← DeepSeek API client, SSE streaming, Markdown→ANSI render
├── lib.rs              ← CLI routing (clap subcommands)
├── main.rs             Entry point
├── bin/dsc.rs          ← `dsc` short alias
└── commands/
    ├── chat.rs         ← Interactive agent loop
    ├── run.rs          ← One-shot prompt
    ├── auth.rs         ← API key management
    ├── config.rs       ← Config wizard
    ├── session.rs      ← SQLite session CRUD
    ├── model.rs        ← Model info
    ├── tools.rs        ← Tool listing
    └── completion.rs   ← Shell completions
```

## Tech Stack

- **Language:** Rust 2021 edition, MSRV 1.75
- **Key deps:** reqwest, tokio, rustyline, serde_json
- **codewhale crates:** codewhale-tools, codewhale-config, codewhale-state, codewhale-execpolicy, codewhale-protocol
- **Storage:** SQLite via codewhale-state (~/.local/share/dscode/state.db)
- **Config:** TOML at ~/.config/dscode/config.toml
- **Binary:** ~3.4MB statically linked
- **License:** MIT

## Agent Loop

Chat mode is agentic by default with 13 tools (file I/O, shell, git, search, web, fetch, patch). Max 15 tool-call rounds per turn. Model fallback: v4-pro → v4-flash on API error.

## Build

```bash
cargo build --release -p dscode
cargo check -p dscode
```

## Links

- Repository: https://github.com/aihop/dscode
- API: https://api.deepseek.com/beta
