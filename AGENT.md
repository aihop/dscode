# dscode — AI Guide

## Project Overview

**dscode** is a mobile-first, terminal-native AI coding agent powered exclusively by DeepSeek.
It is a thin CLI frontend over the dscode engine,
providing an agentic coding experience optimized for narrow terminals (SSH, Termux, iSH).

- **Language:** Rust 2021 edition (MSRV 1.75)
- **Binary:** ~7.5MB statically linked, zero runtime dependencies
- **License:** MIT
- **Repository:** <https://github.com/aihop/dscode>

---

## Core Principles

1. **dscode first** — Reuse `codewhale-*` crates from crates.io. Never reimplement
   what the engine already provides: config, state, agent orchestration, tools, MCP.
2. **DeepSeek-only** — No other providers. All API calls target `https://api.deepseek.com/beta`.
   Supported models: `deepseek-v4-pro`, `deepseek-v4-flash`, `deepseek-r1`, `deepseek-v3`.
3. **Pure CLI** — No TUI, no web UI, no GUI. stdin/stdout only. Rustyline for input editing.
4. **Mobile-optimized** — Auto-detect narrow terminals (≤80 columns). Adapt rendering:
   character-level line wrapping, simplified output, residual character clearing.
5. **Minimal dependencies** — Prefer Rust standard library. Avoid heavy crates.
   Current key deps: `clap`, `reqwest` (rustls), `tokio`, `serde`, `rustyline`.
6. **Agentic by default** — Chat mode always enables tools. The model decides when to invoke them.
   Agent loop supports up to 15 consecutive tool-call rounds before yielding.

---

## Architecture

```
crates/dscode-cli/src/
├── api.rs              ← Shared API layer (DeepSeek HTTP, SSE streaming, tool defs & execution)
├── lib.rs              ← CLI routing (clap subcommand enum → dispatch)
├── main.rs             ← Entry point (3 lines: calls lib::run)
├── bin/dsc.rs          ← `dsc` short alias entry
└── commands/
    ├── chat.rs         ← Interactive chat with agent loop (max 15 rounds)
    ├── run.rs          ← One-shot prompt, print response, exit
    ├── auth.rs         ← API key management (login/status/test)
    ├── config.rs       ← Interactive config wizard + show
    ├── session.rs      ← Session list/show/rename/delete (JSON file store)
    ├── model.rs        ← Model list/info
    ├── tools.rs        ← Tool (list/enable/disable)
    └── completion.rs   ← Shell completion generation (bash/zsh/fish)
```

### Data Flow

```
User Input → lib.rs (route) → chat.rs (agent loop)
                                  ├─→ api.rs::call_stream() → DeepSeek API
                                  ├─→ api.rs::execute_tool() → local execution
                                  └─→ api.rs::render_markdown() → ANSI terminal output
```

### API Layer (`api.rs` — ~900 lines)

- `call_stream()` — POST to `/beta/chat/completions`, parse SSE `data:` lines
- `tool_definitions()` — Return JSON schemas for all 12 tools
- `execute_tool()` — Dispatch tool calls to local implementations
- `render_markdown()` — Convert Markdown spans to ANSI escape sequences

---

## Build & Test

```bash
# Release build
cargo build --release -p dscode

# Check without building
cargo check -p dscode

# Run unit tests
cargo test -p dscode

# Run with verbose output
RUST_LOG=debug dscode chat

# Lint
cargo clippy -- -D warnings
```

### Platform Targets

| Target | Binary Name |
|--------|-------------|
| `aarch64-apple-darwin` | `dscode-aarch64-apple-darwin` |
| `x86_64-apple-darwin` | `dscode-x86_64-apple-darwin` |
| `aarch64-unknown-linux-gnu` | `dscode-aarch64-unknowninux-gnu` |
| `x86_64-unknown-linux-gnu` | `dscode-x86_64-unknown-linux-gnu` |

CI auto-builds all targets on tag push (`.github/workflows/ci.yml`).

---

## Code Conventions

### File Organization
- Keep files under 500 lines. Extract shared logic to `api.rs`.
- One command = one file under `commands/`.
- All DeepSeek API calls go through `crate::api::*`.

### Quality Standards
- **Zero compiler warnings** — treat warnings as errors in CI.
- **No bare unwrap()** — use `.context()?` or proper error handling.
- **Every logical change** gets its own git commit.

### Commit Message Format
```
type: description
```
Types: `feat `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `style`

### Naming
- snake_case for functions/variables, CamelCase for types, SCREAMING_SNAKE for constants
- Command modules match CLI subcommand names exactly

---

## Tools (Agent Mode)

### Available Tools (12)

| # | Tool | Function | Implementation |
|---|------|----------|----------------|
| 1 | `read_file` | Read file contents | `std::fs::read_to_string` |
| 2 | `write_file` | Create or overwrite a file | `std::fs::write` with parent dir creation |
| 3 | `edit_file` | Surgical text replacement | Read → exact-match replace → write |
| 4 | `run_shell` | Execute shell command | `std::process::Command`, dangerous-cmd blocklist |
| 5 | `search_code` | grep codebase | `grep -rn` subprocess |
| 6 | `list_files` | List directory contents | `std::fs::read_dir` |
| 7 | `git_status | Working tree status | `git status --short` |
| 8 | `git_diff` | Unstaged changes diff | `git diff` |
| 9 | `git_commit` Stage all + commit | `git add -A && git commit -m` |
| 10 | `git_log` | Recent commit history | `git log --oneline -n <count>` |
| 11 | `web_search` | Web search | DuckDuckGo HTML scraping |
| 12 | `fetch_url` | HTTP GET | `reqwest::blocking::Client` |

### a New Tool

1. **Define schema** — Add JSON object in `api.rs::tool_definitions()` with `name`, `description`, and `parameters` (JSON Schema format).
2. **Add handler** — Add match arm in `api.rs::execute_tool()` that maps tool name to implementation.
3. **Test manually** — Run `dscode chat` and verify the model can discover and use the new tool.
4. **Update AGENT.md** — Add to the table above.

### Safety Constraints

- `run_shell` blocks dangerous commands: `rm -rf /`, `dd`, `mkfs`, `format`, fork bombs (`:(){ :|:& };:`).
- `edit_file` validates that the `old` string exists exactly once in the target file before replacing.
- `write_file` creates parent directories automatically; refuses to overwrite without explicit intent.

---

## Configuration

Config file: `~/.config/dscode/config.toml`

```toml
api_key = "sk-..."

[providers.deepseek]
model = "deepseek-v4-pro"
base_url = "https://api.deepseek.com/beta"
```

Environment variable fallback: `DEEPSEEK_API_KEY`

---

## Session Storage

Sessions are persisted as JSON files under `~/.local/share/dscode/sessions/`.
Each session file contains the full message history, model info, and metadata (created/updated timestamps, title).

```bash
dscode session list              # List all sessions
dscode session show <id>         # View session details
dscode session rename <id> <n>   # Rename session
dscode session delete <id>       # Delete session
```

---

## Release Process

See [RELEASE.md](./RELEASE.md) for the full checklist.

Quick reference:
```bash
git tag v0.1.0 && git push origin v0.1.0   # Triggers CI build + GitHub Release
```

---

## Links

- **Website:** <https://dscode.org>
- **Repository:** <https://github/aihop/dscode>
- **Engine:** dscode (this project)
- **DeepSeek API:** <https://api.deepseek.com/beta>
