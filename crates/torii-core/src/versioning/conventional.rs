use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq)]
pub enum CommitType {
    Feat,
    Fix,
    Docs,
    Style,
    Refactor,
    Perf,
    Test,
    Chore,
    Build,
    Ci,
    Revert,
    #[allow(dead_code)]
    Breaking,
}

impl CommitType {
    #[allow(dead_code)]
    pub fn should_create_tag(&self) -> bool {
        matches!(self, 
            CommitType::Feat | 
            CommitType::Fix | 
            CommitType::Perf | 
            CommitType::Breaking
        )
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ConventionalCommit {
    pub commit_type: CommitType,
    pub scope: Option<String>,
    pub description: String,
    pub body: Option<String>,
    pub breaking: bool,
}

impl ConventionalCommit {
    /// Parse a commit message following Conventional Commits specification
    /// Format: <type>(<scope>): <description>
    /// 
    /// Examples:
    /// - feat: add user authentication
    /// - fix(auth): resolve login bug
    /// - feat!: breaking change
    /// - BREAKING CHANGE: new API
    pub fn parse(message: &str) -> Result<Self> {
        let message = message.trim();
        
        // Check for BREAKING CHANGE in body
        let breaking_in_body = message.contains("BREAKING CHANGE:");
        
        // Get first line for parsing
        let first_line = message.lines().next().unwrap_or("");
        
        // Check for breaking change indicator (!)
        let breaking_indicator = first_line.contains('!');
        
        // Split type and rest
        let parts: Vec<&str> = first_line.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(anyhow!("Invalid commit format. Expected: <type>(<scope>): <description>"));
        }
        
        let type_part = parts[0].trim();
        let description = parts[1].trim().to_string();
        
        // Parse type and scope
        let (commit_type_str, scope) = if type_part.contains('(') {
            let type_scope: Vec<&str> = type_part.splitn(2, '(').collect();
            let scope_str = type_scope[1].trim_end_matches(')').trim_end_matches('!');
            (type_scope[0], Some(scope_str.to_string()))
        } else {
            (type_part.trim_end_matches('!'), None)
        };
        
        // Parse commit type
        let commit_type = match commit_type_str.to_lowercase().as_str() {
            "feat" => CommitType::Feat,
            "fix" => CommitType::Fix,
            "docs" => CommitType::Docs,
            "style" => CommitType::Style,
            "refactor" => CommitType::Refactor,
            "perf" => CommitType::Perf,
            "test" => CommitType::Test,
            "chore" => CommitType::Chore,
            "build" => CommitType::Build,
            "ci" => CommitType::Ci,
            "revert" => CommitType::Revert,
            _ => return Err(anyhow!("Unknown commit type: {}", commit_type_str)),
        };
        
        // Get body if exists
        let body = if message.lines().count() > 1 {
            Some(message.lines().skip(1).collect::<Vec<_>>().join("\n"))
        } else {
            None
        };
        
        // Determine if breaking change
        let breaking = breaking_indicator || breaking_in_body || commit_type_str == "BREAKING CHANGE";
        
        Ok(ConventionalCommit {
            commit_type,
            scope,
            description,
            body,
            breaking,
        })
    }
    
    pub fn is_breaking(&self) -> bool {
        self.breaking
    }
    
    #[allow(dead_code)]
    pub fn should_create_tag(&self) -> bool {
        self.commit_type.should_create_tag() || self.breaking
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_feat() {
        let commit = ConventionalCommit::parse("feat: add user authentication").unwrap();
        assert_eq!(commit.commit_type, CommitType::Feat);
        assert_eq!(commit.description, "add user authentication");
        assert_eq!(commit.scope, None);
        assert!(!commit.breaking);
    }
    
    #[test]
    fn test_parse_fix_with_scope() {
        let commit = ConventionalCommit::parse("fix(auth): resolve login bug").unwrap();
        assert_eq!(commit.commit_type, CommitType::Fix);
        assert_eq!(commit.description, "resolve login bug");
        assert_eq!(commit.scope, Some("auth".to_string()));
        assert!(!commit.breaking);
    }
    
    #[test]
    fn test_parse_breaking_with_indicator() {
        let commit = ConventionalCommit::parse("feat!: breaking change").unwrap();
        assert_eq!(commit.commit_type, CommitType::Feat);
        assert!(commit.breaking);
    }
    
    #[test]
    fn test_parse_breaking_in_body() {
        let commit = ConventionalCommit::parse("feat: new feature\n\nBREAKING CHANGE: API changed").unwrap();
        assert!(commit.breaking);
    }
}
