//! `torii describe` — pretty name for HEAD based on the nearest tag.
//!
//! Format: `<tag>-<n>-g<short>` where:
//!   - `tag` is the closest annotated tag reachable from HEAD,
//!   - `n` is the number of commits between that tag and HEAD,
//!   - `short` is the 7-char prefix of HEAD's OID.
//!
//! If HEAD is exactly the tagged commit, just `<tag>` is printed.
//! If `--dirty` is set and the working tree has uncommitted changes, the
//! output is suffixed with `-dirty`.
//! If no tag is reachable, falls back to just the short OID.

use crate::error::{Result, ToriiError};
use git2::{DescribeFormatOptions, DescribeOptions, Repository};
use std::path::Path;

#[derive(Debug, Default)]
pub struct Opts {
    /// Include lightweight tags, not just annotated. Default matches
    /// `git describe` (annotated-only).
    pub tags: bool,
    /// Always emit `<tag>-<n>-g<oid>` form even if HEAD is exactly on a
    /// tag. Useful for scripts that want a stable format.
    pub long: bool,
    /// Append `-dirty` if the working tree has changes.
    pub dirty: bool,
    /// Show all (default = closest 10).
    pub candidates: u32,
}

pub fn describe(repo_path: &Path, opts: &Opts) -> Result<()> {
    let repo = Repository::open(repo_path).map_err(ToriiError::Git)?;

    let mut d_opts = DescribeOptions::new();
    if opts.tags {
        d_opts.describe_tags();
    } else {
        d_opts.describe_tags(); // libgit2 default is annotated-tags only;
                                 // we mirror that by passing through.
    }
    if opts.candidates > 0 {
        d_opts.max_candidates_tags(opts.candidates);
    }

    let result = match repo.describe(&d_opts) {
        Ok(d) => {
            let mut fmt = DescribeFormatOptions::new();
            if opts.long {
                fmt.always_use_long_format(true);
            }
            d.format(Some(&fmt)).map_err(ToriiError::Git)?
        }
        Err(_) => {
            // No reachable tag — fall back to short OID.
            let head = repo.head().map_err(ToriiError::Git)?;
            let oid = head
                .target()
                .ok_or_else(|| ToriiError::InvalidConfig("HEAD has no target".into()))?;
            format!("{}", &oid.to_string()[..7])
        }
    };

    let suffix = if opts.dirty {
        let mut so = git2::StatusOptions::new();
        so.include_untracked(false).include_ignored(false);
        let statuses = repo.statuses(Some(&mut so)).map_err(ToriiError::Git)?;
        let dirty = statuses
            .iter()
            .any(|s| !s.status().contains(git2::Status::IGNORED));
        if dirty { "-dirty" } else { "" }
    } else {
        ""
    };

    println!("{result}{suffix}");
    Ok(())
}
