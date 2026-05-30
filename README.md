# dscode

> Mobile-first AI coding agent, powered by DeepSeek.

**dscode** is a terminal-native AI coding agent built on the codewhale engine.
It connects directly to DeepSeek's API and works entirely through the command line —
zero web UI, zero bloat. Optimized for **SSH from your phone** and **Termux on Android**.

```bash
# One-line install
curl -fsSL https://dscode.org/install.sh | sh

# Set your key and start chatting
dscode auth login
dscode chat
```

---

## Features

### Agent Intelligence
- **30+ tools** — file I/O, shell, git (read/write), code search, web search, URL fetch, patch apply, code review, FIM edit, sub-agents, checklist, test runner, memory.
- **7 sub-agent roles** — `explore` (read-only research), `plan` (design + checklist), `architect` (high-level design), `coder` (implement), `reviewer` (audit), `tester` (write tests), `verifier` (run validation). Each role has a tailored system prompt and tool permissions — read-only roles cannot write files.
- **Plan mode** — switch to `/mode plan` for read-only research before writing code. The model can only read, search, and create a checklist. Switch back with `/mode agent` to execute.
- **Auto-verification** — after every edit, dscode auto-runs syntax checking: `cargo check` for Rust, `python -m py_compile` for Python, `node --check` for JS, `tsc --noEmit` for TS, `go vet` for Go, `gcc -fsyntax-only` for C/C++. Results are tagged with `[VERIFY PASS]` or `[VERIFY FAIL]`.
- **Cross-session memory** — tell the model your conventions once with `remember(key, value)`. It persists across sessions and is auto-injected into every new conversation. Query with `recall(query)`.
- **JSON repair** — streaming tool call arguments truncated by mobile network interruptions are automatically repaired (missing brackets, braces, and quotes).

### Mobile-First
- **Pure CLI** — no TUI, no web UI. Just stdin/stdout. Works over SSH with zero latency.
- **Narrow terminal** — auto-detects ≤80 columns, word-wraps output, minimal prompting.
- **Zero system dependencies** — only needs `git`. No `grep`, `curl`, or other system commands required. Built-in search uses the `ignore` crate (Rust-native, gitignore-aware).
- **API resilience** — auto-retry with exponential backoff (3 attempts, 500ms/1s) for unstable mobile networks.
- **Single binary** — ~3.4MB, statically linked, ARM-ready.

### Engineering
- **Streaming Markdown** — full Markdown → ANSI rendering (headings, bold, code blocks with syntax highlighting, lists, tables, blockquotes).
- **Session persistence** — SQLite-backed, save/resume/list/export conversations.
- **Model fallback** — auto-retry with `deepseek-v4-flash` when `deepseek-v4-pro` fails.
- **Reasoning effort** — defaults to `medium` thinking for deeper analysis. Override with `--think low/high`.
- **Command safety** — policy engine blocks destructive commands (`rm -rf /`, `dd`, `mkfs`, etc.).
- **Approval mode** — `--approve` prompts before writing files or running shell commands.

## Quickstart

### 1. Install

```bash
curl -fsSL https://dscode.org/install.sh | sh
```

Or build from source:

```bash
git clone --recursive https://github.com/aihop/dscode.git
cd dscode
cargo build --release -p dscode
cp target/release/dscode ~/.local/bin/
```

### 2. Authenticate

```bash
dscode auth login
# Enter your DeepSeek API key: sk-...
```

Or set the environment variable:

```bash
export DEEPSEEK_API_KEY=sk-your-key-here
```

### 3. Chat

```bash
# Interactive mode (default) — agent with tools
dscode chat

# Single prompt
dscode run "write a fibonacci function in rust"

# Use Flash model for faster responses
dscode chat -m deepseek-v4-flash

# Start in Plan mode (read-only research)
dscode chat --plain

# Enable approval mode
dscode chat --approve
```

### 4. Inline commands

```text
/mode plan      Switch to read-only research mode
/mode agent     Switch back to full tool mode
/clear          Clear conversation history
/save           Save session immediately
/exit           Quit
```

## Commands

| Command | Description |
| ------- | ----------- |
| `dscode chat` | Interactive chat with DeepSeek (agent mode) |
| `dscode chat --plain` | Interactive chat without tools |
| `dscode chat -s <id>` | Resume a specific session |
| `dscode run <prompt>` | Single prompt, print response |
| `dscode auth login` | Set API key (hidden input) |
| `dscode auth test` | Verify API key is valid |
| `dscode auth status` | Check authentication status |
| `dscode config init` | Interactive setup wizard |
| `dscode config show` | View current configuration |
| `dscode session list` | List saved sessions |
| `dscode session show <id>` | Show session details |
| `dscode session rename <id> <name>` | Rename a session |
| `dscode session delete <id>` | Delete a session |
| `dscode session export <id>` | Export session as JSON |
| `dscode tools list` | List available agent tools |
| `dscode model` | List available models |
| `dscode completion bash` | Generate shell completions |

## Agent Tools

dscode includes 30+ built-in tools. List them anytime: `dscode tools list`

```
read_file           Read file contents
write_file          Create or overwrite files
edit_file           Surgical text replacement in files
apply_patch         Apply unified-diff patches
run_shell           Execute shell commands (blocks destructive)
search_code         Search project files with regex
search_symbols      Find definitions (functions, classes, structs, traits)
file_search         Fuzzy filename search
web_search          Search the web (DuckDuckGo)
fetch_url           HTTP GET a URL
list_files          List directory contents
list_tree           Tree view of directory structure
get_file_info       File metadata + preview
git_log             Commit history
git_show            Commit details with diff
git_blame           Who changed each line
git_status          Working tree status
git_diff            Working tree diff
git_add             Stage files
git_commit          Create a commit
git_push            Push to remote
review              Code review (file, diff, or staged)
fim_edit            Fill-in-the-Middle edit via DeepSeek FIM API
agent_open          Spawn a sub-agent with a role (explore/plan/architect/coder/...)
agent_eval          Check sub-agent status
agent_close         Close sub-agent
remember            Store a fact for future sessions
recall              Query stored memory
checklist_write     Create task checklist
checklist_add       Add checklist item
checklist_update    Update item status
checklist_list      List checklist items
test_runner         Run tests and report results
request_user_input  Ask the user for input mid-task
```

## Sub-Agent Roles

When spawning a sub-agent, specify a role to match the task:

```
agent_open(prompt="map the module structure", role="explore")
  → read-only research, returns path:line evidence

agent_open(prompt="design the migration plan", role="plan")
  → design + checklist, no implementation

agent_open(prompt="implement the parser", role="coder")
  → full tool access, writes code

agent_open(prompt="review the diff", role="reviewer")
  → read-only audit, grades with severity

agent_open(prompt="run tests and report", role="verifier")
  → validation only, doesn't fix failures
```

## Session Management

Sessions are automatically saved to `~/.local/share/dscode/state.db` (SQLite).

```bash
# List all sessions (most recent first)
dscode session list

# Show session details and messages
dscode session show abc12345

# Resume a session
dscode chat -s abc12345

# Rename for easy identification
dscode session rename abc12345 "my-fix-branch"

# Export as JSON
dscode session export abc12345 > backup.json

# Delete
dscode session delete abc12345
```

## Mobile Usage

### On Android (Termux)

```bash
pkg install curl git
curl -fsSL https://dscode.org/install.sh | sh
dscode auth login
dscode chat
```

### Via SSH

```bash
ssh user@your-server
dscode chat
```

The CLI adapts to your terminal width — no horizontal scrolling on narrow phone screens. Network issues are handled with automatic retry.

## Configuration

Config lives at `~/.dscode/config.toml`:

```toml
api_key = "sk-..."

[providers.deepseek]
model = "deepseek-v4-pro"
base_url = "https://api.deepseek.com/beta"
```

Memory file: `~/.dscode/memory.md`

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                  dscode CLI (~5,200 LOC)              │
│                                                       │
│  chat.rs ──agent loop──► api.rs ──SSE──► DeepSeek    │
│    │  ▲                      │                       │
│    │  │                      ▼                       │
│    │  │              engine.rs (auto-check           │
│    │  │               + verify gates)                │
│    │  │                      │                       │
│    │  └── tools/ ────────────┘                       │
│    │      file | git | search | agent                │
│    │                                                 │
│    └── session.rs ◄── state.db ──► codewhale-state   │
│                                                       │
│  External deps: git (only)                            │
│  Rust crates: codewhale-tools, codewhale-config,     │
│               codewhale-state, codewhale-execpolicy, │
│               codewhale-protocol, codewhale-agent     │
└──────────────────────────────────────────────────────┘
```

dscode is built on the **codewhale engine** — reusing its tool framework, config system, SQLite store, and command safety policy — while keeping a lightweight, mobile-first CLI surface.

## Links

- Repository: https://github.com/aihop/dscode
- API: https://api.deepseek.com/beta
- License: MIT
