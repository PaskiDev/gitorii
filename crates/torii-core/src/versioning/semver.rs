use anyhow::{Result, anyhow};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VersionBump {
    Major,
    Minor,
    Patch,
    None,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }
    
    pub fn initial() -> Self {
        Self::new(0, 1, 0)
    }
    
    pub fn bump(&self, bump_type: VersionBump) -> Self {
        match bump_type {
            VersionBump::Major => Self::new(self.major + 1, 0, 0),
            VersionBump::Minor => Self::new(self.major, self.minor + 1, 0),
            VersionBump::Patch => Self::new(self.major, self.minor, self.patch + 1),
            VersionBump::None => *self,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = anyhow::Error;
    
    fn from_str(s: &str) -> Result<Self> {
        // Remove 'v' prefix if present
        let s = s.trim_start_matches('v');
        
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow!("Invalid version format. Expected: major.minor.patch"));
        }
        
        let major = parts[0].parse::<u32>()
            .map_err(|_| anyhow!("Invalid major version"))?;
        let minor = parts[1].parse::<u32>()
            .map_err(|_| anyhow!("Invalid minor version"))?;
        let patch = parts[2].parse::<u32>()
            .map_err(|_| anyhow!("Invalid patch version"))?;
        
        Ok(Self::new(major, minor, patch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_version_parse() {
        let v = "1.2.3".parse::<Version>().unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }
    
    #[test]
    fn test_version_parse_with_v() {
        let v = "v2.0.1".parse::<Version>().unwrap();
        assert_eq!(v.major, 2);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 1);
    }
    
    #[test]
    fn test_version_bump_major() {
        let v = Version::new(1, 2, 3);
        let bumped = v.bump(VersionBump::Major);
        assert_eq!(bumped, Version::new(2, 0, 0));
    }
    
    #[test]
    fn test_version_bump_minor() {
        let v = Version::new(1, 2, 3);
        let bumped = v.bump(VersionBump::Minor);
        assert_eq!(bumped, Version::new(1, 3, 0));
    }
    
    #[test]
    fn test_version_bump_patch() {
        let v = Version::new(1, 2, 3);
        let bumped = v.bump(VersionBump::Patch);
        assert_eq!(bumped, Version::new(1, 2, 4));
    }
    
    #[test]
    fn test_version_display() {
        let v = Version::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }
}
