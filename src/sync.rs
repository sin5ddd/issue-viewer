use crate::db::Cache;
#[cfg(test)]
use crate::github::{IssuePage, RepoRef};
use crate::github::{GitHubClient, GitHubError};
#[cfg(test)]
use crate::model::{IssueRow, IssueState};

pub fn sync_repo(
    cache: &Cache,
    client: &dyn GitHubClient,
    owner: &str,
    repo: &str,
) -> Result<Option<u32>, GitHubError> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    let mut remaining = None::<u32>;
    loop {
        let mut page = client.fetch_issue_page(owner, repo, cursor.as_deref())?;
        remaining = page.rate_remaining.or(remaining);
        all.append(&mut page.rows);
        if page.rate_remaining.is_some_and(|n| n < 100) {
            break;
        }
        if !page.has_next {
            break;
        }
        cursor = page.end_cursor;
    }
    cache
        .upsert_issues(owner, repo, &all)
        .map_err(|e| GitHubError::Http(e.to_string()))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    cache
        .set_last_synced(owner, repo, &ts)
        .map_err(|e| GitHubError::Http(e.to_string()))?;
    Ok(remaining)
}

#[cfg(test)]
struct Fake {
    pages: Vec<IssuePage>,
}

#[cfg(test)]
impl GitHubClient for Fake {
    fn list_repos(&self) -> Result<Vec<RepoRef>, GitHubError> {
        Ok(vec![])
    }
    fn fetch_issue_page(
        &self,
        _owner: &str,
        _repo: &str,
        cursor: Option<&str>,
    ) -> Result<IssuePage, GitHubError> {
        let idx = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
        Ok(self.pages.get(idx).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: u64, parent: Option<u64>) -> IssueRow {
        IssueRow {
            number: n,
            title: format!("t{n}"),
            body: String::new(),
            state: IssueState::Open,
            parent_number: parent,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn concatenates_pages_into_cache() {
        let cache = Cache::open_memory().unwrap();
        let fake = Fake {
            pages: vec![
                IssuePage {
                    rows: vec![row(1, None)],
                    has_next: true,
                    end_cursor: Some("1".into()),
                    rate_remaining: Some(4000),
                },
                IssuePage {
                    rows: vec![row(2, Some(1))],
                    has_next: false,
                    end_cursor: None,
                    rate_remaining: Some(3990),
                },
            ],
        };
        sync_repo(&cache, &fake, "acme", "app").unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        assert!(cache.last_synced("acme", "app").unwrap().is_some());
    }

    #[test]
    fn stops_when_rate_remaining_low() {
        let cache = Cache::open_memory().unwrap();
        let fake = Fake {
            pages: vec![
                IssuePage {
                    rows: vec![row(1, None)],
                    has_next: true,
                    end_cursor: Some("1".into()),
                    rate_remaining: Some(50),
                },
                IssuePage {
                    rows: vec![row(2, Some(1))],
                    has_next: false,
                    end_cursor: None,
                    rate_remaining: Some(40),
                },
            ],
        };
        sync_repo(&cache, &fake, "acme", "app").unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 1);
    }
}
