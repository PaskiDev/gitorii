//! `torii sign` / `torii show --signature` — GPG signing helpers.
//!
//! Presentation layer only: the signing/extraction logic lives in
//! `vcs/sign.rs` and returns data; this module prompts and prints.

use crate::core::GitRepo;
use anyhow::Result;

/// 0.7.35 — scope guard for `TORII_SIGN_OVERRIDE`. Set on construction,
/// restored on drop. Lets the `Save` handler force-enable / disable
/// GPG signing for a single commit without leaving the env var dirty
/// for anything that runs after (subprocesses for hooks, the mirror
/// sync that follows a `save -am … && sync`, …).
pub(crate) struct SignOverrideGuard {
    prev: Option<String>,
    touched: bool,
}

impl SignOverrideGuard {
    pub(crate) fn new(value: Option<bool>) -> Self {
        let prev = std::env::var("TORII_SIGN_OVERRIDE").ok();
        match value {
            Some(true) => std::env::set_var("TORII_SIGN_OVERRIDE", "true"),
            Some(false) => std::env::set_var("TORII_SIGN_OVERRIDE", "false"),
            None => {
                return SignOverrideGuard {
                    prev,
                    touched: false,
                }
            }
        }
        SignOverrideGuard {
            prev,
            touched: true,
        }
    }
}

impl Drop for SignOverrideGuard {
    fn drop(&mut self) {
        if !self.touched {
            return;
        }
        match &self.prev {
            Some(v) => std::env::set_var("TORII_SIGN_OVERRIDE", v),
            None => std::env::remove_var("TORII_SIGN_OVERRIDE"),
        }
    }
}

/// `torii show --signature` — extract the GPG armor from a commit
/// object and print it, followed by the local verification verdict.
pub(crate) fn run_show_signature(repo: &GitRepo, object: Option<&str>) -> Result<()> {
    let target = object.unwrap_or("HEAD");
    let sig = repo.extract_commit_signature(target)?;

    println!("commit: {}", sig.oid);
    println!();
    println!("{}", sig.armor.trim_end());
    println!();

    let program = repo
        .workdir()
        .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
        .and_then(|c| c.git.gpg_program);

    match crate::gpg::verify(&sig.armor, &sig.payload, program.as_deref())? {
        crate::gpg::VerifyStatus::Good { signer } => {
            println!("✓ Good signature from {}", signer);
        }
        crate::gpg::VerifyStatus::UnknownKey { key_id } => {
            let k = key_id.as_deref().unwrap_or("?");
            println!("? Unknown signer key {} — import it to verify locally.", k);
        }
        crate::gpg::VerifyStatus::Bad => {
            println!("✗ BAD signature — payload does not match.");
        }
        crate::gpg::VerifyStatus::Other(msg) => {
            println!("? {}", msg);
        }
    }
    Ok(())
}

/// `torii sign <oid|range>` — rewrite the named commits to include a
/// fresh `gpgsig` header. The dirty-tree guard and the rewrite itself
/// live in `vcs/sign.rs`; this handler resolves config, confirms with
/// the user and prints the outcome.
pub(crate) fn run_sign(target: Option<&str>, print_only: bool, yes: bool) -> Result<()> {
    let target = target.unwrap_or("HEAD");
    let repo = GitRepo::open(".")?;

    let tc = repo
        .workdir()
        .and_then(|wd| crate::config::ToriiConfig::load_local(wd).ok())
        .unwrap_or_else(|| crate::config::ToriiConfig::load_global().unwrap_or_default());
    let key = tc
        .git
        .gpg_key
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "git.gpg_key is not set. Configure with `torii config set git.gpg_key <KEY-ID>`."
            )
        })?;

    if print_only {
        let previews = repo.preview_signatures(target, key, tc.git.gpg_program.as_deref())?;
        if previews.is_empty() {
            println!("(no commits in range)");
            return Ok(());
        }
        for (oid, armor) in &previews {
            println!("# {}", oid);
            println!("{}", armor.trim_end());
            println!();
        }
        return Ok(());
    }

    let oids = repo.resolve_commit_range(target)?;
    if oids.is_empty() {
        println!("(no commits in range)");
        return Ok(());
    }

    if !yes {
        println!(
            "About to rewrite {} commit(s) with new GPG signatures.",
            oids.len()
        );
        println!(
            "All affected commits' OIDs will change. Branches pointing at them get rewritten."
        );
        print!("Proceed? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Cancelled.");
            return Ok(());
        }
    }

    let outcome = repo.sign_range(target, key, tc.git.gpg_program.as_deref())?;

    for r in &outcome.rewritten {
        println!("  {} → {}", &r.old[..8], &r.new[..8]);
    }
    if outcome.branches_moved > 0 {
        println!(
            "Moved {} branch tip(s) to the new signed commit(s).",
            outcome.branches_moved
        );
    }
    println!("✓ Signed {} commit(s).", outcome.rewritten.len());
    Ok(())
}
