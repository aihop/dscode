/// File I/O and patch-execution tools.
///
/// Pure sync helpers consumed by the DscHandler dispatcher in mod.rs.

use crate::tools::{cwd_join, ToolCtx};
use serde_json::Value;

// ── Read file ─────────────────────────────────────────────────

pub(crate) fn exec_read_file(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let total = lines.len();
            let start_line = v["start_line"].as_u64().unwrap_or(1).max(1) as usize - 1;
            let start_line = start_line.min(total.saturating_sub(1));
            let max_lines = v["max_lines"].as_u64().unwrap_or(500).min(2_000) as usize;
            let end_line = (start_line + max_lines).min(total);
            let truncated = end_line < total;
            let shown: Vec<&str> = lines[start_line..end_line].to_vec();
            let mut out = format!(
                "<file path=\"{}\" total_lines=\"{}\" start_line=\"{}\" end_line=\"{}\"",
                full_path.display(),
                total,
                start_line + 1,
                end_line
            );
            if truncated {
                out.push_str(" truncated=\"true\"");
            }
            out.push_str(">\n");
            for (i, line) in shown.iter().enumerate() {
                out.push_str(&format!("{:>6}  {}\n", start_line + i + 1, line));
            }
            if truncated {
                out.push_str(&format!("... ({} more lines)\n", total - end_line));
            }
            out.push_str("</file>");
            out
        }
        Err(e) => format!("<error reading {}: {e}>", full_path.display()),
    }
}

// ── Write file ────────────────────────────────────────────────

pub(crate) fn exec_write_file(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    let content = v["content"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    match std::fs::write(&full_path, content) {
        Ok(_) => {
            let diff = diff_preview(ctx, path_str);
            if !diff.is_empty() {
                format!("written {} ({} bytes)\n{}", full_path.display(), content.len(), diff)
            } else {
                format!("written {} ({} bytes)", full_path.display(), content.len())
            }
        }
        Err(e) => format!("error writing {}: {e}", full_path.display()),
    }
}

// ── Edit file ─────────────────────────────────────────────────

pub(crate) fn exec_edit_file(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    let old = v["old"].as_str().unwrap_or("");
    let new = v["new"].as_str().unwrap_or("");
    let line_hint = v["line"].as_u64();
    if path_str.is_empty() {
        return "error: no path".to_string();
    }
    if old.is_empty() {
        return "error: no old text provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    match std::fs::read_to_string(&full_path) {
        Ok(ref content) => {
            let lines: Vec<&str> = content.lines().collect();
            let match_result = find_edit_match(content, &lines, old, line_hint);
            match match_result {
                Err(EditMatchError::NotFound) => {
                    let snippet = if let Some(ln) = line_hint {
                        let idx = (ln as usize).saturating_sub(1).min(lines.len().saturating_sub(1));
                        let start = idx.saturating_sub(3);
                        let end = (idx + 3).min(lines.len());
                        let snip: Vec<String> = lines[start..end].iter().enumerate().map(|(i, l)| {
                            format!("{:>6}  {}", start + i + 1, l)
                        }).collect();
                        format!(" near line {}. Lines around it:\n{}", ln, snip.join("\n"))
                    } else {
                        String::new()
                    };
                    format!("error: match not found in {}{}", full_path.display(), snippet)
                }
                Err(EditMatchError::Ambiguous(count)) => {
                    if let Some(ln) = line_hint {
                        format!("error: matched text appears {count} times near line {ln} — include more context lines")
                    } else {
                        format!("error: matched text appears {count} times — include more context lines or provide a line hint")
                    }
                }
                Ok((pos, match_len)) => {
                    let new_content = format!("{}{}{}", &content[..pos], new, &content[pos + match_len..]);
                    match std::fs::write(&full_path, &new_content) {
                        Ok(_) => {
                            let diff = diff_preview(ctx, path_str);
                            if !diff.is_empty() {
                                format!("edited {}\n{}", full_path.display(), diff)
                            } else {
                                format!("edited {}", full_path.display())
                            }
                        }
                        Err(e) => format!("error writing {}: {e}", full_path.display()),
                    }
                }
            }
        }
        Err(e) => format!("error reading {}: {e}", full_path.display()),
    }
}

// ── Edit matching helpers ─────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum EditMatchError {
    NotFound,
    Ambiguous(usize),
}

fn find_edit_match(
    content: &str,
    lines: &[&str],
    old: &str,
    line_hint: Option<u64>,
) -> Result<(usize, usize), EditMatchError> {
    let exact_matches = find_exact_matches(content, old);
    match select_edit_match(lines, &exact_matches, line_hint) {
        Ok(m) => return Ok(m),
        Err(EditMatchError::Ambiguous(_)) => return Err(EditMatchError::Ambiguous(exact_matches.len())),
        Err(EditMatchError::NotFound) => {}
    }
    let fuzzy_matches = find_fuzzy_matches(content, lines, old, line_hint);
    select_edit_match(lines, &fuzzy_matches, line_hint)
}

fn find_exact_matches(content: &str, old: &str) -> Vec<(usize, usize)> {
    content.match_indices(old).map(|(pos, s)| (pos, s.len())).collect()
}

fn find_fuzzy_matches(
    content: &str,
    lines: &[&str],
    old: &str,
    line_hint: Option<u64>,
) -> Vec<(usize, usize)> {
    let old_lines: Vec<&str> = old.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if old_lines.is_empty() {
        return Vec::new();
    }
    let content_trimmed: Vec<&str> = lines.iter().map(|l| l.trim()).collect();
    let line_offsets: Vec<usize> = lines.iter().scan(0usize, |acc, l| {
        let o = *acc;
        *acc += l.len() + 1;
        Some(o)
    }).collect();

    let search_lines = if let Some(ln) = line_hint {
        let start = (ln as usize).saturating_sub(20).min(lines.len());
        let end = ((ln as usize) + 20).min(lines.len());
        start..end
    } else {
        0..lines.len()
    };

    let max_start = search_lines.end.saturating_sub(old_lines.len().saturating_sub(1));
    let mut matches = Vec::new();
    for start in search_lines.start..max_start {
        let mut all_match = true;
        for (i, old_line) in old_lines.iter().enumerate() {
            let file_line = content_trimmed.get(start + i).unwrap_or(&"");
            let old_norm: String = old_line.split_whitespace().collect::<Vec<_>>().join(" ");
            let file_norm: String = file_line.split_whitespace().collect::<Vec<_>>().join(" ");
            if old_norm != file_norm {
                all_match = false;
                break;
            }
        }
        if all_match {
            let byte_start = line_offsets[start];
            let byte_end = line_offsets
                .get(start + old_lines.len())
                .copied()
                .unwrap_or(content.len());
            matches.push((byte_start, byte_end.saturating_sub(byte_start)));
        }
    }
    matches
}

fn select_edit_match(
    lines: &[&str],
    matches: &[(usize, usize)],
    line_hint: Option<u64>,
) -> Result<(usize, usize), EditMatchError> {
    match matches.len() {
        0 => Err(EditMatchError::NotFound),
        1 => Ok(matches[0]),
        _ => {
            if let Some(ln) = line_hint {
                let line_offsets: Vec<usize> = lines.iter().scan(0usize, |acc, l| {
                    let o = *acc;
                    *acc += l.len() + 1;
                    Some(o)
                }).collect();
                let target_line = ln as usize;
                let best = matches
                    .iter()
                    .min_by_key(|(pos, _)| {
                        let line_no = byte_offset_to_line(&line_offsets, *pos);
                        line_no.abs_diff(target_line)
                    })
                    .copied()
                    .unwrap_or(matches[0]);
                Ok(best)
            } else {
                Err(EditMatchError::Ambiguous(matches.len()))
            }
        }
    }
}

fn byte_offset_to_line(line_offsets: &[usize], pos: usize) -> usize {
    match line_offsets.binary_search(&pos) {
        Ok(idx) => idx + 1,
        Err(idx) => idx.max(1),
    }
}

// ── List files ────────────────────────────────────────────────

pub(crate) fn exec_list_files(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let path_str = v["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    match std::fs::read_dir(&full_path) {
        Ok(entries) => {
            let mut items: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ty = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        "dir"
                    } else {
                        "file"
                    };
                    format!("  {ty:4}  {name}")
                })
                .collect();
            items.sort();
            format!("{} ({} entries):\n{}", full_path.display(), items.len(), items.join("\n"))
        }
        Err(e) => format!("error listing {}: {e}", full_path.display()),
    }
}

// ── Apply patch ───────────────────────────────────────────────

pub(crate) fn exec_apply_patch(ctx: &ToolCtx, args: &str) -> String {
    let v: Value = serde_json::from_str(args).unwrap_or_default();
    let patch = v["patch"].as_str().unwrap_or("");
    if patch.is_empty() {
        return "error: no patch provided".to_string();
    }
    let tmp = std::env::temp_dir().join(format!("dscode-patch-{}.diff", std::process::id()));
    let write_ok = std::fs::write(&tmp, patch).is_ok();
    if !write_ok {
        return "error: could not write patch file".to_string();
    }
    let result = std::process::Command::new("git")
        .args(["apply", &tmp.to_string_lossy()])
        .current_dir(&ctx.cwd)
        .output();
    let _ = std::fs::remove_file(&tmp);
    match result {
        Ok(o) if o.status.success() => "patch applied successfully".to_string(),
        Ok(o) => format!("patch failed:\n{}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => format!("git apply error: {e}"),
    }
}

// ── Diff preview helper ───────────────────────────────────────

fn diff_preview(ctx: &ToolCtx, path_str: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["diff", "--", path_str])
        .current_dir(&ctx.cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if out.is_empty() { String::new() } else { out }
        }
        _ => String::new(),
    }
}
