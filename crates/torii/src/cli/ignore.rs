//! `torii ignore` — .toriignore rules.

use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum IgnoreCommands {
    /// Add a path pattern to .toriignore (or .toriignore.local with --local)
    Add {
        /// Glob/path pattern (e.g. `build/`, `*.log`, `/internal/`)
        pattern: String,
        /// Write to .toriignore.local instead of .toriignore (private, not committed)
        #[arg(long)]
        local: bool,
    },
    /// Add a secret regex rule. Defaults to .toriignore.local (private).
    /// Pass --public to put the rule in the committed .toriignore instead.
    Secret {
        /// Regex pattern matching the secret
        pattern: String,
        /// Optional human name shown when the rule fires
        #[arg(long)]
        name: Option<String>,
        /// Write to public .toriignore instead of .toriignore.local
        #[arg(long)]
        public: bool,
    },
    /// List effective rules (public + local merged)
    List,
}

pub(crate) fn handle_ignore(action: &IgnoreCommands) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    let repo_root = std::path::Path::new(".");
    let public = repo_root.join(".toriignore");
    let local = repo_root.join(".toriignore.local");

    fn append_section(path: &std::path::Path, section: &str, line: &str) -> Result<()> {
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let header = format!("[{}]", section);
        // Active header = line equal to `[section]` after trimming, NOT commented.
        let has_active_header = existing.lines().any(|l| l.trim() == header);
        let mut out = OpenOptions::new().create(true).append(true).open(path)?;
        if !has_active_header {
            if !existing.is_empty() && !existing.ends_with('\n') {
                writeln!(out)?;
            }
            writeln!(out)?;
            writeln!(out, "{}", header)?;
        }
        writeln!(out, "{}", line)?;
        Ok(())
    }

    match action {
        IgnoreCommands::Add {
            pattern,
            local: use_local,
        } => {
            let target = if *use_local { &local } else { &public };
            let existing = std::fs::read_to_string(target).unwrap_or_default();
            let mut f = OpenOptions::new().create(true).append(true).open(target)?;
            if !existing.is_empty() && !existing.ends_with('\n') {
                writeln!(f)?;
            }
            writeln!(f, "{}", pattern)?;
            let label = if *use_local {
                ".toriignore.local (private)"
            } else {
                ".toriignore"
            };
            println!("✅ Added `{}` to {}", pattern, label);
        }
        IgnoreCommands::Secret {
            pattern,
            name,
            public: use_public,
        } => {
            // Validate regex before writing
            regex::Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex: {}", e))?;
            let line = match name {
                Some(n) => format!("deny: {}  # {}", pattern, n),
                None => format!("deny: {}", pattern),
            };
            let target = if *use_public { &public } else { &local };
            append_section(target, "secrets", &line)?;
            let label = if *use_public {
                ".toriignore (public — visible in repo)"
            } else {
                ".toriignore.local (private — never committed)"
            };
            println!("✅ Added secret rule to {}", label);
            if *use_public {
                println!("⚠️  Consider --local instead: secret-pattern shape can aid recon if repo leaks");
            }
        }
        IgnoreCommands::List => {
            let ti = crate::toriignore::ToriIgnore::load(repo_root)?;
            println!("📋 Effective .toriignore rules (public + local merged)\n");
            println!("Paths ({}):", ti.patterns().len());
            for p in ti.patterns() {
                println!("  {}", p);
            }
            println!("\nSecrets ({}):", ti.secrets.len());
            for s in &ti.secrets {
                println!("  {} → {}", s.name, s.regex.as_str());
            }
            if ti.size.max_bytes.is_some() || ti.size.warn_bytes.is_some() {
                println!("\nSize:");
                if let Some(m) = ti.size.max_bytes {
                    println!("  max: {} bytes", m);
                }
                if let Some(w) = ti.size.warn_bytes {
                    println!("  warn: {} bytes", w);
                }
            }
            if !ti.hooks.pre_save.is_empty() || !ti.hooks.pre_sync.is_empty() {
                println!("\nHooks:");
                for h in &ti.hooks.pre_save {
                    println!("  pre-save: {}", h);
                }
                for h in &ti.hooks.pre_sync {
                    println!("  pre-sync: {}", h);
                }
                for h in &ti.hooks.post_save {
                    println!("  post-save: {}", h);
                }
                for h in &ti.hooks.post_sync {
                    println!("  post-sync: {}", h);
                }
            }
            if local.exists() {
                println!("\n🔒 .toriignore.local present (private, gitignored)");
            }
        }
    }
    Ok(())
}
