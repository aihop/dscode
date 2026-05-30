/// Get current terminal width, fallback to 80.
pub fn terminal_width() -> u16 {
    if let Ok(cols) = std::env::var("COLUMNS") {
        if let Ok(w) = cols.parse::<u16>() {
            if w > 0 {
                return w;
            }
        }
    }
    if let Ok(o) = std::process::Command::new("stty")
        .args(["size"])
        .stdin(std::process::Stdio::inherit())
        .output()
    {
        if let Ok(s) = String::from_utf8(o.stdout) {
            let parts: Vec<&str> = s.trim().split_whitespace().collect();
            if parts.len() == 2 {
                if let Ok(w) = parts[1].parse::<u16>() {
                    if w > 0 {
                        return w;
                    }
                }
            }
        }
    }
    80
}

/// Check if terminal is considered "narrow" (<= 80 chars).
pub fn is_narrow_terminal() -> bool {
    terminal_width() <= 72
}

/// Unified dscode home directory (~/.dscode).
/// Replaces scattered ~/.config/dscode and ~/.local/share/dscode paths.
pub fn dscode_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| "~".to_string());
    std::path::PathBuf::from(home).join(".dscode")
}
