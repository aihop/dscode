# Release Checklist

## 1. Push to GitHub

```bash
git remote add origin git@github.com:Hmbown/dscode.git
git push origin main
git push origin --tags
```

## 2. Create GitHub Release

Push tags triggers the CI workflow (`.github/workflows/ci.yml`):
- Builds for 4 platforms (macOS ARM/x86, Linux ARM/x86)
- Auto-creates a GitHub Release with binaries

To trigger: `git tag v0.1.0 && git push origin v0.1.0`

Or manually:
1. Go to https://github.com/Hmbown/dscode/releases
2. Click "Draft a new release"
3. Tag: `v0.1.0`, Title: `dscode v0.1.0`
4. Upload binaries from `target/release/dscode`

## 3. Set up dscode.org

### Option A: GitHub Pages
1. Push the `www/` folder to `gh-pages` branch
```bash
git subtree push --prefix www origin gh-pages
```
2. Go to repo Settings → Pages → Source: `gh-pages` branch
3. Set DNS: dscode.org CNAME → `hmbown.github.io`

### Option B: Vercel
1. Connect repo, set output dir to `www/`
2. Set custom domain dscode.org

## 4. Verify Installation

```bash
# Install via script
curl -fsSL https://dscode.org/install.sh | sh

# Or via cargo (after crates.io publish)
cargo install dscode

# Or build from source
git clone --recursive https://github.com/Hmbown/dscode.git
cd dscode && cargo build --release -p dscode
```

## 5. Smoke test

```bash
dscode --version
dscode model
dscode auth login
dscode chat -m flash
dscode run "say hello in python" -m flash
dscode session list
```

## Pre-built binary targets

| Target | Binary name |
|--------|-------------|
| macOS ARM64 | `dscode-aarch64-apple-darwin` |
| macOS x86_64 | `dscode-x86_64-apple-darwin` |
| Linux ARM64 | `dscode-aarch64-unknown-linux-gnu` |
| Linux x86_64 | `dscode-x86_64-unknown-linux-gnu` |

These are auto-built by CI on tag push.
