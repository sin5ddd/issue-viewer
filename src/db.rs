use crate::model::{IssueRow, IssueState};
use rusqlite::{Connection, OptionalExtension, Result};

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        let conn = Connection::open(path)?;
        let cache = Self { conn };
        cache.migrate()?;
        Ok(cache)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let cache = Self { conn };
        cache.migrate()?;
        Ok(cache)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS issues (
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                number INTEGER NOT NULL,
                title TEXT NOT NULL,
                state TEXT NOT NULL,
                parent_number INTEGER,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (owner, repo, number)
            );
            CREATE TABLE IF NOT EXISTS sync_meta (
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                last_synced_rfc3339 TEXT NOT NULL,
                PRIMARY KEY (owner, repo)
            );
            ",
        )?;
        Ok(())
    }

    pub fn upsert_issues(&self, owner: &str, repo: &str, rows: &[IssueRow]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM issues WHERE owner = ?1 AND repo = ?2",
            rusqlite::params![owner, repo],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO issues (owner, repo, number, title, state, parent_number, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for r in rows {
                let state = match r.state {
                    IssueState::Open => "open",
                    IssueState::Closed => "closed",
                };
                stmt.execute(rusqlite::params![
                    owner,
                    repo,
                    r.number as i64,
                    r.title,
                    state,
                    r.parent_number.map(|n| n as i64),
                    r.updated_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_issues(&self, owner: &str, repo: &str) -> Result<Vec<IssueRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT number, title, state, parent_number, updated_at
             FROM issues WHERE owner = ?1 AND repo = ?2 ORDER BY number",
        )?;
        let rows = stmt.query_map(rusqlite::params![owner, repo], |row| {
            let state_s: String = row.get(2)?;
            let parent: Option<i64> = row.get(3)?;
            Ok(IssueRow {
                number: row.get::<_, i64>(0)? as u64,
                title: row.get(1)?,
                state: if state_s == "closed" {
                    IssueState::Closed
                } else {
                    IssueState::Open
                },
                parent_number: parent.map(|n| n as u64),
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn last_synced(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        self.conn.query_row(
            "SELECT last_synced_rfc3339 FROM sync_meta WHERE owner = ?1 AND repo = ?2",
            rusqlite::params![owner, repo],
            |row| row.get(0),
        ).optional()
    }

    pub fn set_last_synced(&self, owner: &str, repo: &str, ts: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_meta (owner, repo, last_synced_rfc3339) VALUES (?1, ?2, ?3)
             ON CONFLICT(owner, repo) DO UPDATE SET last_synced_rfc3339 = excluded.last_synced_rfc3339",
            rusqlite::params![owner, repo, ts],
        )?;
        Ok(())
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
    fn upsert_then_list_roundtrip() {
        let cache = Cache::open_memory().unwrap();
        cache
            .upsert_issues("acme", "app", &[row(1, None), row(2, Some(1))])
            .unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got.iter().find(|r| r.number == 2).unwrap().parent_number, Some(1));
    }

    #[test]
    fn upsert_replaces_repo_rows() {
        let cache = Cache::open_memory().unwrap();
        cache.upsert_issues("acme", "app", &[row(1, None)]).unwrap();
        cache.upsert_issues("acme", "app", &[row(3, None)]).unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 3);
    }

    #[test]
    fn last_synced_roundtrip() {
        let cache = Cache::open_memory().unwrap();
        assert_eq!(cache.last_synced("acme", "app").unwrap(), None);
        cache.set_last_synced("acme", "app", "2026-09-19T00:00:00Z").unwrap();
        assert_eq!(
            cache.last_synced("acme", "app").unwrap().as_deref(),
            Some("2026-09-19T00:00:00Z")
        );
    }
}
