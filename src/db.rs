use crate::model::{IssueRow, IssueState};
use rusqlite::{Connection, OptionalExtension, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiLayout {
    pub window_w: f32,
    pub window_h: f32,
    pub left_w: f32,
    pub right_w: f32,
}

impl Default for UiLayout {
    fn default() -> Self {
        Self {
            window_w: 1366.0,
            window_h: 768.0,
            left_w: 280.0,
            right_w: 240.0,
        }
    }
}

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

    #[cfg(test)]
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
            CREATE TABLE IF NOT EXISTS session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                owner TEXT NOT NULL,
                repo TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ui_layout (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                window_w REAL NOT NULL,
                window_h REAL NOT NULL,
                left_w REAL NOT NULL,
                right_w REAL NOT NULL
            );
            ",
        )?;
        for sql in [
            "ALTER TABLE issues ADD COLUMN body TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE issues ADD COLUMN created_at TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE session ADD COLUMN issue_number INTEGER",
        ] {
            let _ = self.conn.execute_batch(sql);
        }
        Ok(())
    }

    pub fn ui_layout(&self) -> Result<UiLayout> {
        let row = self
            .conn
            .query_row(
                "SELECT window_w, window_h, left_w, right_w FROM ui_layout WHERE id = 1",
                [],
                |row| {
                    Ok(UiLayout {
                        window_w: row.get(0)?,
                        window_h: row.get(1)?,
                        left_w: row.get(2)?,
                        right_w: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    pub fn set_ui_layout(&self, layout: &UiLayout) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ui_layout (id, window_w, window_h, left_w, right_w) VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               window_w = excluded.window_w,
               window_h = excluded.window_h,
               left_w = excluded.left_w,
               right_w = excluded.right_w",
            rusqlite::params![
                layout.window_w,
                layout.window_h,
                layout.left_w,
                layout.right_w
            ],
        )?;
        Ok(())
    }

    fn upsert_issues_in(
        tx: &rusqlite::Transaction<'_>,
        owner: &str,
        repo: &str,
        rows: &[IssueRow],
    ) -> Result<()> {
        let mut stmt = tx.prepare(
            "INSERT INTO issues (owner, repo, number, title, state, parent_number, updated_at, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(owner, repo, number) DO UPDATE SET
               title = excluded.title,
               state = excluded.state,
               parent_number = excluded.parent_number,
               updated_at = excluded.updated_at,
               body = excluded.body,
               created_at = excluded.created_at",
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
                r.body,
                r.created_at,
            ])?;
        }
        Ok(())
    }

    pub fn upsert_issues(&self, owner: &str, repo: &str, rows: &[IssueRow]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::upsert_issues_in(&tx, owner, repo, rows)?;
        tx.commit()?;
        Ok(())
    }

    pub fn replace_issues(&self, owner: &str, repo: &str, rows: &[IssueRow]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM issues WHERE owner = ?1 AND repo = ?2",
            rusqlite::params![owner, repo],
        )?;
        Self::upsert_issues_in(&tx, owner, repo, rows)?;
        tx.commit()?;
        Ok(())
    }

    pub fn max_updated_at(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        let value: Option<String> = self.conn.query_row(
            "SELECT MAX(updated_at) FROM issues WHERE owner = ?1 AND repo = ?2",
            rusqlite::params![owner, repo],
            |row| row.get(0),
        )?;
        Ok(value.filter(|s| !s.is_empty()))
    }

    pub fn list_issues(&self, owner: &str, repo: &str) -> Result<Vec<IssueRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT number, title, state, parent_number, updated_at, body, created_at
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
                body: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn last_synced(&self, owner: &str, repo: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT last_synced_rfc3339 FROM sync_meta WHERE owner = ?1 AND repo = ?2",
                rusqlite::params![owner, repo],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn set_last_synced(&self, owner: &str, repo: &str, ts: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_meta (owner, repo, last_synced_rfc3339) VALUES (?1, ?2, ?3)
             ON CONFLICT(owner, repo) DO UPDATE SET last_synced_rfc3339 = excluded.last_synced_rfc3339",
            rusqlite::params![owner, repo, ts],
        )?;
        Ok(())
    }

    pub fn last_repo(&self) -> Result<Option<(String, String, Option<u64>)>> {
        self.conn
            .query_row(
                "SELECT owner, repo, issue_number FROM session WHERE id = 1",
                [],
                |row| {
                    let issue: Option<i64> = row.get(2)?;
                    Ok((row.get(0)?, row.get(1)?, issue.map(|n| n as u64)))
                },
            )
            .optional()
    }

    pub fn set_last_repo(&self, owner: &str, repo: &str, issue_number: Option<u64>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session (id, owner, repo, issue_number) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET owner = excluded.owner, repo = excluded.repo, issue_number = excluded.issue_number",
            rusqlite::params![owner, repo, issue_number.map(|n| n as i64)],
        )?;
        Ok(())
    }

    pub fn clear_last_repo(&self) -> Result<()> {
        self.conn.execute("DELETE FROM session", [])?;
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
            body: format!("body{n}"),
            state: IssueState::Open,
            parent_number: parent,
            created_at: "2026-01-01T00:00:00Z".into(),
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
        let two = got.iter().find(|r| r.number == 2).unwrap();
        assert_eq!(two.parent_number, Some(1));
        assert_eq!(two.body, "body2");
        assert_eq!(two.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn upsert_keeps_untouched_rows() {
        let cache = Cache::open_memory().unwrap();
        cache.upsert_issues("acme", "app", &[row(1, None)]).unwrap();
        cache.upsert_issues("acme", "app", &[row(3, None)]).unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|r| r.number == 1));
        assert!(got.iter().any(|r| r.number == 3));
    }

    #[test]
    fn replace_issues_drops_missing_numbers() {
        let cache = Cache::open_memory().unwrap();
        cache.upsert_issues("acme", "app", &[row(1, None)]).unwrap();
        cache
            .replace_issues("acme", "app", &[row(3, None)])
            .unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 3);
    }

    #[test]
    fn max_updated_at_none_then_latest() {
        let cache = Cache::open_memory().unwrap();
        assert_eq!(cache.max_updated_at("acme", "app").unwrap(), None);
        let mut newer = row(2, None);
        newer.updated_at = "2026-02-01T00:00:00Z".into();
        cache
            .upsert_issues("acme", "app", &[row(1, None), newer])
            .unwrap();
        assert_eq!(
            cache.max_updated_at("acme", "app").unwrap().as_deref(),
            Some("2026-02-01T00:00:00Z")
        );
    }

    #[test]
    fn upsert_overwrites_body_for_same_number() {
        let cache = Cache::open_memory().unwrap();
        cache.upsert_issues("acme", "app", &[row(1, None)]).unwrap();
        let mut updated = row(1, None);
        updated.body = "changed".into();
        cache.upsert_issues("acme", "app", &[updated]).unwrap();
        let got = cache.list_issues("acme", "app").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].body, "changed");
    }

    #[test]
    fn last_synced_roundtrip() {
        let cache = Cache::open_memory().unwrap();
        assert_eq!(cache.last_synced("acme", "app").unwrap(), None);
        cache
            .set_last_synced("acme", "app", "2026-09-19T00:00:00Z")
            .unwrap();
        assert_eq!(
            cache.last_synced("acme", "app").unwrap().as_deref(),
            Some("2026-09-19T00:00:00Z")
        );
    }

    #[test]
    fn last_repo_roundtrip_until_cleared() {
        let cache = Cache::open_memory().unwrap();
        assert_eq!(cache.last_repo().unwrap(), None);
        cache.set_last_repo("acme", "app", None).unwrap();
        assert_eq!(
            cache.last_repo().unwrap(),
            Some(("acme".into(), "app".into(), None))
        );
        cache.clear_last_repo().unwrap();
        assert_eq!(cache.last_repo().unwrap(), None);
    }

    #[test]
    fn session_roundtrip_includes_issue_number() {
        let cache = Cache::open_memory().unwrap();
        cache.set_last_repo("acme", "app", Some(7)).unwrap();
        assert_eq!(
            cache.last_repo().unwrap(),
            Some(("acme".into(), "app".into(), Some(7)))
        );
    }

    #[test]
    fn session_issue_none() {
        let cache = Cache::open_memory().unwrap();
        cache.set_last_repo("acme", "app", None).unwrap();
        assert_eq!(
            cache.last_repo().unwrap(),
            Some(("acme".into(), "app".into(), None))
        );
    }

    #[test]
    fn ui_layout_defaults_then_roundtrip() {
        let cache = Cache::open_memory().unwrap();
        assert_eq!(cache.ui_layout().unwrap(), UiLayout::default());
        let layout = UiLayout {
            window_w: 1600.0,
            window_h: 900.0,
            left_w: 320.0,
            right_w: 200.0,
        };
        cache.set_ui_layout(&layout).unwrap();
        assert_eq!(cache.ui_layout().unwrap(), layout);
    }
}
