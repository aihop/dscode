/// File I/O and patch-execution tools.
///
/// Pure sync helpers consumed by the DscHandler dispatcher in mod.rs.

use crate::tools::{cwd_join, ToolCtx};
use serde_json::{json, Value};

/// Backup a file before destructive write. Returns the backup path or None.
/// Backups go to ~/.dscode/backups/ with timestamp and original filename.
pub(crate) fn backup_before_write(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if !path.exists() {
        return None;
    }
    let backup_dir = crate::utils::dscode_dir().join("backups");
    std::fs::create_dir_all(&backup_dir).ok()?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).ok()?
        .as_secs();
    let safe_name = path.to_string_lossy()
        .replace('/', "_")
        .replace('\\', "_");
    let bak_path = backup_dir.join(format!("{}.{}.bak", safe_name, ts));
    std::fs::copy(path, &bak_path).ok().map(|_| bak_path)
}

/// Parse JSON tool arguments, returning a clear error message on failure.
macro_rules! parse_args {
    ($args:expr) => {{
        let __args = $args;
        match serde_json::from_str::<Value>(__args) {
            Ok(v) => v,
            Err(e) => return format!("error: invalid JSON arguments: {e}"),
        }
    }};
}

// ── Read file ─────────────────────────────────────────────────

pub(crate) fn exec_read_file(ctx: &ToolCtx, args: &str) -> String {
    let v = parse_args!(args);
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
            // For large files, inject a symbol map to help the model navigate
            if total > 500 && start_line == 0 {
                let symbols = build_symbol_map(&lines);
                if !symbols.is_empty() {
                    out.push_str(&format!("\n## Symbols ({})\n{}", total, symbols));
                }
            }
            out
        }
        Err(e) => format!("<error reading {}: {e}>", full_path.display()),
    }
}

// ── Write file ────────────────────────────────────────────────

pub(crate) fn exec_write_file(ctx: &ToolCtx, args: &str) -> String {
    let v = parse_args!(args);
    let path_str = v["path"].as_str().unwrap_or("");
    let content = v["content"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let bak = backup_before_write(&full_path);
    match std::fs::write(&full_path, content) {
        Ok(_) => {
            let bak_info = bak.map(|b| format!(" [backup: {}]", b.display())).unwrap_or_default();
            let diff = diff_preview(ctx, path_str);
            if !diff.is_empty() {
                format!("written {} ({} bytes){}\n{}", full_path.display(), content.len(), bak_info, diff)
            } else {
                format!("written {} ({} bytes){}", full_path.display(), content.len(), bak_info)
            }
        }
        Err(e) => format!("error writing {}: {e}", full_path.display()),
    }
}

// ── Edit file ─────────────────────────────────────────────────

pub(crate) fn exec_edit_file(ctx: &ToolCtx, args: &str) -> String {
    let v = parse_args!(args);
    let path_str = v["path"].as_str().unwrap_or("");
    let old = v["old"].as_str().unwrap_or("");
    let new = v["new"].as_str().unwrap_or("");
    let target = v["target"].as_str();      // NEW: symbol name or line number
    let new_lines = v["new_lines"].as_str(); // NEW: replacement content for target-based edit
    let line_hint = v["line"].as_u64().or(v["start_line"].as_u64());
    if path_str.is_empty() {
        return "error: no path".to_string();
    }
    let full_path = cwd_join(ctx, path_str);

    // Auto-detect file size and pick the best edit strategy
    let line_count = std::fs::read_to_string(&full_path)
        .ok()
        .map(|c| c.lines().count())
        .unwrap_or(0);
    let is_small_file = line_count > 0 && line_count < 50;

    // Small file + content provided → auto switch to write_file (faster, no matching)
    if is_small_file && !new.is_empty() && target.is_none() {
        let bak = backup_before_write(&full_path);
        let bak_info = bak.map(|b| format!(" [backup: {}]", b.display())).unwrap_or_default();
        match std::fs::write(&full_path, new) {
            Ok(_) => {
                let diff = diff_preview(ctx, path_str);
                if !diff.is_empty() {
                    return format!("written {} (auto: small file, {} lines){}\n{}", full_path.display(), line_count, bak_info, diff);
                } else {
                    return format!("written {} (auto: small file, {} lines){}", full_path.display(), line_count, bak_info);
                }
            }
            Err(e) => return format!("error writing {}: {e}", full_path.display()),
        }
    }

    // New fast path: target-based editing (no old text needed)
    if let Some(tgt) = target {
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => return format!("error reading {}: {e}", full_path.display()),
        };
        let lines: Vec<&str> = content.lines().collect();
        let range = if let Ok(ln) = tgt.parse::<usize>() {
            // Target is a line number
            let start = ln.saturating_sub(1).min(lines.len());
            (start, start + 1)
        } else {
            // Target is a symbol name
            resolve_symbol_range(&lines, tgt)
                .unwrap_or_else(|| {
                    // Fallback: text search
                    for (i, line) in lines.iter().enumerate() {
                        if line.contains(tgt) {
                            let start = i.saturating_sub(1);
                            return (start, (i + 2).min(lines.len()));
                        }
                    }
                    (0, lines.len())
                })
        };
        let replacement = new_lines.unwrap_or(new);
        let result = format!(
            "{}{}{}",
            lines[..range.0].join("\n"),
            if range.0 > 0 { "\n" } else { "" },
            replacement,
        );
        let result = if range.1 < lines.len() {
            format!("{}\n{}", result, lines[range.1..].join("\n"))
        } else {
            result
        };
        let bak = backup_before_write(&full_path);
        let bak_info = bak.map(|b| format!(" [backup: {}]", b.display())).unwrap_or_default();
        match std::fs::write(&full_path, &result) {
            Ok(_) => {
                let diff = diff_preview(ctx, path_str);
                if !diff.is_empty() {
                    return format!("edited {} (target={}, lines {}-{}){}\n{}",
                        full_path.display(), tgt, range.0 + 1, range.1, bak_info, diff);
                } else {
                    return format!("edited {} (target={}, lines {}-{}){}",
                        full_path.display(), tgt, range.0 + 1, range.1, bak_info);
                }
            }
            Err(e) => return format!("error writing {}: {e}", full_path.display()),
        }
    }

    // Legacy path: old + new based editing
    if old.is_empty() && line_hint.is_none() {
        return "error: provide old+new, or target+new_lines, or a line hint".to_string();
    }
    match std::fs::read_to_string(&full_path) {
        Ok(ref content) => {
            let lines: Vec<&str> = content.lines().collect();
            let match_result = find_edit_match(content, &lines, old, line_hint);
            match match_result {
                Err(EditMatchError::NotFound) => {
                    // Fallback: if line_hint is provided, do line-based replacement
                    if let Some(ln) = line_hint {
                        let start = (ln as usize).saturating_sub(1).min(lines.len());
                        let old_line_count = old.lines().count().max(1);
                        let end = (start + old_line_count).min(lines.len());
                        let new_content = format!(
                            "{}{}{}",
                            lines[..start].join("\n"),
                            if start > 0 { "\n" } else { "" },
                            new,
                        );
                        // Add remaining lines after the replaced range
                        let new_content = if end < lines.len() {
                            format!("{}\n{}", new_content, lines[end..].join("\n"))
                        } else {
                            new_content
                        };
                        // Only apply if the old text roughly matches what we're replacing
                        let replaced_section: String = lines[start..end].join("\n");
                        let similarity = text_similarity(old, &replaced_section);
                        if similarity > 0.3 || old.len() < 20 {
                            let bak = backup_before_write(&full_path);
                            let bak_info = bak.map(|b| format!(" [backup: {}]", b.display())).unwrap_or_default();
                            match std::fs::write(&full_path, &new_content) {
                                Ok(_) => {
                                    let diff = diff_preview(ctx, path_str);
                                    if !diff.is_empty() {
                                        return format!("edited {} (line-based, similarity={:.0}%){}\n{}", full_path.display(), similarity * 100.0, bak_info, diff);
                                    } else {
                                        return format!("edited {} (line-based, similarity={:.0}%){}", full_path.display(), similarity * 100.0, bak_info);
                                    }
                                }
                                Err(e) => return format!("error writing {}: {e}", full_path.display()),
                            }
                        }
                    }
                    // Show detailed error with mismatch info
                    let mut snippet = if let Some(ln) = line_hint {
                        let idx = (ln as usize).saturating_sub(1).min(lines.len().saturating_sub(1));
                        let start = idx.saturating_sub(10);
                        let end = (idx + 10).min(lines.len());
                        let snip: Vec<String> = lines[start..end].iter().enumerate().map(|(i, l)| {
                            let current_ln = start + i + 1;
                            let prefix = if current_ln == ln as usize { "> " } else { "  " };
                            format!("{}{:>4} | {}", prefix, current_ln, l)
                        }).collect();
                        format!(" near line {}. Content around line {}:\n{}", ln, ln, snip.join("\n"))
                    } else {
                        let end = 15.min(lines.len());
                        let snip: Vec<String> = lines[..end].iter().enumerate().map(|(i, l)| {
                            format!("  {:>4} | {}", i + 1, l)
                        }).collect();
                        format!(". First 15 lines:\n{}", snip.join("\n"))
                    };
                    
                    // Show the first line of old text vs what's at that line in the file
                    let old_first = old.lines().next().unwrap_or("").trim();
                    let expected_line = line_hint.unwrap_or(1) as usize;
                    let file_at_line = lines.get(expected_line.saturating_sub(1)).unwrap_or(&"").trim();
                    snippet.push_str("\n\nTIP: Matching failed.\n");
                    if !old_first.is_empty() && !file_at_line.is_empty() {
                        snippet.push_str(&format!(
                            "  Your old text starts with:  \"{}\"\n  File content around line {}: \"{}\"\n\n",
                            old_first.chars().take(60).collect::<String>(),
                            expected_line,
                            file_at_line.chars().take(60).collect::<String>(),
                        ));
                    }
                    snippet.push_str("Common causes:\n");
                    snippet.push_str("1. Indentation mismatch — DeepSeek sometimes drops leading spaces\n");
                    snippet.push_str("2. You are looking at an outdated version of the file\n");
                    snippet.push_str("3. The code block has a typo in the middle\n\n");
                    snippet.push_str("ACTION: Use 'read_file' to get the exact content before retrying.");
                    
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
                    let bak = backup_before_write(&full_path);
                    let bak_info = bak.as_ref().map(|b| format!(" [backup: {}]", b.display())).unwrap_or_default();
                    match std::fs::write(&full_path, &new_content) {
                        Ok(_) => {
                            let diff = diff_preview(ctx, path_str);
                            if !diff.is_empty() {
                                format!("edited {}{}\n{}", full_path.display(), bak_info, diff)
                            } else {
                                format!("edited {}{}", full_path.display(), bak_info)
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
                // Normalization: strip all whitespace to compare content only
                let old_norm: String = old_line.chars().filter(|c| !c.is_whitespace()).collect();
                let file_norm: String = file_line.chars().filter(|c| !c.is_whitespace()).collect();
                
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
    
    // If no match found and we have a line hint, try even more relaxed matching
    if matches.is_empty() && line_hint.is_some() {
        let ln = line_hint.unwrap() as usize;
        let start_search = ln.saturating_sub(10).min(lines.len());
        let end_search = (ln + 10).min(lines.len());
        
        for start in start_search..end_search.saturating_sub(old_lines.len().saturating_sub(1)) {
            let mut matches_count = 0;
            for (i, old_line) in old_lines.iter().enumerate() {
                let file_line = content_trimmed.get(start + i).unwrap_or(&"");
                let old_norm: String = old_line.split_whitespace().collect::<Vec<_>>().join(" ");
                let file_norm: String = file_line.split_whitespace().collect::<Vec<_>>().join(" ");
                if old_norm == file_norm || (old_norm.len() > 10 && (file_norm.contains(&old_norm) || old_norm.contains(&file_norm))) {
                    matches_count += 1;
                }
            }
            // If most lines match, consider it a match
            if matches_count >= old_lines.len().saturating_sub(1).max(1) {
                let byte_start = line_offsets[start];
                let byte_end = line_offsets.get(start + old_lines.len()).copied().unwrap_or(content.len());
                matches.push((byte_start, byte_end.saturating_sub(byte_start)));
                break; // Take the first "good enough" match near the hint
            }
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

// ── Position resolution engine ──────────────────────────────

/// Find the line range [start, end] (0-based, end exclusive) for a target.
/// Supports: Rust symbols, shell/bash functions, and any language by text search.
fn resolve_symbol_range(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    // Scan all lines for a definition
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();

        // Rust definitions
        let is_rust_def = (trimmed.contains(&format!("fn {}", name))
            || trimmed.contains(&format!("struct {}", name))
            || trimmed.contains(&format!("enum {}", name))
            || trimmed.contains(&format!("trait {}", name))
            || trimmed.contains(&format!("type {}", name))
            || (trimmed.starts_with("impl") && trimmed.contains(name)))
            && (trimmed.starts_with("pub") || trimmed.starts_with("fn")
                || trimmed.starts_with("struct") || trimmed.starts_with("enum")
                || trimmed.starts_with("trait") || trimmed.starts_with("type")
                || trimmed.starts_with("impl"));

        // Shell function definitions: name() {  or  function name {
        let is_shell_def = trimmed.starts_with(&format!("{}()", name))
            || trimmed.starts_with(&format!("{} ()", name))
            || trimmed.starts_with(&format!("function {} ", name));

        if is_rust_def || is_shell_def {
            let end = find_block_end(lines, i);
            return Some((i, end));
        }
    }

    // Fallback: text search with brace extension
    for (i, line) in lines.iter().enumerate() {
        if line.contains(name) {
            if line.contains('{') {
                let end = find_block_end(lines, i);
                return Some((i, end));
            }
            if i + 1 < lines.len() && lines[i + 1].contains('{') {
                let end = find_block_end(lines, i + 1);
                return Some((i, end));
            }
            return Some((i, i + 1));
        }
    }

    None
}

/// Build a compact symbol map for large files (>500 lines).
/// Scans for fn/struct/enum/trait/type/impl/mod definitions and their line ranges.
fn build_symbol_map(lines: &[&str]) -> String {
    let kw_pairs = [
        ("fn ", "fn"), ("struct ", "struct"), ("enum ", "enum"),
        ("trait ", "trait"), ("type ", "type"), ("impl ", "impl"),
        ("mod ", "mod"), ("pub mod ", "mod"),
    ];
    let mut entries: Vec<String> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();

        // Skip non-definition lines quickly
        if !trimmed.starts_with("pub") && !trimmed.starts_with("fn")
            && !trimmed.starts_with("struct") && !trimmed.starts_with("enum")
            && !trimmed.starts_with("trait") && !trimmed.starts_with("type")
            && !trimmed.starts_with("impl") && !trimmed.starts_with("mod")
        {
            continue;
        }

        for (kw, label) in &kw_pairs {
            // Find keyword in the trimmed line at word boundary
            if let Some(pos) = trimmed.find(kw) {
                // Extract the name after the keyword
                let rest = &trimmed[pos + kw.len()..].trim_start();
                let name = rest.split(|c: char| c.is_whitespace() || c == '{' || c == '(' || c == '<' || c == ':').next().unwrap_or("");
                if name.is_empty() || name.starts_with('{') { continue; }

                let end = find_block_end(lines, i);
                let line_range = if end > i + 1 {
                    format!("{}-{}", i + 1, end)
                } else {
                    (i + 1).to_string()
                };
                entries.push(format!("  {} {}:{}", label, name, line_range));
                break;
            }
        }

        if entries.len() >= 40 { break; } // cap at 40 entries
    }
    entries.join("\n")
}

/// Find where a brace-delimited block ends (0-based index, exclusive).
/// Counts `{` and `}` starting from the given line. Returns the line
/// index after the closing `}` of the top-level block.
fn find_block_end(lines: &[&str], start: usize) -> usize {
    let mut depth: i32 = 0;
    let mut first_brace_seen = false;

    for i in start..lines.len() {
        for ch in lines[i].chars() {
            match ch {
                '{' => { depth += 1; first_brace_seen = true; }
                '}' => { depth -= 1; }
                _ => {}
            }
        }
        // Block ends when depth returns to 0 after the first opening brace
        if first_brace_seen && depth <= 0 {
            return i + 1; // exclusive end
        }
    }
    lines.len() // fallback: entire rest of file
}

// ── List files ────────────────────────────────────────────────

pub(crate) fn exec_list_files(ctx: &ToolCtx, args: &str) -> String {
    let v = parse_args!(args);
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
    let v = parse_args!(args);
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

// ── Get file info ─────────────────────────────────────────────

pub(crate) fn exec_get_file_info(ctx: &ToolCtx, args: &str) -> String {
    let v = parse_args!(args);
    let path_str = v["path"].as_str().unwrap_or("");
    if path_str.is_empty() {
        return "error: no path provided".to_string();
    }
    let full_path = cwd_join(ctx, path_str);
    match std::fs::metadata(&full_path) {
        Ok(meta) => {
            let size = meta.len();
            let is_dir = meta.is_dir();
            let modified = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            
            let mut res = json!({
                "path": full_path.display().to_string(),
                "size": size,
                "is_dir": is_dir,
                "modified": modified,
            });

            if !is_dir {
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let lines: Vec<&str> = content.lines().collect();
                    res["line_count"] = json!(lines.len());
                    // Add first 2 lines as preview
                    let preview: Vec<&str> = lines.iter().take(2).copied().collect();
                    res["preview"] = json!(preview.join("\n"));
                }
            }
            res.to_string()
        }
        Err(e) => format!("error: {e}"),
    }
}

// ── List tree ─────────────────────────────────────────────────

pub(crate) fn exec_list_tree(ctx: &ToolCtx, args: &str) -> String {
    let v = parse_args!(args);
    let path_str = v["path"].as_str().unwrap_or(".");
    let max_depth = v["depth"].as_u64().unwrap_or(2) as usize;
    let root = cwd_join(ctx, path_str);
    
    let mut out = String::new();
    fn walk(dir: &std::path::Path, prefix: &str, depth: usize, max_depth: usize, out: &mut String) {
        if depth > max_depth { return; }
        let Ok(entries) = std::fs::read_dir(dir) else { return; };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        
        for (i, entry) in entries.iter().enumerate() {
            let is_last = i == entries.len() - 1;
            let connector = if is_last { "└── " } else { "├── " };
            let name = entry.file_name().to_string_lossy().to_string();
            out.push_str(&format!("{}{}{}\n", prefix, connector, name));
            
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
                walk(&entry.path(), &new_prefix, depth + 1, max_depth, out);
            }
        }
    }
    
    out.push_str(&format!("{}\n", root.display()));
    walk(&root, "", 1, max_depth, &mut out);
    out
}

// ── Diff preview helper ───────────────────────────────────────

pub(crate) fn diff_preview(ctx: &ToolCtx, path_str: &str) -> String {
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

/// Simple character-level similarity between two strings (0.0 - 1.0).
/// Used by edit_file to decide whether a line-based fallback is safe.
fn text_similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() { return 1.0; }
    if a.is_empty() || b.is_empty() { return 0.0; }
    let max_len = a.len().max(b.len()) as f64;
    let mut matches = 0usize;
    for i in 0..a.len().min(b.len()) {
        if a[i] == b[i] { matches += 1; }
    }
    matches as f64 / max_len
}
