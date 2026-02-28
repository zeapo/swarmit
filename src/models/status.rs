use serde::{Deserialize, Serialize};
use std::fmt;

/// Task/Epic status.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Todo,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

impl Status {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Status::Done | Status::Cancelled)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Status::Todo => "Todo",
            Status::InProgress => "In Progress",
            Status::Done => "Done",
            Status::Blocked => "Blocked",
            Status::Cancelled => "Cancelled",
        }
    }

    /// All non-terminal statuses for filtering active work.
    pub fn active_statuses() -> &'static [Status] {
        &[Status::Todo, Status::InProgress, Status::Blocked]
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Work item priority.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Urgent,
}

impl Priority {
    pub fn display_name(&self) -> &'static str {
        match self {
            Priority::Low => "Low",
            Priority::Medium => "Medium",
            Priority::High => "High",
            Priority::Urgent => "Urgent",
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_ordering() {
        // Todo < InProgress < Done < Blocked < Cancelled (by enum variant order)
        assert!(Status::Todo < Status::InProgress);
        assert!(Status::InProgress < Status::Done);
    }

    #[test]
    fn priority_ordering() {
        assert!(Priority::Low < Priority::Medium);
        assert!(Priority::Medium < Priority::High);
        assert!(Priority::High < Priority::Urgent);
    }

    #[test]
    fn status_terminal() {
        assert!(Status::Done.is_terminal());
        assert!(Status::Cancelled.is_terminal());
        assert!(!Status::Todo.is_terminal());
        assert!(!Status::InProgress.is_terminal());
    }

    #[test]
    fn status_display() {
        assert_eq!(Status::InProgress.to_string(), "In Progress");
        assert_eq!(Status::Todo.to_string(), "Todo");
    }

    #[test]
    fn status_serde_roundtrip() {
        let s = Status::InProgress;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#""in_progress""#);
        let back: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
