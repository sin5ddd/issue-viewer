use crate::db::Cache;
use crate::github::{GitHubClient, GitHubError, RepoRef};
use crate::model::{IssueRow, IssueState};

pub fn sync_repo(
    cache: &Cache,
    client: &dyn GitHubClient,
    owner: &str,
    repo: &str,
) -> Result<(), GitHubError> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let (mut rows, more, next) = client.fetch_issue_page(owner, repo, cursor.as_deref())?;
        all.append(&mut rows);
        if !more {
            break;
        }
        cursor = next;
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
    Ok(())
}

struct Fake {
    pages: Vec<Vec<IssueRow>>,
}

impl GitHubClient for Fake {
    fn list_repos(&self) -> Result<Vec<RepoRef>, GitHubError> {
        Ok(vec![])
    }
    fn fetch_issue_page(
        &self,
        _owner: &str,
        _repo: &str,
        cursor: Option<&str>,
    ) -> Result<(Vec<IssueRow>, bool, Option<String>), GitHubError> {
        let idx = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
        let page = self.pages.get(idx).cloned().unwrap_or_default();
        let next = idx + 1;
        let more = next < self.pages.len();
        Ok((page, more, more.then(|| next.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: u64, parent: Option<u64>) -> IssueRow {
        IssueRow {
            number: n,
            title: format!("t{n}"),
            state: IssueState::Open,
            parent_number: parent,
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn concatenates_pages_into_cache() {
        let cache = Cache::open_memory().unwrap();
        let fake = Fake {
            pages: vec![vec![row(1, None)], vec![row(2, Some(1))]],
        };
        sync_repo(&cache, &fake, "acme", "app").unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        assert!(cache.last_synced("acme", "app").unwrap().is_some());
    }
}
