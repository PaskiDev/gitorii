use crate::core::GitRepo;
use crate::tag::TagManager;
use crate::vcs::conflict::{
    resolve_with, ConflictContext, ConflictResolver, IntegrateOutcome, ManualResolver, OpKind,
};
use anyhow::Result;
use git2;

impl GitRepo {
    /// Create a tag
    pub fn create_tag(&self, name: &str, message: Option<&str>) -> Result<()> {
        let tag_mgr = TagManager::new(&self.repo);
        tag_mgr.create_tag(name, message)?;
        Ok(())
    }

    /// List all tags
    pub fn list_tags(&self) -> Result<()> {
        let tag_mgr = TagManager::new(&self.repo);
        let tags = tag_mgr.list_tags()?;

        if tags.is_empty() {
            println!("No tags found");
        } else {
            println!("📌 Tags:");
            for tag in tags {
                println!("  {}", tag);
            }
        }

        Ok(())
    }

    /// Get tags as a vector (for internal use)
    pub fn get_tags_list(&self) -> Result<Vec<String>> {
        let tag_mgr = TagManager::new(&self.repo);
        Ok(tag_mgr.list_tags()?)
    }

    /// Delete a tag
    pub fn delete_tag(&self, name: &str) -> Result<()> {
        let tag_mgr = TagManager::new(&self.repo);
        tag_mgr.delete_tag(name)?;
        Ok(())
    }

    /// Push tags to remote. When `force` is true, the refspec is prefixed
    /// with `+` so the remote tag is replaced even if it already points at
    /// a different commit — same semantics as `git push --force-with-lease`
    /// /`--force` for tags, and matches the behaviour of `--force` on the
    /// `push_all_tags` path.
    pub fn push_tags(&self, name: Option<&str>, force: bool) -> Result<()> {
        if let Some(tag) = name {
            // Push a specific tag. Force-push uses `+oldref:newref` to
            // tell the remote "replace this regardless of fast-forward".
            let prefix = if force { "+" } else { "" };
            let refspec = format!("{0}refs/tags/{1}:refs/tags/{1}", prefix, tag);
            let mut remote = self.repo.find_remote("origin")?;
            let remote_url = remote.url().unwrap_or("").to_string();
            let callbacks = Self::auth_callbacks_for(&remote_url);
            let mut push_options = git2::PushOptions::new();
            push_options.remote_callbacks(callbacks);
            remote.push(&[refspec.as_str()], Some(&mut push_options))?;
        } else {
            // Push all tags — push_all_tags already supports force.
            self.push_all_tags("origin", force)?;
        }
        Ok(())
    }

    /// Show tag details
    pub fn show_tag(&self, name: &str) -> Result<()> {
        let tag_mgr = TagManager::new(&self.repo);
        tag_mgr.show_tag(name)?;
        Ok(())
    }

    /// Cherry-pick a commit
    pub fn cherry_pick(&self, commit_hash: &str) -> Result<()> {
        self.cherry_pick_with(commit_hash, &ManualResolver)?;
        Ok(())
    }

    /// Like [`cherry_pick`](Self::cherry_pick), but asks `r` how to resolve
    /// conflicts and commits when none remain.
    pub fn cherry_pick_with(
        &self,
        commit_hash: &str,
        r: &dyn ConflictResolver,
    ) -> Result<IntegrateOutcome> {
        let oid = git2::Oid::from_str(commit_hash)?;
        let commit = self.repo.find_commit(oid)?;

        println!("🍒 Cherry-picking commit: {}", commit.id());

        // Perform cherry-pick
        self.repo.cherrypick(&commit, None)?;

        // Check for conflicts
        let mut index = self.repo.index()?;
        if index.has_conflicts() {
            let id = commit.id().to_string();
            let ctx = ConflictContext {
                op: OpKind::CherryPick,
                local_label: self
                    .get_current_branch()
                    .unwrap_or_else(|_| "HEAD".to_string()),
                remote_label: format!("{} {}", &id[..7], commit.summary().unwrap_or("")),
            };
            let left = resolve_with(&self.repo, &ctx, r)?;
            if left > 0 {
                println!("⚠️  Conflicts detected!");
                println!("💡 Resolve conflicts and run: torii cherry-pick --continue");
                return Ok(IntegrateOutcome::Conflicts(left));
            }
            index = self.repo.index()?;
        }

        // Create commit
        let sig = crate::core::resolve_signature(&self.repo)?;
        let tree_oid = index.write_tree()?;
        let tree = self.repo.find_tree(tree_oid)?;
        let head = self.repo.head()?.peel_to_commit()?;

        crate::core::commit_inner(
            &self.repo,
            Some("HEAD"),
            &sig,
            commit.message().unwrap_or("Cherry-picked commit"),
            &tree,
            &[&head],
        )?;
        // Leave no CHERRY_PICK_HEAD behind once the commit exists.
        self.repo.cleanup_state()?;

        println!("✅ Cherry-pick complete");

        Ok(IntegrateOutcome::Done)
    }

    /// Continue cherry-pick after resolving conflicts
    pub fn cherry_pick_continue(&self) -> Result<()> {
        println!("🔄 Continuing cherry-pick...");

        let sig = crate::core::resolve_signature(&self.repo)?;
        let mut index = self.repo.index()?;
        let tree_oid = index.write_tree()?;
        let tree = self.repo.find_tree(tree_oid)?;
        let head = self.repo.head()?.peel_to_commit()?;

        crate::core::commit_inner(
            &self.repo,
            Some("HEAD"),
            &sig,
            "Cherry-picked commit",
            &tree,
            &[&head],
        )?;

        println!("✅ Cherry-pick complete");

        Ok(())
    }

    /// Abort cherry-pick
    pub fn cherry_pick_abort(&self) -> Result<()> {
        println!("❌ Aborting cherry-pick...");

        // Reset to HEAD
        let head = self.repo.head()?.peel_to_commit()?;
        self.repo
            .reset(head.as_object(), git2::ResetType::Hard, None)?;

        println!("✅ Cherry-pick aborted");

        Ok(())
    }

    /// Show blame for a file
    pub fn blame(&self, file: &str, lines: Option<&str>) -> Result<()> {
        let blame = self.repo.blame_file(std::path::Path::new(file), None)?;

        let (start_line, end_line) = if let Some(range) = lines {
            let parts: Vec<&str> = range.split(',').collect();
            if parts.len() == 2 {
                let start = parts[0].parse::<usize>().unwrap_or(1);
                let end = parts[1].parse::<usize>().unwrap_or(usize::MAX);
                (start, end)
            } else {
                (1, usize::MAX)
            }
        } else {
            (1, usize::MAX)
        };

        println!("📝 Blame for: {}", file);
        println!();

        for (idx, hunk) in blame.iter().enumerate() {
            let line_num = idx + 1;

            if line_num < start_line || line_num > end_line {
                continue;
            }

            let commit_id = hunk.final_commit_id();
            let commit = self.repo.find_commit(commit_id)?;
            let author = commit.author();
            let short_id = format!("{:.7}", commit_id);

            println!(
                "{} {} ({}) {}",
                short_id,
                author.name().unwrap_or("Unknown"),
                line_num,
                commit.message().unwrap_or("").lines().next().unwrap_or("")
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod cherry_pick_resolver_tests {
    use super::*;
    use crate::vcs::conflict::{FixedResolver, IntegrateOutcome, Side};
    use tempfile::TempDir;

    #[test]
    fn cherry_pick_with_remote_resolver_commits() {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let s = git2::Signature::now("t", "t@example.com").unwrap();
        let commit = |content: &str, msg: &str, parents: &[git2::Oid]| {
            std::fs::write(dir.path().join("f.txt"), content).unwrap();
            let mut idx = repo.index().unwrap();
            idx.add_path(std::path::Path::new("f.txt")).unwrap();
            idx.write().unwrap();
            let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
            let pcs: Vec<git2::Commit> = parents
                .iter()
                .map(|&o| repo.find_commit(o).unwrap())
                .collect();
            let prefs: Vec<&git2::Commit> = pcs.iter().collect();
            repo.commit(Some("HEAD"), &s, &s, msg, &tree, &prefs)
                .unwrap()
        };
        let base = commit("base\n", "base", &[]);
        let picked = commit("picked\n", "picked", &[base]);
        repo.reset(
            repo.find_commit(base).unwrap().as_object(),
            git2::ResetType::Hard,
            None,
        )
        .unwrap();
        let ours = commit("ours\n", "ours", &[base]);
        drop(repo);

        let gr = GitRepo::open(dir.path()).unwrap();
        let out = gr
            .cherry_pick_with(&picked.to_string(), &FixedResolver(Side::Remote))
            .unwrap();
        assert_eq!(out, IntegrateOutcome::Done);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
            "picked\n"
        );
        let head = gr.repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent_count(), 1);
        assert_eq!(head.parent(0).unwrap().id(), ours);
        assert_eq!(head.message(), Some("picked"));
        assert_eq!(gr.repo.state(), git2::RepositoryState::Clean);
    }
}
