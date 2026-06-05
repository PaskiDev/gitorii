//! GPG signing over existing commit objects — data layer for `torii sign`
//! and `torii show --signature`. Presentation (prompts, printing) lives in
//! `cli/sign.rs`; everything here returns data and precise errors.

use crate::core::GitRepo;
use crate::error::{Result, ToriiError};

/// Armor + signed payload extracted from a commit's `gpgsig` header.
pub struct CommitSignature {
    pub oid: String,
    pub armor: String,
    pub payload: Vec<u8>,
}

/// One rewritten commit from a signing pass.
pub struct SignedRewrite {
    pub old: String,
    pub new: String,
}

/// Result of [`GitRepo::sign_range`].
pub struct SignOutcome {
    pub rewritten: Vec<SignedRewrite>,
    pub branches_moved: usize,
}

impl GitRepo {
    /// Extract the GPG armor and signed payload from a commit. Errors with
    /// [`ToriiError::RepoState`] when the commit carries no signature.
    pub fn extract_commit_signature(&self, target: &str) -> Result<CommitSignature> {
        let r = &self.repo;
        let oid = r
            .revparse_single(target)
            .map_err(|e| ToriiError::Usage(format!("`{}`: {}", target, e)))?
            .id();

        let (sig_buf, payload_buf) = r.extract_signature(&oid, None).map_err(|_| {
            ToriiError::RepoState(format!(
                "commit {} has no GPG signature attached. Use `torii sign {}` to add one.",
                &oid.to_string()[..8],
                target
            ))
        })?;
        let armor = std::str::from_utf8(&sig_buf)
            .map_err(|e| ToriiError::RepoState(format!("signature is not valid UTF-8: {}", e)))?
            .to_string();

        Ok(CommitSignature {
            oid: oid.to_string(),
            armor,
            payload: (&*payload_buf).to_vec(),
        })
    }

    /// Resolve a `<rev>` or `<from>..<to>` spec to the commit OIDs it
    /// covers (newest first, revwalk order). Public so callers can show
    /// counts / ask for confirmation before a destructive pass.
    pub fn resolve_commit_range(&self, target: &str) -> Result<Vec<String>> {
        Ok(self
            .resolve_range_oids(target)?
            .iter()
            .map(|o| o.to_string())
            .collect())
    }

    fn resolve_range_oids(&self, target: &str) -> Result<Vec<git2::Oid>> {
        let r = &self.repo;
        if let Some((from, to)) = target.split_once("..") {
            let from_oid = r.revparse_single(from)?.id();
            let to_oid = r.revparse_single(to)?.id();
            let mut walk = r.revwalk()?;
            walk.push(to_oid)?;
            walk.hide(from_oid)?;
            Ok(walk.flatten().collect())
        } else {
            Ok(vec![r.revparse_single(target)?.id()])
        }
    }

    /// Render the signature armor each commit in the range *would* get,
    /// without rewriting anything. Returns `(oid, armor)` pairs.
    pub fn preview_signatures(
        &self,
        target: &str,
        key: &str,
        gpg_program: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        let r = &self.repo;
        let mut out = Vec::new();
        for oid in self.resolve_range_oids(target)? {
            let commit = r.find_commit(oid)?;
            let buffer = r.commit_create_buffer(
                &commit.author(),
                &commit.committer(),
                commit.message().unwrap_or(""),
                &commit.tree()?,
                &commit
                    .parents()
                    .collect::<Vec<_>>()
                    .iter()
                    .collect::<Vec<_>>(),
            )?;
            let armor = crate::gpg::sign_blob(&buffer, key, gpg_program)?;
            out.push((oid.to_string(), armor));
        }
        Ok(out)
    }

    /// Rewrite every commit in the range with a fresh `gpgsig` header and
    /// move affected local branch tips onto the rewritten OIDs. Refuses to
    /// run on a dirty work tree — rewriting history with uncommitted
    /// changes makes the resulting state hard to reason about.
    pub fn sign_range(
        &self,
        target: &str,
        key: &str,
        gpg_program: Option<&str>,
    ) -> Result<SignOutcome> {
        let r = &self.repo;

        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(false);
        if !r.statuses(Some(&mut opts))?.is_empty() {
            return Err(ToriiError::RepoState(
                "working tree is dirty — commit or stash first. (`torii sign` rewrites \
                 history; running with uncommitted changes makes the resulting state \
                 hard to reason about.)"
                    .to_string(),
            ));
        }

        let oids = self.resolve_range_oids(target)?;

        // Walk the range oldest-first so each rewrite's child can reuse
        // the parent's new OID instead of the original.
        let mut ordered = oids.clone();
        ordered.reverse();
        let mut remap: std::collections::HashMap<git2::Oid, git2::Oid> =
            std::collections::HashMap::new();
        let mut rewritten = Vec::new();

        for oid in &ordered {
            let commit = r.find_commit(*oid)?;
            let parents: Vec<git2::Commit> = commit
                .parents()
                .map(|p| {
                    let real = remap.get(&p.id()).copied().unwrap_or(p.id());
                    r.find_commit(real).map_err(ToriiError::Git)
                })
                .collect::<Result<Vec<_>>>()?;
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
            let tree = commit.tree()?;
            let buffer = r.commit_create_buffer(
                &commit.author(),
                &commit.committer(),
                commit.message().unwrap_or(""),
                &tree,
                &parent_refs,
            )?;
            let buffer_str = std::str::from_utf8(&buffer)
                .map_err(|e| ToriiError::RepoState(format!("commit buffer is not UTF-8: {}", e)))?;
            let armor = crate::gpg::sign_blob(&buffer, key, gpg_program)?;
            let new_oid = r.commit_signed(buffer_str, &armor, Some("gpgsig"))?;
            remap.insert(*oid, new_oid);
            rewritten.push(SignedRewrite {
                old: oid.to_string(),
                new: new_oid.to_string(),
            });
        }

        // Move every local branch whose tip is one of the rewritten oids
        // onto the new oid.
        let mut moved = 0usize;
        for b in r.branches(Some(git2::BranchType::Local))?.flatten() {
            let (br, _) = b;
            let tip = br.get().target();
            if let (Some(t), Some(name)) = (tip, br.name().ok().flatten()) {
                if let Some(new_oid) = remap.get(&t) {
                    r.reference(
                        &format!("refs/heads/{}", name),
                        *new_oid,
                        true,
                        "torii sign — re-sign history",
                    )?;
                    moved += 1;
                }
            }
        }

        Ok(SignOutcome {
            rewritten,
            branches_moved: moved,
        })
    }
}
