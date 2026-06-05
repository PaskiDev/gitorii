//! Azure DevOps — issue client.

use crate::error::{Result, ToriiError};
use crate::platforms::issue::*;
use reqwest::blocking::Client;

pub struct AzureIssueClient {
    token: String,
}

impl AzureIssueClient {
    pub fn new() -> Result<Self> {
        let token = crate::auth::resolve_token("azure", ".").value
            .ok_or_else(|| ToriiError::Auth { provider: "azure".into(), message: "Azure DevOps PAT not found. Create at https://dev.azure.com/{org}/_usersSettings/tokens \
                 with `Work Items (read/write)` scope, then: torii auth set azure YOUR_PAT".to_string() })?;
        Ok(Self { token })
    }

    fn client(&self) -> Client {
        crate::http::make_client()
    }

    fn auth(&self) -> String {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!(":{}", self.token));
        format!("Basic {}", b64)
    }
}

impl IssueClient for AzureIssueClient {
    fn list(&self, owner: &str, _repo: &str, state: &str) -> Result<Vec<Issue>> {
        // Azure Issues are project-scoped, not repo-scoped — we ignore
        // `repo` here. The WIQL query filters by State.
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        let state_filter = match state {
            "open" => {
                r#"[System.State] <> 'Closed' AND [System.State] <> 'Resolved' AND [System.State] <> 'Done' AND [System.State] <> 'Removed'"#
            }
            "closed" => {
                r#"([System.State] = 'Closed' OR [System.State] = 'Resolved' OR [System.State] = 'Done')"#
            }
            _ => "[System.Id] > 0", // dummy always-true
        };
        let query = format!(
            "SELECT [System.Id] FROM workitems WHERE [System.TeamProject] = '{}' AND {} ORDER BY [System.Id] DESC",
            project, state_filter
        );

        // Step 1: WIQL query → list of IDs.
        let wiql_url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/wiql?api-version=7.0&$top=50",
            org, project
        );
        let wiql_req = self
            .client()
            .post(&wiql_url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&serde_json::json!({ "query": query }));
        let wiql_json = crate::http::send_json(wiql_req, "Azure WIQL")?;
        let ids: Vec<u64> = wiql_json["workItems"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v["id"].as_u64()).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            return Ok(vec![]);
        }

        // Step 2: batch GET work items by id.
        let ids_csv = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fields = "System.Id,System.Title,System.Description,System.State,\
                      System.CreatedBy,System.CreatedDate,System.AssignedTo,System.Tags";
        let wi_url = format!(
            "https://dev.azure.com/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.0",
            org, ids_csv, fields
        );
        let wi_req = self
            .client()
            .get(&wi_url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json");
        let wi_json = crate::http::send_json(wi_req, "Azure get work items")?;
        let arr = wi_json["value"]
            .as_array()
            .ok_or_else(|| ToriiError::MalformedResponse {
                provider: "azure".into(),
                message: format!("Azure returned no `value` array. Body: {}", wi_json),
            })?;
        let org_for_url = org.clone();
        Ok(arr
            .iter()
            .filter_map(|v| parse_azure_work_item(v, &org_for_url).ok())
            .collect())
    }

    fn create(&self, owner: &str, _repo: &str, opts: CreateIssueOptions) -> Result<Issue> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        // POST body is JSON-Patch — the Content-Type matters.
        let mut ops = vec![
            serde_json::json!({ "op": "add", "path": "/fields/System.Title", "value": opts.title }),
        ];
        if let Some(b) = opts.body {
            ops.push(serde_json::json!({ "op": "add", "path": "/fields/System.Description", "value": b }));
        }
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems/$Issue?api-version=7.0",
            org, project
        );
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Content-Type", "application/json-patch+json")
            .header("Accept", "application/json")
            .json(&serde_json::Value::Array(ops));
        let json = crate::http::send_json(req, "Azure create work item")?;
        parse_azure_work_item(&json, &org)
    }

    fn close(&self, owner: &str, _repo: &str, number: u64) -> Result<()> {
        let (org, _project) = crate::platforms::pr::split_azure_owner(owner)?;
        let url = format!(
            "https://dev.azure.com/{}/_apis/wit/workitems/{}?api-version=7.0",
            org, number
        );
        let body = serde_json::json!([
            { "op": "add", "path": "/fields/System.State", "value": "Closed" }
        ]);
        let req = self
            .client()
            .patch(&url)
            .header("Authorization", self.auth())
            .header("Content-Type", "application/json-patch+json")
            .header("Accept", "application/json")
            .json(&body);
        crate::http::send_empty(req, "Azure close work item")
    }

    fn comment(&self, owner: &str, _repo: &str, number: u64, body: &str) -> Result<()> {
        let (org, project) = crate::platforms::pr::split_azure_owner(owner)?;
        // Comments endpoint is still preview as of api-version 7.1.
        let url = format!(
            "https://dev.azure.com/{}/{}/_apis/wit/workitems/{}/comments?api-version=7.1-preview.3",
            org, project, number
        );
        let payload = serde_json::json!({ "text": body });
        let req = self
            .client()
            .post(&url)
            .header("Authorization", self.auth())
            .header("Accept", "application/json")
            .json(&payload);
        crate::http::send_empty(req, "Azure comment work item")
    }
}

fn parse_azure_work_item(json: &serde_json::Value, org: &str) -> Result<Issue> {
    let id = json["id"].as_u64().unwrap_or(0);
    let fields = &json["fields"];
    let state_raw = fields["System.State"].as_str().unwrap_or("");
    let project = fields["System.TeamProject"].as_str().unwrap_or("");
    Ok(Issue {
        number: id,
        title: fields["System.Title"].as_str().unwrap_or("").to_string(),
        body: fields["System.Description"].as_str().map(String::from),
        state: match state_raw {
            "New" | "Active" | "Open" | "Approved" | "To Do" | "Committed" | "In Progress" => {
                "open".to_string()
            }
            "Closed" | "Resolved" | "Done" | "Removed" => "closed".to_string(),
            other => other.to_string(),
        },
        author: fields["System.CreatedBy"]["displayName"]
            .as_str()
            .or_else(|| fields["System.CreatedBy"].as_str())
            .unwrap_or("")
            .to_string(),
        url: if !project.is_empty() {
            format!(
                "https://dev.azure.com/{}/{}/_workitems/edit/{}",
                org, project, id
            )
        } else {
            json["url"].as_str().unwrap_or("").to_string()
        },
        labels: fields["System.Tags"]
            .as_str()
            .map(|s| {
                s.split(';')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        assignees: fields["System.AssignedTo"]["displayName"]
            .as_str()
            .or_else(|| fields["System.AssignedTo"].as_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        created_at: fields["System.CreatedDate"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        comments: 0,
    })
}

// ── Factory ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_azure_work_item_full() {
        let json = serde_json::json!({
            "id": 7u64,
            "fields": {
                "System.Title": "Crash on startup",
                "System.Description": "<div>boom</div>",
                "System.State": "Active",
                "System.TeamProject": "proj",
                "System.CreatedBy": { "displayName": "Jane" },
                "System.AssignedTo": { "displayName": "Bob" },
                "System.Tags": "bug; ui ; ",
                "System.CreatedDate": "2026-01-01T00:00:00Z",
            },
        });
        let issue = parse_azure_work_item(&json, "org").unwrap();
        assert_eq!(issue.number, 7);
        assert_eq!(issue.title, "Crash on startup");
        assert_eq!(issue.body.as_deref(), Some("<div>boom</div>"));
        assert_eq!(issue.state, "open");
        assert_eq!(issue.author, "Jane");
        assert_eq!(
            issue.url,
            "https://dev.azure.com/org/proj/_workitems/edit/7"
        );
        // Tags split on `;`, trimmed, empties dropped.
        assert_eq!(issue.labels, vec!["bug".to_string(), "ui".to_string()]);
        assert_eq!(issue.assignees, vec!["Bob".to_string()]);
        assert_eq!(issue.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(issue.comments, 0);
    }

    #[test]
    fn parse_azure_work_item_state_mapping() {
        for (az, ours) in [
            ("New", "open"),
            ("Active", "open"),
            ("To Do", "open"),
            ("In Progress", "open"),
            ("Closed", "closed"),
            ("Resolved", "closed"),
            ("Done", "closed"),
            ("Removed", "closed"),
            ("Blocked", "Blocked"), // unknown states pass through raw
        ] {
            let json = serde_json::json!({ "fields": { "System.State": az } });
            assert_eq!(parse_azure_work_item(&json, "org").unwrap().state, ours);
        }
    }

    #[test]
    fn parse_azure_work_item_url_falls_back_without_project() {
        let json = serde_json::json!({
            "id": 3u64,
            "url": "https://dev.azure.com/org/_apis/wit/workItems/3",
            "fields": {},
        });
        let issue = parse_azure_work_item(&json, "org").unwrap();
        assert_eq!(issue.url, "https://dev.azure.com/org/_apis/wit/workItems/3");
    }

    #[test]
    fn parse_azure_work_item_identity_string_fallbacks() {
        // Older API shapes return identities as plain strings rather
        // than `{ displayName }` objects.
        let json = serde_json::json!({
            "fields": {
                "System.CreatedBy": "jane@example.com",
                "System.AssignedTo": "bob@example.com",
            },
        });
        let issue = parse_azure_work_item(&json, "org").unwrap();
        assert_eq!(issue.author, "jane@example.com");
        assert_eq!(issue.assignees, vec!["bob@example.com".to_string()]);
    }

    #[test]
    fn parse_azure_work_item_minimal_defaults() {
        let json = serde_json::json!({});
        let issue = parse_azure_work_item(&json, "org").unwrap();
        assert_eq!(issue.number, 0);
        assert_eq!(issue.title, "");
        assert_eq!(issue.body, None);
        assert!(issue.labels.is_empty());
        assert!(issue.assignees.is_empty());
        assert_eq!(issue.created_at, "");
    }
}
