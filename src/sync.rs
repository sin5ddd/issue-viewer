use crate::db::Cache;
use crate::github::{GitHubClient, GitHubError};
#[cfg(test)]
use crate::github::{IssuePage, RepoRef};
#[cfg(test)]
use crate::model::{IssueRow, IssueState};

pub fn sync_repo(
    cache: &Cache,
    client: &dyn GitHubClient,
    owner: &str,
    repo: &str,
) -> Result<Option<u32>, GitHubError> {
    let since = cache
        .max_updated_at(owner, repo)
        .map_err(|e| GitHubError::Http(e.to_string()))?;
    let full_replace = since.is_none();
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    let mut remaining = None::<u32>;
    loop {
        let mut page = client.fetch_issue_page(owner, repo, cursor.as_deref(), since.as_deref())?;
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
    if full_replace {
        cache
            .replace_issues(owner, repo, &all)
            .map_err(|e| GitHubError::Http(e.to_string()))?;
    } else {
        cache
            .upsert_issues(owner, repo, &all)
            .map_err(|e| GitHubError::Http(e.to_string()))?;
    }
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
    seen_since: std::cell::RefCell<Vec<Option<String>>>,
}

#[cfg(test)]
impl Fake {
    fn new(pages: Vec<IssuePage>) -> Self {
        Self {
            pages,
            seen_since: std::cell::RefCell::new(Vec::new()),
        }
    }
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
        since: Option<&str>,
    ) -> Result<IssuePage, GitHubError> {
        self.seen_since.borrow_mut().push(since.map(str::to_string));
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
        let fake = Fake::new(vec![
            IssuePage {
                rows: vec![row(1, None)],
                has_next: true,
                end_cursor: Some("1".into()),
                rate_remaining: Some(4000),
                rate_cost: Some(1),
            },
            IssuePage {
                rows: vec![row(2, Some(1))],
                has_next: false,
                end_cursor: None,
                rate_remaining: Some(3990),
                rate_cost: Some(1),
            },
        ]);
        sync_repo(&cache, &fake, "acme", "app").unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        assert!(cache.last_synced("acme", "app").unwrap().is_some());
        assert_eq!(fake.seen_since.borrow().as_slice(), &[None, None]);
    }

    #[test]
    fn stops_when_rate_remaining_low() {
        let cache = Cache::open_memory().unwrap();
        let fake = Fake::new(vec![
            IssuePage {
                rows: vec![row(1, None)],
                has_next: true,
                end_cursor: Some("1".into()),
                rate_remaining: Some(50),
                rate_cost: Some(1),
            },
            IssuePage {
                rows: vec![row(2, Some(1))],
                has_next: false,
                end_cursor: None,
                rate_remaining: Some(40),
                rate_cost: Some(1),
            },
        ]);
        sync_repo(&cache, &fake, "acme", "app").unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 1);
        assert_eq!(fake.seen_since.borrow().len(), 1);
    }

    #[test]
    fn incremental_upserts_without_dropping_old_rows() {
        let cache = Cache::open_memory().unwrap();
        let first = Fake::new(vec![IssuePage {
            rows: vec![row(1, None), row(2, Some(1))],
            has_next: false,
            end_cursor: None,
            rate_remaining: Some(4000),
            rate_cost: Some(1),
        }]);
        sync_repo(&cache, &first, "acme", "app").unwrap();
        assert_eq!(first.seen_since.borrow().as_slice(), &[None]);

        let mut updated = row(2, Some(1));
        updated.body = "new".into();
        updated.updated_at = "2026-02-01T00:00:00Z".into();
        let second = Fake::new(vec![IssuePage {
            rows: vec![updated],
            has_next: false,
            end_cursor: None,
            rate_remaining: Some(3999),
            rate_cost: Some(1),
        }]);
        sync_repo(&cache, &second, "acme", "app").unwrap();
        assert_eq!(
            second.seen_since.borrow()[0].as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(second.seen_since.borrow().len(), 1);
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        let one = got.iter().find(|r| r.number == 1).unwrap();
        let two = got.iter().find(|r| r.number == 2).unwrap();
        assert_eq!(one.body, "");
        assert_eq!(two.body, "new");
        assert_eq!(two.updated_at, "2026-02-01T00:00:00Z");
    }

    #[test]
    fn rate_stop_does_not_drop_cached_rows() {
        let cache = Cache::open_memory().unwrap();
        cache
            .upsert_issues("acme", "app", &[row(1, None), row(2, Some(1))])
            .unwrap();
        let fake = Fake::new(vec![
            IssuePage {
                rows: vec![row(1, None)],
                has_next: true,
                end_cursor: Some("1".into()),
                rate_remaining: Some(50),
                rate_cost: Some(1),
            },
            IssuePage {
                rows: vec![row(2, Some(1))],
                has_next: false,
                end_cursor: None,
                rate_remaining: Some(40),
                rate_cost: Some(1),
            },
        ]);
        sync_repo(&cache, &fake, "acme", "app").unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|r| r.number == 2));
        assert_eq!(fake.seen_since.borrow().len(), 1);
    }
}
