# dscode

> Mobile-first AI agent, powered by DeepSeek.

**dscode** is a terminal-native AI coding agent built on the dscode engine.  
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
- **Streaming responses** — see output token by token.
- **Mobile-optimized** — auto-detects narrow terminals (≤80 columns).
- **Session management** — save, resume, export conversations.
- **DeepSeek-only** — zero configuration for other providers.
- **Single binary** — ~7.5MB, statically linked, ARM-ready.

## Quickstart

### 1. Install

```bash
curl -fsSL https://dscode.org/install.sh | sh
```

Or via `cargo install` (once published to crates.io):

```bash
cargo install dscode
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
# Interactive mode (default)
dscode chat

# Single prompt
dscode run "write a fibonacci function in rust"

# Use Flash model for faster responses
dscode chat -m deepseek-v4-flash
```

## Commands

| Command | Description |
|---------|-------------|
| `dscode chat` | Interactive chat with DeepSeek |
| `dscode run <prompt>` | Single prompt, print response |
| `dscode auth login` | Set API key (hidden input) |
| `dscode auth test` | Verify API key is valid |
| `dscode auth status` | Check authentication status |
| `dscode config init` | Interactive setup wizard |
| `dscode config show` | View configuration |
| `dscode session list` | List saved sessions |
| `dscode session rename <id> <name>` | Rename a session |
| `dscode model` | List available models |
| `dscode completion bash` | Generate shell completions |

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
dscode CLI ──► DeepSeek API ──► dscode-v4-pro / dscode-v4-flash
     │
     ├── dscode engine (agent + tools + policy)
     ├── SQLite session store
     └── Narrow-terminal renderer
```

Built on the dscode engine —  
DeepSeek AI coding agent for the terminal.

## License

MIT
