/// Git commands for repository management.

use crate::tools::{cwd_join, ToolCtx};
use serde_json::Value;

// ── Git read tools ────────────────────────────────────────────

pub(crate) fn exec_git_log(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let max_count = v["max_count"].as_u64().unwrap_or(20).min(100);
    let path = v["path"].as_str().unwrap_or("");
    let mut cmd = std::process::Command::new("git");
    cmd.args(["log", "--oneline", "-n", &max_count.to_string()]);
    if !path.is_empty() { cmd.arg(path); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => format!("git log failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git log error: {e}"),
    }
}

pub(crate) fn exec_git_show(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let rev = v["rev"].as_str().unwrap_or("");
    if rev.is_empty() { return "error: no rev provided".to_string(); }
    let output = std::process::Command::new("git")
        .args(["show", "--stat", "--patch", rev])
        .current_dir(&ctx.cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            if s.len() > 8000 { s = format!("{}... (truncated, {} total)", &s[..8000], s.len()); }
            s
        }
        Ok(o) => format!("git show failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git show error: {e}"),
    }
}

pub(crate) fn exec_git_blame(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    if path.is_empty() { return "error: no path provided".to_string(); }
    let full_path = cwd_join(ctx, path);
    let start = v["start_line"].as_u64().unwrap_or(1).max(1);
    let max_lines = v["max_lines"].as_u64().unwrap_or(200).min(1000);
    let end = start + max_lines - 1;
    match std::process::Command::new("git")
        .args(["blame", &format!("-L{start},{end}"), "--", &full_path.to_string_lossy()])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => format!("git blame failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git blame error: {e}"),
    }
}

// ── Git write tools ───────────────────────────────────────────

pub(crate) fn exec_git_status(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    let mut cmd = std::process::Command::new("git");
    cmd.args(["status", "--short", "--branch"]);
    if !path.is_empty() { cmd.arg(path); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("git status failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git status error: {e}"),
    }
}

pub(crate) fn exec_git_diff(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or("");
    let cached = v["cached"].as_bool().unwrap_or(false);
    let mut cmd = std::process::Command::new("git");
    cmd.arg("diff");
    if cached { cmd.arg("--cached"); }
    if !path.is_empty() { cmd.arg(path); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            if out.is_empty() { "no changes".to_string() } else { out }
        }
        Ok(o) => format!("git diff failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git diff error: {e}"),
    }
}

pub(crate) fn exec_git_add(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path = v["path"].as_str().unwrap_or(".");
    match std::process::Command::new("git")
        .args(["add", path])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => format!("staged {path}"),
        Ok(o) => format!("git add failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git add error: {e}"),
    }
}

pub(crate) fn exec_git_commit(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let msg = v["message"].as_str().unwrap_or("");
    if msg.is_empty() { return "error: no commit message".to_string(); }
    match std::process::Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(&ctx.cwd)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("commit failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("commit error: {e}"),
    }
}

pub(crate) fn exec_git_push(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let remote = v["remote"].as_str().unwrap_or("origin");
    let branch = v["branch"].as_str().unwrap_or("");
    let mut cmd = std::process::Command::new("git");
    cmd.args(["push", remote]);
    if !branch.is_empty() { cmd.arg(branch); }
    cmd.current_dir(&ctx.cwd);
    match cmd.output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if out.is_empty() { "pushed successfully".to_string() } else { out }
        }
        Ok(o) => format!("push failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("push error: {e}"),
    }
}
