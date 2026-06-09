use crate::core::GitRepo;
use crate::tag::TagManager;
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
        let oid = git2::Oid::from_str(commit_hash)?;
        let commit = self.repo.find_commit(oid)?;

        println!("🍒 Cherry-picking commit: {}", commit.id());

        // Perform cherry-pick
        self.repo.cherrypick(&commit, None)?;

        // Check for conflicts
        let mut index = self.repo.index()?;
        if index.has_conflicts() {
            println!("⚠️  Conflicts detected!");
            println!("💡 Resolve conflicts and run: torii cherry-pick --continue");
            return Ok(());
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

        println!("✅ Cherry-pick complete");

        Ok(())
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
