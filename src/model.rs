#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRow {
    pub number: u64,
    pub title: String,
    pub state: IssueState,
    pub parent_number: Option<u64>,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueState {
    Open,
    Closed,
}
