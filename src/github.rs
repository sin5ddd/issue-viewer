use crate::model::{IssueRow, IssueState};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GqlResponse {
    data: Option<GqlData>,
}

#[derive(Debug, Deserialize)]
struct GqlData {
    repository: Option<GqlRepo>,
}

#[derive(Debug, Deserialize)]
struct GqlRepo {
    issues: GqlIssues,
}

#[derive(Debug, Deserialize)]
struct GqlIssues {
    nodes: Vec<GqlIssue>,
    #[serde(rename = "pageInfo")]
    page_info: GqlPage,
}

#[derive(Debug, Deserialize)]
struct GqlPage {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GqlIssue {
    number: u64,
    title: String,
    state: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    parent: Option<GqlParent>,
}

#[derive(Debug, Deserialize)]
struct GqlParent {
    number: u64,
}

pub fn rows_from_graphql_json(
    json: &str,
) -> Result<(Vec<IssueRow>, bool, Option<String>), serde_json::Error> {
    let parsed: GqlResponse = serde_json::from_str(json)?;
    let Some(issues) = parsed.data.and_then(|d| d.repository).map(|r| r.issues) else {
        return Ok((Vec::new(), false, None));
    };
    let rows = issues
        .nodes
        .into_iter()
        .map(|n| IssueRow {
            number: n.number,
            title: n.title,
            state: if n.state.eq_ignore_ascii_case("CLOSED") {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            parent_number: n.parent.map(|p| p.number),
            updated_at: n.updated_at,
        })
        .collect();
    Ok((
        rows,
        issues.page_info.has_next_page,
        issues.page_info.end_cursor,
    ))
}

pub const ISSUES_QUERY: &str = r#"
query($owner: String!, $name: String!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    issues(first: 100, after: $cursor, states: OPEN, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number
        title
        state
        updatedAt
        parent { number }
      }
    }
  }
}
"#;

#[derive(Debug, thiserror::Error)]
pub enum GitHubError {
    #[error("http: {0}")]
    Http(String),
    #[error("auth required")]
    Auth,
}

#[derive(Clone, Debug)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

pub trait GitHubClient {
    fn list_repos(&self) -> Result<Vec<RepoRef>, GitHubError>;
    fn fetch_issue_page(
        &self,
        owner: &str,
        repo: &str,
        cursor: Option<&str>,
    ) -> Result<(Vec<IssueRow>, bool, Option<String>), GitHubError>;
}

pub struct LiveClient {
    pub token: String,
    http: reqwest::blocking::Client,
}

impl LiveClient {
    pub fn new(token: String) -> Self {
        Self {
            token,
            http: reqwest::blocking::Client::new(),
        }
    }
}

impl GitHubClient for LiveClient {
    fn list_repos(&self) -> Result<Vec<RepoRef>, GitHubError> {
        let url = "https://api.github.com/user/repos?per_page=100&sort=updated&affiliation=owner,collaborator,organization_member";
        let resp = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "issue-viewer")
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GitHubError::Auth);
        }
        #[derive(Deserialize)]
        struct R {
            name: String,
            owner: ROwner,
        }
        #[derive(Deserialize)]
        struct ROwner {
            login: String,
        }
        let repos: Vec<R> = resp.json().map_err(|e| GitHubError::Http(e.to_string()))?;
        Ok(repos
            .into_iter()
            .map(|r| RepoRef {
                owner: r.owner.login,
                name: r.name,
            })
            .collect())
    }

    fn fetch_issue_page(
        &self,
        owner: &str,
        repo: &str,
        cursor: Option<&str>,
    ) -> Result<(Vec<IssueRow>, bool, Option<String>), GitHubError> {
        let body = serde_json::json!({
            "query": ISSUES_QUERY,
            "variables": { "owner": owner, "name": repo, "cursor": cursor },
        });
        let resp = self
            .http
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "issue-viewer")
            .json(&body)
            .send()
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        let text = resp.text().map_err(|e| GitHubError::Http(e.to_string()))?;
        rows_from_graphql_json(&text).map_err(|e| GitHubError::Http(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parent_number() {
        let json = r#"{
          "data": {
            "repository": {
              "issues": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": [
                  {
                    "number": 1,
                    "title": "parent",
                    "state": "OPEN",
                    "updatedAt": "2026-01-01T00:00:00Z",
                    "parent": null
                  },
                  {
                    "number": 2,
                    "title": "child",
                    "state": "OPEN",
                    "updatedAt": "2026-01-02T00:00:00Z",
                    "parent": { "number": 1 }
                  }
                ]
              }
            }
          }
        }"#;
        let (rows, more, _) = rows_from_graphql_json(json).unwrap();
        assert!(!more);
        assert_eq!(rows[1].parent_number, Some(1));
    }
}
