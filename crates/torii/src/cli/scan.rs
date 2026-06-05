//! `torii scan` — secret scanner + commit-policy linting.

use crate::scanner;
use anyhow::Result;
use std::path::PathBuf;

/// Template `policies/commits.toml` written by `torii init`. Conservative
/// defaults so a fresh repo doesn't fail every save out of the box — users
/// uncomment / extend rules they want enforced.
pub(crate) const DEFAULT_COMMITS_POLICY: &str = r#"# torii commit policy — written by `torii init`.
# Edit / extend; run `torii scan --commits` to evaluate.
# Docs: https://gitorii.com/docs/policies/commits

# Block AI-tooling co-author trailers from leaking into history.
forbid_trailers = [
    "Co-Authored-By:.*Claude",
    "Co-Authored-By:.*Copilot",
    "Co-Authored-By:.*GPT",
]

# Reject lazy / temp subjects.
forbid_subjects = ["^(wip|tmp|temp|misc|asdf|update|fix)$"]

# Subject sanity.
subject_min_length = 8
subject_max_length = 72

# Conventional Commits — uncomment to enforce.
# require_conventional = true

# Pin commits to your domain (uncomment + adjust):
# author_email_matches = ".*@example\\.com$"

# DCO sign-off (uncomment to require):
# require_trailers = ["Signed-off-by:"]
"#;

pub(crate) fn run(
    history: &bool,
    commits: &bool,
    policy_file: &Option<PathBuf>,
    limit: &usize,
) -> Result<()> {
    if *commits {
        run_commit_scan(policy_file.as_deref(), *limit)?;
    } else {
        run_scan(*history)?;
    }
    Ok(())
}

fn run_scan(history: bool) -> Result<()> {
    let repo_path = std::path::Path::new(".");
    if history {
        println!("🔍 Scanning full git history for sensitive data...\n");
        let results = scanner::scan_history(repo_path)?;
        if results.is_empty() {
            println!("✅ No sensitive data found in history.");
        } else {
            println!("⚠️  Found sensitive data in {} commit(s):\n", results.len());
            for (commit, findings) in &results {
                println!("  📌 {}", commit);
                for f in findings {
                    println!("     {}:{} — {}", f.file, f.line, f.pattern_name);
                    println!("     {}", f.preview);
                }
                println!();
            }
            println!("💡 To clean history: torii history rebase <base> --todo-file <plan>");
        }
    } else {
        println!("🔍 Scanning staged files for sensitive data...\n");
        let findings = scanner::scan_staged(repo_path)?;
        if findings.is_empty() {
            println!("✅ No sensitive data detected in staged files.");
        } else {
            println!("⚠️  Found {} issue(s):\n", findings.len());
            for f in &findings {
                println!("  {}:{} — {}", f.file, f.line, f.pattern_name);
                println!("  {}\n", f.preview);
            }
            println!("💡 Tip: use .env.example for placeholder values.");
        }
    }
    Ok(())
}

fn run_commit_scan(policy_path: Option<&std::path::Path>, limit: usize) -> Result<()> {
    use crate::commit_scan::{default_policy_path, scan_repo, CompiledCommitPolicy};
    let repo = git2::Repository::discover(".").map_err(|e| anyhow::anyhow!("not a repo: {}", e))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repos can't host policies/commits.toml"))?
        .to_path_buf();
    let path = match policy_path {
        Some(p) => p.to_path_buf(),
        None => default_policy_path(&workdir),
    };
    let policy = match CompiledCommitPolicy::load(&path)? {
        Some(p) => p,
        None => {
            println!("ℹ️  No commit policy found at {}.", path.display());
            println!("    Run `torii init` (or create the file manually) to add one.");
            return Ok(());
        }
    };
    let violations = scan_repo(&repo, &policy, limit)?;
    if violations.is_empty() {
        println!("✅ {} commits scanned, no policy violations.", limit);
        return Ok(());
    }
    println!(
        "❌ {} violation(s) across the last {} commits:\n",
        violations.len(),
        limit
    );
    for v in &violations {
        println!("  {} \"{}\"", v.commit_short, v.subject);
        println!("      [{}] {}", v.rule, v.detail);
    }
    println!();
    std::process::exit(1);
}
