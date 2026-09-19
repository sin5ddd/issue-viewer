use crate::model::{IssueRow, IssueState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Created,
    Updated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

pub fn matches_query(row: &IssueRow, query: &str) -> bool {
    let tokens: Vec<&str> = query
        .split_whitespace()
        .filter(|t| {
            let l = t.to_ascii_lowercase();
            l != "is:open" && l != "is:closed"
        })
        .collect();
    if tokens.is_empty() {
        return true;
    }
    let title = row.title.to_ascii_lowercase();
    tokens.iter().all(|tok| {
        let t = tok.to_ascii_lowercase();
        if let Some(rest) = t.strip_prefix('#') {
            return rest.parse::<u64>().ok() == Some(row.number);
        }
        if let Ok(n) = t.parse::<u64>() {
            return n == row.number;
        }
        title.contains(&t)
    })
}

fn state_flags_from_query(query: &str, show_open: bool, show_closed: bool) -> (bool, bool) {
    let mut open = show_open;
    let mut closed = show_closed;
    let mut saw = false;
    for tok in query.split_whitespace() {
        match tok.to_ascii_lowercase().as_str() {
            "is:open" => {
                saw = true;
                open = true;
                closed = false;
            }
            "is:closed" => {
                saw = true;
                open = false;
                closed = true;
            }
            _ => {}
        }
    }
    if saw {
        return (open, closed);
    }
    if !open && !closed {
        (true, true)
    } else {
        (open, closed)
    }
}

pub fn apply(
    rows: &[IssueRow],
    query: &str,
    show_open: bool,
    show_closed: bool,
    sort: SortKey,
    dir: SortDir,
) -> Vec<IssueRow> {
    let (show_open, show_closed) = state_flags_from_query(query, show_open, show_closed);
    let mut out: Vec<IssueRow> = rows
        .iter()
        .filter(|r| match r.state {
            IssueState::Open => show_open,
            IssueState::Closed => show_closed,
        })
        .filter(|r| matches_query(r, query))
        .cloned()
        .collect();
    out.sort_by(|a, b| {
        let key = match sort {
            SortKey::Created => a.created_at.cmp(&b.created_at),
            SortKey::Updated => a.updated_at.cmp(&b.updated_at),
        };
        let key = match dir {
            SortDir::Asc => key,
            SortDir::Desc => key.reverse(),
        };
        key.then(a.number.cmp(&b.number))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: u64, title: &str, state: IssueState, created: &str, updated: &str) -> IssueRow {
        IssueRow {
            number: n,
            title: title.into(),
            body: String::new(),
            state,
            parent_number: None,
            created_at: created.into(),
            updated_at: updated.into(),
        }
    }

    #[test]
    fn is_open_hides_closed() {
        let rows = vec![
            row(1, "a", IssueState::Open, "1", "1"),
            row(2, "b", IssueState::Closed, "1", "1"),
        ];
        let got = apply(&rows, "is:open", true, true, SortKey::Updated, SortDir::Desc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 1);
    }

    #[test]
    fn foo_matches_title() {
        let rows = vec![
            row(1, "hello foo", IssueState::Open, "1", "1"),
            row(2, "bar", IssueState::Open, "1", "1"),
        ];
        let got = apply(&rows, "foo", true, false, SortKey::Updated, SortDir::Desc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 1);
    }

    #[test]
    fn hash_number_matches() {
        let rows = vec![
            row(1, "a", IssueState::Open, "1", "1"),
            row(2, "b", IssueState::Open, "1", "1"),
        ];
        let got = apply(&rows, "#2", true, false, SortKey::Updated, SortDir::Desc);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].number, 2);
    }

    #[test]
    fn updated_desc_orders_later_first() {
        let rows = vec![
            row(1, "a", IssueState::Open, "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z"),
            row(2, "b", IssueState::Open, "2026-01-01T00:00:00Z", "2026-02-01T00:00:00Z"),
        ];
        let got = apply(&rows, "", true, false, SortKey::Updated, SortDir::Desc);
        assert_eq!(got[0].number, 2);
        assert_eq!(got[1].number, 1);
    }
}
