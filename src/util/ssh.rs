use std::path::PathBuf;
use std::fs;
use crate::error::Result;
use dirs;

/// SSH key detection and management
pub struct SshHelper;

impl SshHelper {
    /// Check if user has SSH keys configured
    pub fn has_ssh_keys() -> bool {
        let ssh_dir = Self::ssh_dir();
        if !ssh_dir.exists() {
            return false;
        }

        // Check for common SSH key files
        let key_files = vec![
            "id_rsa",
            "id_ed25519",
            "id_ecdsa",
            "id_dsa",
        ];

        key_files.iter().any(|key| {
            ssh_dir.join(key).exists()
        })
    }

    /// Get SSH directory path
    pub fn ssh_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".ssh")
    }

    /// List available SSH keys
    pub fn list_keys() -> Vec<String> {
        let ssh_dir = Self::ssh_dir();
        if !ssh_dir.exists() {
            return vec![];
        }

        let key_files = vec![
            "id_rsa",
            "id_ed25519",
            "id_ecdsa",
            "id_dsa",
        ];

        key_files
            .iter()
            .filter(|key| ssh_dir.join(key).exists())
            .map(|s| s.to_string())
            .collect()
    }

    /// Get public key content for display
    #[allow(dead_code)]
    pub fn get_public_key(key_name: &str) -> Result<String> {
        let ssh_dir = Self::ssh_dir();
        let pub_key_path = ssh_dir.join(format!("{}.pub", key_name));
        
        if !pub_key_path.exists() {
            return Ok(String::new());
        }

        Ok(fs::read_to_string(pub_key_path)?)
    }

    /// Recommend protocol based on SSH availability
    #[allow(dead_code)]
    pub fn recommend_protocol() -> &'static str {
        if Self::has_ssh_keys() {
            "ssh"
        } else {
            "https"
        }
    }

    /// Get setup instructions for SSH
    #[allow(dead_code)]
    pub fn get_setup_instructions() -> String {
        let mut instructions = String::new();
        
        instructions.push_str("📚 SSH Setup Instructions:\n\n");
        instructions.push_str("1. Generate SSH key:\n");
        instructions.push_str("   ssh-keygen -t ed25519 -C \"your_email@example.com\"\n\n");
        instructions.push_str("2. Start SSH agent:\n");
        instructions.push_str("   eval \"$(ssh-agent -s)\"\n");
        instructions.push_str("   ssh-add ~/.ssh/id_ed25519\n\n");
        instructions.push_str("3. Copy public key:\n");
        instructions.push_str("   cat ~/.ssh/id_ed25519.pub\n\n");
        instructions.push_str("4. Add to your platform:\n");
        instructions.push_str("   • GitHub: https://github.com/settings/keys\n");
        instructions.push_str("   • GitLab: https://gitlab.com/-/profile/keys\n");
        instructions.push_str("   • Bitbucket: https://bitbucket.org/account/settings/ssh-keys/\n");
        
        instructions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_dir() {
        let dir = SshHelper::ssh_dir();
        assert!(dir.to_string_lossy().contains(".ssh"));
    }

    #[test]
    fn test_recommend_protocol() {
        let protocol = SshHelper::recommend_protocol();
        assert!(protocol == "ssh" || protocol == "https");
    }
}
