use anyhow::Result;
use crate::versioning::conventional::{ConventionalCommit, CommitType};
use crate::versioning::semver::{Version, VersionBump};
use crate::core::GitRepo;
use git2;

pub struct AutoTagger {
    repo: GitRepo,
    prefix: String,
}

impl AutoTagger {
    pub fn new(repo: GitRepo) -> Self {
        Self {
            repo,
            prefix: "v".to_string(),
        }
    }
    
    #[allow(dead_code)]
    pub fn with_prefix(mut self, prefix: String) -> Self {
        self.prefix = prefix;
        self
    }
    
    /// Get the latest version tag from the repository
    pub fn get_latest_version(&self) -> Result<Option<Version>> {
        let tags = self.repo.get_tags_list()?;
        
        let mut versions: Vec<Version> = tags
            .iter()
            .filter_map(|tag| {
                tag.strip_prefix(&self.prefix)
                    .and_then(|v| v.parse::<Version>().ok())
            })
            .collect();
        
        versions.sort();
        Ok(versions.last().copied())
    }
    
    /// Determine the version bump type based on commit message
    pub fn determine_bump(&self, commit_msg: &str) -> Result<VersionBump> {
        let commit = ConventionalCommit::parse(commit_msg)?;
        
        if commit.is_breaking() {
            return Ok(VersionBump::Major);
        }
        
        match commit.commit_type {
            CommitType::Feat => Ok(VersionBump::Minor),
            CommitType::Fix | CommitType::Perf => Ok(VersionBump::Patch),
            _ => Ok(VersionBump::None),
        }
    }
    
    /// Calculate the next version based on commit message
    #[allow(dead_code)]
    pub fn calculate_next_version(&self, commit_msg: &str) -> Result<Option<Version>> {
        let bump = self.determine_bump(commit_msg)?;
        
        if bump == VersionBump::None {
            return Ok(None);
        }
        
        let current_version = self.get_latest_version()?
            .unwrap_or_else(Version::initial);
        
        Ok(Some(current_version.bump(bump)))
    }
    
    /// Create a tag for the given version
    pub fn create_tag(&self, version: &Version, message: &str) -> Result<()> {
        let tag_name = format!("{}{}", self.prefix, version);
        self.repo.create_tag(&tag_name, Some(message))?;
        println!("✅ Created tag: {}", tag_name);
        Ok(())
    }
    
    /// Calculate next version by scanning commits since the last tag
    pub fn calculate_next_version_from_log(&self) -> Result<Option<Version>> {
        let latest = self.get_latest_version()?;

        // Find the OID of the latest tag commit (if any)
        let since_oid: Option<git2::Oid> = if let Some(ref v) = latest {
            let tag_name = format!("{}{}", self.prefix, v);
            self.repo.repo.find_reference(&format!("refs/tags/{}", tag_name))
                .ok()
                .and_then(|r| r.peel_to_commit().ok())
                .map(|c| c.id())
        } else {
            None
        };

        // Walk commits from HEAD, stopping at the tag commit
        let mut revwalk = self.repo.repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TIME)?;

        let mut highest = VersionBump::None;

        for oid in revwalk.filter_map(|r| r.ok()) {
            // Stop when we reach the tagged commit
            if Some(oid) == since_oid {
                break;
            }
            if let Ok(commit) = self.repo.repo.find_commit(oid) {
                let msg = commit.summary().unwrap_or("");
                let bump = self.determine_bump(msg).unwrap_or(VersionBump::None);
                match bump {
                    VersionBump::Major => { highest = VersionBump::Major; break; }
                    VersionBump::Minor if highest != VersionBump::Major => { highest = VersionBump::Minor; }
                    VersionBump::Patch if highest == VersionBump::None => { highest = VersionBump::Patch; }
                    _ => {}
                }
            }
        }

        if highest == VersionBump::None {
            return Ok(None);
        }

        let base = latest.unwrap_or_else(Version::initial);
        Ok(Some(base.bump(highest)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_determine_bump_feat() {
        let repo = GitRepo::open(".").unwrap();
        let tagger = AutoTagger::new(repo);
        let bump = tagger.determine_bump("feat: add new feature").unwrap();
        assert_eq!(bump, VersionBump::Minor);
    }
    
    #[test]
    fn test_determine_bump_fix() {
        let repo = GitRepo::open(".").unwrap();
        let tagger = AutoTagger::new(repo);
        let bump = tagger.determine_bump("fix: resolve bug").unwrap();
        assert_eq!(bump, VersionBump::Patch);
    }
    
    #[test]
    fn test_determine_bump_breaking() {
        let repo = GitRepo::open(".").unwrap();
        let tagger = AutoTagger::new(repo);
        let bump = tagger.determine_bump("feat!: breaking change").unwrap();
        assert_eq!(bump, VersionBump::Major);
    }
    
    #[test]
    fn test_determine_bump_docs() {
        let repo = GitRepo::open(".").unwrap();
        let tagger = AutoTagger::new(repo);
        let bump = tagger.determine_bump("docs: update README").unwrap();
        assert_eq!(bump, VersionBump::None);
    }
}
