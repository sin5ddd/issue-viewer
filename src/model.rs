use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRow {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    pub parent_number: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
    pub labels: Vec<IssueLabel>,
    pub issue_type: Option<IssueTypeRef>,
    pub fields: Vec<IssueFieldValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueLabel {
    pub name: String,
    #[serde(default)]
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueTypeRef {
    pub name: String,
    #[serde(default)]
    pub color: String,
}

/// One GitHub issue-field value (not a Project field).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueFieldValue {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}
