# dscode — AI Guide

## 🎯 Identity & Three Pillars

**dscode** is a coding agent with three non-negotiable priorities.
Every decision — architecture, output style, tool behavior — must serve these three:

### 1. 🔴 DeepSeek-Only (No Other Providers)

- **All** LLM API calls go exclusively to `https://api.deepseek.com/beta`.
- Supported models: `deepseek-v4-pro`, `deepseek-v4-flash`, `deepseek-r1`, `deepseek-v3`.
- Never, reference, or fall back to any other provider (OpenAI, Anthropic, Google, etc.).
- Use DeepSeek-specific features: `reasoning_content` echo-back, `reasoning_effort` param, beta endpoint.

### 2. 📱 Mobile CLI (Termux / iSH / SSH)

- The primary target is **narrow terminals on phones**: Termux (Android), iSH (iOS), SSH from mobile.
- Auto-detect narrow mode (≤80 columns). Render output character-by-character to avoid broken lines.
- Pure CLI: no TUI, no web UI, no GUI. stdin/stdout only. rustyline for input.
- Keep output terse. On narrow terminals, skip decorations and use compact one-liners.
- Pipe safety: when stdout is not a TTY, strip ANSI escape codes (use `std::io::IsTerminal`).

### 3. 📉 Token Budget Conscious

- **Token efficiency is a feature, not an afterthought.**
- Prefer short, direct answers. Avoid verbose explanations, preambles, and summaries unless asked.
- When reading files, use targeted reads (by line range) rather than slurping entire files.
- When searching, limit results (grep `-m`, `head`). Avoid returning massive dumps.
- Tool output truncation: cap at reasonable limits. Prefer structured truncation (keep head + tail).
- Code snippets: show the minimal diff, not the entire file.
- For simple tasks (git status, list files), use `deepseek-v4-flash` (cheaper). For complex reasoning, use `deepseek-v4-pro` or `deepseek-r1`.
- Think before adding new system prompt content: every 100 characters ≈ 25-30 tokens per request.

---

## Project Overview

- **Language:** Rust 2021 edition (MSRV 1.75)
- **Binary:** ~7.5MB statically linked, zero runtime dependencies
- **License:** MIT
- **Repository:** <https://github.com/aihop/dscode>

---

## Core Principles

1. **dscode first** — Reuse `codewhale-*` crates from crates.io. Never reimplement what the engine already provides: config, state, agent orchestration, tools, MCP.
2. **DeepSeek-only** — See Pillar 1 above. No exceptions.
3. **Pure CLI** — See Pillar 2 above. No TUI/GUI.
4. **Mobile-optimized** — See Pillar 2 above. Narrow terminal rendering.
5. **Token-efficient** — See Pillar 3 above. Every token counts.
6. **Agentic by default** — Chat mode always enables tools. The model decides when to invoke them. Agent loop supports up to 15 consecutive tool-call rounds before yielding.

---

## Architecture

```
crates/dscode-cli/src/
├── api.rs              ← Shared API layer (DeepSeek HTTP, SSE streaming, tool defs & execution)
├── lib.rs              ← CLI routing (clap subcommand enum → dispatch)
├── main.rs             Entry point (3 lines: calls lib::run)
├── bin/dsc.rs          ← `dsc` short alias entry
└── commands/
    ├── chat.rs         ← Interactive chat with agent loop (max 15 rounds)
    ├── run.rs          ← One-shot prompt, print response, exit
    ├── auth.rs         ← API key management (login/status/test)
    ├── config.rs       ← Interactive config wizard + show
    ├── session.rs      ← Session list/show/rename/delete (SQLite)
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

### API Layer (`api.rs`)

- `call_stream()` — POST to `/beta/chat/completions`, parse SSE `data:` lines
- `tool_definitions()` — Return JSON schemas for all tools
- `execute_tool()` — Dispatch tool calls to local implementations
- `render_markdown()` — Convert Markdown spans to ANSI escape sequences

---

## Build & Test

```bash
cargo build --release -p dscode   # Release build
cargo checkp dscode             # Check without building
cargo test -p dscode              # Run unit tests
RUST_LOG=debug dscode chat        # Run with verbose output
```

### Platform Targets

| Target | Binary Name |
|--------|-------------|
| `aarch64-apple-darwin` | `dscode-aarch64-apple-darwin` |
| `x86_64-apple-darwin` | `dscode-x86_64-apple-darwin` |
| `aarch64-unknown-linux-gnu` | `dscode-aarch64-unknown-linux-gnu` |
| `x86_64-unknown-linux-gnu` | `dscode-x86_64-unknown-linux-gnu` |

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
Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `style`

 Naming
- snake_case for functions/variables, CamelCase for types, SCREAMING_SNA for constants
- Command modules match CLI subcommand names exactly

---

## Tools (Agent Mode)

### Available Tools

| # | Tool | Function | Implementation |
|---|----------------|----------------|
| 1 | `read_file` | Read file contents | `std::fs::read_to_string` |
| 2 | `write_file` | Create or overwrite a | `std::fs::write` with parent dir creation |
| 3 | `edit_file` | Surgical text replacement | Read → exact-match replace → write |
| 4 | `run_shell` | Execute shell command | `std::process::Command`, dangerous-cmd blocklist |
| 5 | `search_code` | grep codebase | `grep -rn` subprocess |
| 6 | `list_files` | List directory contents | `std::fs::read_dir` |
| 7 | `web_search` | Web search | DuckDuckGo HTML scraping |
| 8 | `fetch_url` | HTTP GET | `reqwest::blocking::Client` |

### Adding a New Tool

1. **Define schema** — Add JSON object in `api.rs::tool_definitions()` with `name`, `description`, and `parameters` (JSON Schema format).
2. **Add handler** — Add match arm in `api.rs::execute_tool()` that maps tool name to implementation.
3. **Test manually** — Run `dscode chat` and verify the model can discover and use the new tool.
4. **Update AGENT.md** — Add the table above.

### Safety Constraints

- `run_shell` blocks dangerous commands: `rm -rf`, `dd`, `mkfs`, `format`, fork bombs.
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

Sessions are persisted via SQLite under `~/.local/share/dscode/`.
Each session contains full message history, model info, and metadata.

```bash
dscode session list              # List all sessions
dscode session show <id>         # View session details
dscode session rename <id> <>   # Rename session
dscode session delete <id>       # Delete session
```

---

## Release Process

See [RELEASE.md](./RELEASE.md) for the full checklist.

```bash
git tag v0.1.0 && git push origin v0.1.0   # Triggers CI build + GitHub Release
```

---

## Links

- **Website:** <https://dscode.org>
- **Repository:** <https://github.com/aihop/dscode>
- **DeepSeek API:** <https://api.deepseek.com/beta>
