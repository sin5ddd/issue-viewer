use crate::model::{IssueRow, IssueState};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GqlResponse {
    data: Option<GqlData>,
}

#[derive(Debug, Deserialize)]
struct GqlData {
    #[serde(rename = "rateLimit")]
    rate_limit: Option<GqlRate>,
    repository: Option<GqlRepo>,
}

#[derive(Debug, Deserialize)]
struct GqlRate {
    remaining: u32,
    #[serde(default)]
    cost: Option<u32>,
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
    #[serde(default)]
    body: String,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    parent: Option<GqlParent>,
}

#[derive(Debug, Deserialize)]
struct GqlParent {
    number: u64,
}

#[derive(Debug, Clone, Default)]
pub struct IssuePage {
    pub rows: Vec<IssueRow>,
    pub has_next: bool,
    pub end_cursor: Option<String>,
    pub rate_remaining: Option<u32>,
    pub rate_cost: Option<u32>,
}

pub fn rows_from_graphql_json(json: &str) -> Result<IssuePage, serde_json::Error> {
    let parsed: GqlResponse = serde_json::from_str(json)?;
    let (remaining, cost) = parsed
        .data
        .as_ref()
        .and_then(|d| d.rate_limit.as_ref())
        .map(|r| (Some(r.remaining), r.cost))
        .unwrap_or((None, None));
    let Some(issues) = parsed.data.and_then(|d| d.repository).map(|r| r.issues) else {
        return Ok(IssuePage {
            rate_remaining: remaining,
            rate_cost: cost,
            ..IssuePage::default()
        });
    };
    let rows = issues
        .nodes
        .into_iter()
        .map(|n| IssueRow {
            number: n.number,
            title: n.title,
            body: n.body,
            state: if n.state.eq_ignore_ascii_case("CLOSED") {
                IssueState::Closed
            } else {
                IssueState::Open
            },
            parent_number: n.parent.map(|p| p.number),
            created_at: n.created_at.unwrap_or_default(),
            updated_at: n.updated_at,
        })
        .collect();
    Ok(IssuePage {
        rows,
        has_next: issues.page_info.has_next_page,
        end_cursor: issues.page_info.end_cursor,
        rate_remaining: remaining,
        rate_cost: cost,
    })
}

pub const ISSUES_QUERY: &str = r#"
query($owner: String!, $name: String!, $cursor: String, $since: DateTime) {
  rateLimit { remaining resetAt cost }
  repository(owner: $owner, name: $name) {
    issues(first: 100, after: $cursor, states: [OPEN, CLOSED], filterBy: { since: $since }, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo { hasNextPage endCursor }
      nodes {
        number
        title
        body
        state
        createdAt
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
        since: Option<&str>,
    ) -> Result<IssuePage, GitHubError>;
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
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(GitHubError::Http("rate_limited".into()));
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
        since: Option<&str>,
    ) -> Result<IssuePage, GitHubError> {
        let body = serde_json::json!({
            "query": ISSUES_QUERY,
            "variables": { "owner": owner, "name": repo, "cursor": cursor, "since": since },
        });
        let resp = self
            .http
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "issue-viewer")
            .json(&body)
            .send()
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GitHubError::Auth);
        }
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(GitHubError::Http("rate_limited".into()));
        }
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
            "rateLimit": { "remaining": 4990, "cost": 1 },
            "repository": {
              "issues": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": [
                  {
                    "number": 1,
                    "title": "parent",
                    "body": "hello",
                    "state": "OPEN",
                    "createdAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-01T00:00:00Z",
                    "parent": null
                  },
                  {
                    "number": 2,
                    "title": "child",
                    "body": "world",
                    "state": "CLOSED",
                    "createdAt": "2026-01-02T00:00:00Z",
                    "updatedAt": "2026-01-02T00:00:00Z",
                    "parent": { "number": 1 }
                  }
                ]
              }
            }
          }
        }"#;
        let page = rows_from_graphql_json(json).unwrap();
        assert!(!page.has_next);
        assert_eq!(page.rate_remaining, Some(4990));
        assert_eq!(page.rate_cost, Some(1));
        assert_eq!(page.rows[1].parent_number, Some(1));
        assert_eq!(page.rows[1].body, "world");
        assert_eq!(page.rows[1].state, IssueState::Closed);
        assert_eq!(page.rows[0].created_at, "2026-01-01T00:00:00Z");
    }
}
