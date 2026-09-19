use std::collections::{HashMap, HashSet};

use crate::model::{IssueRow, IssueState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub issue: IssueRow,
    pub children: Vec<TreeNode>,
}

pub fn build_tree(rows: &[IssueRow]) -> Vec<TreeNode> {
    let present: HashSet<u64> = rows.iter().map(|r| r.number).collect();
    let normalized: Vec<IssueRow> = rows
        .iter()
        .map(|r| {
            let mut row = r.clone();
            if let Some(p) = row.parent_number {
                if !present.contains(&p) {
                    row.parent_number = None;
                }
            }
            row
        })
        .collect();

    let mut by_parent: HashMap<Option<u64>, Vec<IssueRow>> = HashMap::new();
    for row in &normalized {
        by_parent
            .entry(row.parent_number)
            .or_default()
            .push(row.clone());
    }
    fn build(parent: Option<u64>, by_parent: &HashMap<Option<u64>, Vec<IssueRow>>) -> Vec<TreeNode> {
        by_parent
            .get(&parent)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|issue| {
                let number = issue.number;
                TreeNode {
                    issue,
                    children: build(Some(number), by_parent),
                }
            })
            .collect()
    }
    build(None, &by_parent)
}

fn sample(number: u64, parent: Option<u64>, title: &str) -> IssueRow {
    IssueRow {
        number,
        title: title.to_string(),
        state: IssueState::Open,
        parent_number: parent,
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_child_under_parent() {
        let rows = vec![
            sample(1, None, "parent"),
            sample(2, Some(1), "child"),
        ];
        let tree = build_tree(&rows);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].issue.number, 1);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].issue.number, 2);
    }

    #[test]
    fn two_roots_stay_roots() {
        let rows = vec![sample(1, None, "a"), sample(2, None, "b")];
        let tree = build_tree(&rows);
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn missing_parent_becomes_root() {
        let rows = vec![sample(2, Some(99), "orphan")];
        let tree = build_tree(&rows);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].issue.number, 2);
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn nests_grandchild() {
        let rows = vec![
            sample(1, None, "p"),
            sample(2, Some(1), "c"),
            sample(3, Some(2), "gc"),
        ];
        let tree = build_tree(&rows);
        assert_eq!(tree[0].children[0].children[0].issue.number, 3);
    }
}
