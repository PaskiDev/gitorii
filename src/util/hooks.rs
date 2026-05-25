use std::path::Path;
use std::process::Command;
use std::time::Instant;
use anyhow::{Result, anyhow};

use crate::toriignore::{HookRules, SizeRules, glob_match};

/// Execute every hook command in order. First non-zero exit aborts.
///
/// **Security:** `.toriignore` lives in the repo, so cloning a hostile repo
/// would otherwise let it run arbitrary `sh -c …` on the very first
/// `torii save`. Before executing, we require the user to have *trusted*
/// this exact set of commands for this exact repo path. Trust is stored in
/// `~/.config/torii/hook-trust.toml` keyed by repo + sha256(commands).
///
/// Bypass:
///   `TORII_TRUST_HOOKS=1` — skip the prompt (CI / scripted use)
///   `TORII_NO_HOOKS=1`    — skip hooks entirely
///   `--skip-hooks` flag   — same as above for one invocation
pub fn run_hooks(label: &str, commands: &[String], repo: &Path) -> Result<()> {
    if commands.is_empty() { return Ok(()); }
    if std::env::var("TORII_NO_HOOKS").is_ok() {
        return Ok(());
    }

    if !is_trusted(repo, commands)? {
        if std::env::var("TORII_TRUST_HOOKS").is_ok() {
            // Implicit trust on CI; remember so subsequent runs don't re-trigger.
            mark_trusted(repo, commands)?;
        } else if !prompt_trust(repo, label, commands)? {
            return Err(anyhow!(
                "hook execution declined. Re-run with TORII_TRUST_HOOKS=1 to trust, \
                 TORII_NO_HOOKS=1 to skip, or --skip-hooks for this invocation."
            ));
        }
    }

    println!("🪝 {} hooks: {} command(s)", label, commands.len());
    for cmd in commands {
        let start = Instant::now();
        print!("   → {} ", cmd);
        use std::io::Write;
        std::io::stdout().flush().ok();

        let status = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(repo)
            .status()
            .map_err(|e| anyhow!("failed to spawn `{}`: {}", cmd, e))?;

        let dur = start.elapsed();
        if !status.success() {
            let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
            return Err(anyhow!(
                "hook failed: `{}` exited with {} after {:.2}s — fix the issue or rerun with --skip-hooks",
                cmd, code, dur.as_secs_f64()
            ));
        }
        println!("✓ ({:.2}s)", dur.as_secs_f64());
    }
    Ok(())
}

// ── Trust store ──────────────────────────────────────────────────────────────

fn trust_file_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("torii").join("hook-trust.toml"))
}

/// SHA256 of the joined command list. Cheap, deterministic, no extra dep —
/// stdlib lacks sha256 so we use a small FNV-1a 64-bit fallback. Collision
/// resistance is not required: the worst case is a malicious actor crafting
/// a hook list with the same hash as a previously trusted one for the same
/// repo, which already requires repo-level write access (game over anyway).
fn hash_commands(commands: &[String]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for c in commands {
        for b in c.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= b'\n' as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

fn repo_key(repo: &Path) -> String {
    repo.canonicalize()
        .unwrap_or_else(|_| repo.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn is_trusted(repo: &Path, commands: &[String]) -> Result<bool> {
    let Some(path) = trust_file_path() else { return Ok(false) };
    if !path.exists() { return Ok(false); }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("read {}: {}", path.display(), e))?;
    let key = repo_key(repo);
    let hash = hash_commands(commands);
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let Some((k, v)) = line.split_once('=') else { continue };
        let k = k.trim().trim_matches('"');
        let v = v.trim().trim_matches('"');
        if k == key && v == hash { return Ok(true); }
    }
    Ok(false)
}

fn mark_trusted(repo: &Path, commands: &[String]) -> Result<()> {
    let Some(path) = trust_file_path() else { return Ok(()); };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let key = repo_key(repo);
    let hash = hash_commands(commands);

    // Read existing, drop any prior entry for this repo (so a re-trust
    // replaces stale hash), then append the new line.
    let mut buf = String::new();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    buf.push_str(line);
                    buf.push('\n');
                    continue;
                }
                let key_in_line = trimmed
                    .split_once('=')
                    .map(|(k, _)| k.trim().trim_matches('"').to_string())
                    .unwrap_or_default();
                if key_in_line != key {
                    buf.push_str(line);
                    buf.push('\n');
                }
            }
        }
    }
    if buf.is_empty() {
        buf.push_str("# torii hook trust store — written by `torii` after explicit user consent\n");
    }
    buf.push_str(&format!("\"{}\" = \"{}\"\n", key, hash));
    std::fs::write(&path, buf)
        .map_err(|e| anyhow!("write {}: {}", path.display(), e))?;
    Ok(())
}

fn prompt_trust(repo: &Path, label: &str, commands: &[String]) -> Result<bool> {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        // No tty → cannot prompt. Refuse rather than silently execute.
        eprintln!(
            "⚠️  {} hooks defined in {} (untrusted, no tty to prompt).",
            label, repo.display()
        );
        eprintln!("   Run interactively to trust, or set TORII_TRUST_HOOKS=1 / --skip-hooks.");
        return Ok(false);
    }
    println!();
    println!("⚠️  This repo defines {} hook(s) that will run via `sh -c`:", label);
    for cmd in commands {
        println!("     • {}", cmd);
    }
    println!("   repo: {}", repo.display());
    print!("   Trust and run? [y/N] ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let answer = line.trim().to_ascii_lowercase();
    let yes = matches!(answer.as_str(), "y" | "yes");
    if yes {
        mark_trusted(repo, commands)?;
        println!("   ✓ trusted; remembered in ~/.config/torii/hook-trust.toml");
    }
    Ok(yes)
}

/// Convenience: pre-save / pre-sync / post-* dispatch
pub fn pre_save(rules: &HookRules, repo: &Path) -> Result<()> {
    run_hooks("pre-save", &rules.pre_save, repo)
}
pub fn pre_sync(rules: &HookRules, repo: &Path) -> Result<()> {
    run_hooks("pre-sync", &rules.pre_sync, repo)
}
pub fn post_save(rules: &HookRules, repo: &Path) {
    let _ = run_hooks("post-save", &rules.post_save, repo);
}
pub fn post_sync(rules: &HookRules, repo: &Path) {
    let _ = run_hooks("post-sync", &rules.post_sync, repo);
}

/// Check staged file sizes against [size] limits.
/// Returns Err if any file exceeds `max`. Prints warnings for `warn` overruns.
pub fn check_size(rules: &SizeRules, repo: &Path, staged_paths: &[String]) -> Result<()> {
    if rules.max_bytes.is_none() && rules.warn_bytes.is_none() { return Ok(()); }

    let mut blocked: Vec<(String, u64)> = Vec::new();
    let mut warned: Vec<(String, u64)> = Vec::new();

    for rel in staged_paths {
        if rules.exclude.iter().any(|g| glob_match(rel, g)) { continue; }
        let abs = repo.join(rel);
        let size = match std::fs::metadata(&abs) {
            Ok(m) => m.len(),
            Err(_) => continue, // deleted file or unreadable
        };
        if let Some(max) = rules.max_bytes {
            if size > max { blocked.push((rel.clone(), size)); continue; }
        }
        if let Some(warn) = rules.warn_bytes {
            if size > warn { warned.push((rel.clone(), size)); }
        }
    }

    for (path, size) in &warned {
        println!("⚠️  large file: {} ({})", path, human_size(*size));
    }
    if !blocked.is_empty() {
        let mut msg = String::from("size limit exceeded:\n");
        for (path, size) in &blocked {
            msg.push_str(&format!("   {} — {}\n", path, human_size(*size)));
        }
        msg.push_str("\nAdjust [size] max in .toriignore, exclude these paths, or use git LFS.");
        return Err(anyhow!(msg));
    }
    Ok(())
}

fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB { format!("{:.2} GB", bytes as f64 / GB as f64) }
    else if bytes >= MB { format!("{:.2} MB", bytes as f64 / MB as f64) }
    else if bytes >= KB { format!("{:.1} KB", bytes as f64 / KB as f64) }
    else { format!("{} B", bytes) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toriignore::SizeRules;

    #[test]
    fn human_size_boundaries() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(2 * 1024 * 1024), "2.00 MB");
    }

    #[test]
    fn size_check_blocks_oversize() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.bin");
        std::fs::write(&big, vec![0u8; 1024 * 1024]).unwrap(); // 1 MB
        let rules = SizeRules { max_bytes: Some(500 * 1024), warn_bytes: None, exclude: vec![] };
        let err = check_size(&rules, dir.path(), &["big.bin".to_string()]).unwrap_err();
        assert!(err.to_string().contains("size limit exceeded"));
    }

    #[test]
    fn size_check_respects_exclude() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("artwork.psd");
        std::fs::write(&big, vec![0u8; 1024 * 1024]).unwrap();
        let rules = SizeRules {
            max_bytes: Some(100),
            warn_bytes: None,
            exclude: vec!["*.psd".to_string()],
        };
        check_size(&rules, dir.path(), &["artwork.psd".to_string()]).unwrap();
    }

    #[test]
    fn size_check_skips_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let rules = SizeRules { max_bytes: Some(100), warn_bytes: None, exclude: vec![] };
        check_size(&rules, dir.path(), &["nonexistent".to_string()]).unwrap();
    }
}
