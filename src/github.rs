use crate::model::{IssueFieldValue, IssueLabel, IssueRow, IssueState, IssueTypeRef};
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
    #[serde(default)]
    labels: Option<GqlLabels>,
    #[serde(rename = "issueType", default)]
    issue_type: Option<GqlIssueType>,
    #[serde(rename = "issueFieldValues", default)]
    field_values: Option<GqlFieldValues>,
}

#[derive(Debug, Deserialize)]
struct GqlParent {
    number: u64,
}

#[derive(Debug, Deserialize)]
struct GqlLabels {
    #[serde(default)]
    nodes: Option<Vec<Option<GqlLabel>>>,
}

#[derive(Debug, Deserialize)]
struct GqlLabel {
    name: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GqlIssueType {
    name: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GqlFieldValues {
    #[serde(default)]
    nodes: Option<Vec<serde_json::Value>>,
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
            labels: labels_from(n.labels),
            issue_type: n.issue_type.and_then(issue_type_from),
            fields: fields_from(n.field_values),
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

fn labels_from(conn: Option<GqlLabels>) -> Vec<IssueLabel> {
    conn.and_then(|c| c.nodes)
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .filter(|l| !l.name.is_empty())
        .map(|l| IssueLabel {
            name: l.name,
            color: l.color.unwrap_or_default(),
        })
        .collect()
}

fn issue_type_from(t: GqlIssueType) -> Option<IssueTypeRef> {
    if t.name.is_empty() {
        return None;
    }
    Some(IssueTypeRef {
        name: t.name,
        color: t.color.unwrap_or_default(),
    })
}

fn fields_from(conn: Option<GqlFieldValues>) -> Vec<IssueFieldValue> {
    conn.and_then(|c| c.nodes)
        .unwrap_or_default()
        .iter()
        .filter_map(issue_field_from_value)
        .collect()
}

fn issue_field_from_value(v: &serde_json::Value) -> Option<IssueFieldValue> {
    if v.is_null() {
        return None;
    }
    let typename = v.get("__typename").and_then(|t| t.as_str()).unwrap_or("");
    let name = v
        .get("field")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    match typename {
        "IssueFieldSingleSelectValue" => {
            let value = v.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
            if value.is_empty() {
                return None;
            }
            Some(IssueFieldValue {
                name,
                value: value.to_string(),
                color: v
                    .get("color")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
                    .filter(|c| !c.is_empty()),
            })
        }
        "IssueFieldTextValue" | "IssueFieldDateValue" => {
            let value = v
                .get("value")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            if value.is_empty() {
                return None;
            }
            Some(IssueFieldValue {
                name,
                value,
                color: None,
            })
        }
        "IssueFieldNumberValue" => Some(IssueFieldValue {
            name,
            value: json_number(v.get("value")?),
            color: None,
        }),
        _ => None,
    }
}

fn json_number(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.is_finite() {
                    format!("{}", f as i64)
                } else {
                    format!("{f}")
                }
            } else {
                n.to_string()
            }
        }
        serde_json::Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// True when the payload has GraphQL errors and no repository issue data.
pub fn graphql_issue_data_missing(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    if v.get("errors").is_none() {
        return false;
    }
    match v.get("data") {
        None | Some(serde_json::Value::Null) => true,
        Some(data) => data.get("repository").map(|r| r.is_null()).unwrap_or(true),
    }
}

pub fn graphql_error_summary(json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return "graphql error".into();
    };
    v.get("errors")
        .and_then(|e| e.as_array())
        .and_then(|a| a.first())
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or("graphql error")
        .to_string()
}

const ISSUE_NODES: &str = r#"
        number
        title
        body
        state
        createdAt
        updatedAt
        parent { number }
        issueType { name color }
        labels(first: 20) { nodes { name color } }
"#;

const ISSUE_FIELDS: &str = r#"
        issueFieldValues(first: 20) {
          nodes {
            __typename
            ... on IssueFieldDateValue {
              value
              field { ... on IssueFieldDate { name } }
            }
            ... on IssueFieldTextValue {
              value
              field { ... on IssueFieldText { name } }
            }
            ... on IssueFieldNumberValue {
              value
              field { ... on IssueFieldNumber { name } }
            }
            ... on IssueFieldSingleSelectValue {
              name
              color
              field { ... on IssueFieldSingleSelect { name } }
            }
          }
        }
"#;

fn issues_query(with_fields: bool) -> String {
    let extra = if with_fields { ISSUE_FIELDS } else { "" };
    format!(
        r#"
query($owner: String!, $name: String!, $cursor: String, $since: DateTime) {{
  rateLimit {{ remaining resetAt cost }}
  repository(owner: $owner, name: $name) {{
    issues(first: 100, after: $cursor, states: [OPEN, CLOSED], filterBy: {{ since: $since }}, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{
      pageInfo {{ hasNextPage endCursor }}
      nodes {{
        {ISSUE_NODES}
        {extra}
      }}
    }}
  }}
}}
"#
    )
}

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

    fn post_issues(
        &self,
        query: &str,
        owner: &str,
        repo: &str,
        cursor: Option<&str>,
        since: Option<&str>,
        issue_fields: bool,
    ) -> Result<String, GitHubError> {
        let body = serde_json::json!({
            "query": query,
            "variables": { "owner": owner, "name": repo, "cursor": cursor, "since": since },
        });
        let mut req = self
            .http
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "issue-viewer");
        if issue_fields {
            // Required for issueFieldValues. Labels and issueType do not need it.
            req = req.header("GraphQL-Features", "issue_fields");
        }
        let resp = req
            .json(&body)
            .send()
            .map_err(|e| GitHubError::Http(e.to_string()))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(GitHubError::Auth);
        }
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(GitHubError::Http("rate_limited".into()));
        }
        resp.text().map_err(|e| GitHubError::Http(e.to_string()))
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
        let text = self.post_issues(&issues_query(true), owner, repo, cursor, since, true)?;
        let text = if graphql_issue_data_missing(&text) {
            self.post_issues(&issues_query(false), owner, repo, cursor, since, false)?
        } else {
            text
        };
        if graphql_issue_data_missing(&text) {
            return Err(GitHubError::Http(graphql_error_summary(&text)));
        }
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
        assert!(page.rows[0].labels.is_empty());
        assert!(page.rows[0].issue_type.is_none());
        assert!(page.rows[0].fields.is_empty());
    }

    #[test]
    fn parses_labels_type_and_fields() {
        let json = r#"{
          "data": {
            "rateLimit": { "remaining": 100, "cost": 2 },
            "repository": {
              "issues": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": [
                  {
                    "number": 7,
                    "title": "typed",
                    "body": "",
                    "state": "OPEN",
                    "createdAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-03T00:00:00Z",
                    "parent": null,
                    "issueType": { "name": "Bug", "color": "RED" },
                    "labels": { "nodes": [ { "name": "p1", "color": "d73a4a" }, null, { "name": "", "color": "000000" } ] },
                    "issueFieldValues": {
                      "nodes": [
                        {
                          "__typename": "IssueFieldSingleSelectValue",
                          "name": "P1",
                          "color": "RED",
                          "field": { "name": "Priority" }
                        },
                        {
                          "__typename": "IssueFieldDateValue",
                          "value": "2026-04-15",
                          "field": { "name": "Target" }
                        },
                        {
                          "__typename": "IssueFieldNumberValue",
                          "value": 3,
                          "field": { "name": "Effort" }
                        },
                        {
                          "__typename": "IssueFieldTextValue",
                          "value": "api",
                          "field": { "name": "Area" }
                        },
                        {
                          "__typename": "IssueFieldTextValue",
                          "value": "",
                          "field": { "name": "Empty" }
                        }
                      ]
                    }
                  }
                ]
              }
            }
          }
        }"#;
        let page = rows_from_graphql_json(json).unwrap();
        let row = &page.rows[0];
        assert_eq!(
            row.issue_type.as_ref().map(|t| t.name.as_str()),
            Some("Bug")
        );
        assert_eq!(
            row.issue_type.as_ref().map(|t| t.color.as_str()),
            Some("RED")
        );
        assert_eq!(row.labels.len(), 1);
        assert_eq!(row.labels[0].name, "p1");
        assert_eq!(row.labels[0].color, "d73a4a");
        assert_eq!(
            row.fields,
            vec![
                IssueFieldValue {
                    name: "Priority".into(),
                    value: "P1".into(),
                    color: Some("RED".into()),
                },
                IssueFieldValue {
                    name: "Target".into(),
                    value: "2026-04-15".into(),
                    color: None,
                },
                IssueFieldValue {
                    name: "Effort".into(),
                    value: "3".into(),
                    color: None,
                },
                IssueFieldValue {
                    name: "Area".into(),
                    value: "api".into(),
                    color: None,
                },
            ]
        );
    }

    #[test]
    fn missing_repository_with_errors_is_a_failed_payload() {
        let json =
            r#"{"data":{"repository":null},"errors":[{"message":"issueFieldValues not found"}]}"#;
        assert!(graphql_issue_data_missing(json));
        assert_eq!(graphql_error_summary(json), "issueFieldValues not found");
        assert!(!graphql_issue_data_missing(
            r#"{"data":{"repository":{"issues":{}}}}"#
        ));
    }
}
