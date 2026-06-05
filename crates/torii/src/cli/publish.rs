//! `torii publish` — cargo publish with torii-managed token.

use anyhow::Result;

pub(crate) fn run(
    dry_run: &bool,
    no_verify: &bool,
    token: &Option<String>,
    allow_dirty: &bool,
) -> Result<()> {
    let resolved = match token {
        Some(t) => t.clone(),
        None => crate::auth::resolve_token("cargo", ".")
            .value
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No cargo token configured. Set one with:\n  torii auth set cargo <token>\n\
                             …or pass --token <value> just for this invocation."
                )
            })?,
    };
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("publish");
    if *dry_run {
        cmd.arg("--dry-run");
    }
    if *no_verify {
        cmd.arg("--no-verify");
    }
    if *allow_dirty {
        cmd.arg("--allow-dirty");
    }
    // Pass --locked by default — same convention torii uses
    // elsewhere so the verify-build matches the committed lock.
    cmd.arg("--locked");
    cmd.env("CARGO_REGISTRY_TOKEN", &resolved);
    let mode = if *dry_run { "dry-run" } else { "publishing" };
    println!("📦 cargo {} (token injected from torii auth)…", mode);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("cargo publish exited with {}", status);
    }
    if !*dry_run {
        // Read [package].name from Cargo.toml so the URL is
        // accurate even if the binary name differs from the
        // crate name (gitorii binary is `torii`).
        let crate_name = std::fs::read_to_string("Cargo.toml")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.trim_start().starts_with("name "))
                    .and_then(|l| l.split('=').nth(1))
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "<crate>".to_string());
        println!(
            "\n✅ Published. View at https://crates.io/crates/{}",
            crate_name
        );
    }
    Ok(())
}
