# dscode

> Mobile-first AI agent, powered by DeepSeek.

**dscode** is a terminal-native AI coding agent.  
It connects directly to DeepSeek's API and works entirely through the command line —  
perfect for **SSH from your phone** or **Termux on Android**.

```bash
# One-line install
curl -fsSL https://dscode.org/install.sh | sh

# Set your key and start chatting
dscode auth login
dscode chat
```

---

## Features

- **Pure CLI** — no web UI, no TUI, no bloat. Just stdin/stdout.
- **Agent with 13 tools** — read/write/edit files, git history, shell execution, code search, file search, web search, patch apply, URL fetch.
- **Streaming Markdown** — see output token by token with full Markdown → ANSI rendering (headings, bold, code blocks, lists, syntax highlighting).
- **Mobile-optimized** — auto-detects narrow terminals (≤80 columns), word-wrap, minimal output.
- **Session persistence** — SQLite-backed, save/resume/list/export conversations.
- **Model fallback** — auto-retry with `deepseek-v4-flash` when `deepseek-v4-pro` fails.
- **Command safety** — policy engine blocks destructive commands (`rm -rf /`, `dd`, `mkfs`, etc.).
- **Single binary** — ~3.4MB, statically linked, ARM-ready.

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

# Plain chat mode (no tools)
dscode chat --plain
```

## Commands

| Command | Description |
|---------|-------------|
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

## Agent Tools

dscode includes 27 built-in tools for autonomous code work:

```
read_file      Read file contents
write_file     Create or overwrite files
edit_file      Surgical text replacement in files
run_shell      Execute shell commands (blocked: destructive)
search_code    Grep for regex patterns in the project
list_files     List directory contents
web_search     Search the web via DuckDuckGo
fetch_url      HTTP GET a URL
git_log        Show commit history (optional path/max_count)
git_show       Show commit details with diff
git_blame      Show who last modified each line of a file
file_search    Fuzzy filename search in the project
apply_patch    Apply unified-diff patches to the working tree
git_diff       Show working tree diff (unstaged or staged)
git_add        Stage files for commit
git_commit     Create a commit with a message
git_push       Push commits to remote repository
review         Code review (file, diff, or staged changes)
fim_edit       Fill-in-the-Middle edit via DeepSeek FIM API
agent_open     Spawn a sub-agent for background work
agent_eval     Check sub-agent status and results
agent_close    Close sub-agent and get final results
checklist_write  Create task checklist
checklist_add  Add checklist item
checklist_update  Update item status
checklist_list  List all checklist items
test_runner    Run tests and report results
```

List them anytime: `dscode tools list`

## Mobile Usage

### On Android (Termux)

```bash
pkg install curl
curl -fsSL https://dscode.org/install.sh | sh
dscode auth login
dscode chat
```

### Via SSH

```bash
ssh user@your-server
dscode chat
```

The CLI auto-detects terminal width and adapts output —  
no horizontal scrolling on narrow phone screens.

## Configuration

Config lives at `~/.config/dscode/config.toml`:

```toml
api_key = "sk-..."

[providers.deepseek]
model = "deepseek-v4-pro"
base_url = "https://api.deepseek.com/beta"
```

## Architecture

```
┌──────────────────────────────────────────────────┐
│                   dscode CLI                      │
│                                                   │
│  chat.rs ──agent loop──► api.rs ──SSE──► DeepSeek│
│    │  ▲                      │                   │
│    │  │                      ▼                   │
│    │  │                 tools.rs                 │
│    │  │            (8 tools via ToolRegistry)     │
│    │  │                   │                      │
│    ▼  │                   ▼                      │
│  session.rs ◄── state.db ──► codewhale-state     │
│    (SQLite)                 (SQLite)              │
│                                                   │
│  ┌─ Codewhale engine ────────────────────────┐   │
│  │  codewhale-tools     Tool framework       │   │
│  │  codewhale-config    Config management    │   │
│  │  codewhale-state     SQLite persistence   │   │
│  │  codewhale-execpolicy Shell safety        │   │
│  │  codewhale-protocol  Protocol types       │   │
│  └───────────────────────────────────────────┘   │
└──────────────────────────────────────────────────┘
```

dscode is built on the **codewhale engine** — reusing its tool framework, config system, SQLite store, and command safety policy — while keeping a lightweight, mobile-first CLI surface.

## License

MIT
