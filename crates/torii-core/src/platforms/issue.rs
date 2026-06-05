use super::azure::AzureIssueClient;
use super::bitbucket::BitbucketIssueClient;
use super::gitea::GiteaIssueClient;
use super::github::GitHubIssueClient;
use super::gitlab::GitLabIssueClient;
use super::radicle::RadicleIssueClient;
use super::sourcehut::SourcehutIssueClient;
use crate::error::{Result, ToriiError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub author: String,
    pub url: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub created_at: String,
    pub comments: u64,
}

#[derive(Debug, Clone)]
pub struct CreateIssueOptions {
    pub title: String,
    pub body: Option<String>,
}

pub trait IssueClient: Send {
    fn list(&self, owner: &str, repo: &str, state: &str) -> Result<Vec<Issue>>;
    fn create(&self, owner: &str, repo: &str, opts: CreateIssueOptions) -> Result<Issue>;
    fn close(&self, owner: &str, repo: &str, number: u64) -> Result<()>;
    fn comment(&self, owner: &str, repo: &str, number: u64, body: &str) -> Result<()>;
}

// ── GitHub ────────────────────────────────────────────────────────────────────

pub fn get_issue_client(platform: &str) -> Result<Box<dyn IssueClient>> {
    match platform.to_lowercase().as_str() {
        "github"    => Ok(Box::new(GitHubIssueClient::new()?)),
        "gitlab"    => Ok(Box::new(GitLabIssueClient::new()?)),
        "gitea"     => Ok(Box::new(GiteaIssueClient::new()?)),
        "sourcehut" => Ok(Box::new(SourcehutIssueClient::new()?)),
        "radicle"   => Ok(Box::new(RadicleIssueClient::new()?)),
        "bitbucket" => Ok(Box::new(BitbucketIssueClient::new()?)),
        "azure"     => Ok(Box::new(AzureIssueClient::new()?)),
        other => Err(ToriiError::Unsupported(format!("Unsupported platform: {}. Supported: github, gitlab, gitea, sourcehut, radicle, bitbucket, azure", other))),
    }
}
