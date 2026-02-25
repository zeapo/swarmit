use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::id::{AgentId, ItemId};
use super::status::{Priority, Status};

/// An epic groups related tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    pub id: ItemId,
    pub title: String,
    pub description: Option<String>,
    pub status: Status,
    pub priority: Priority,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: AgentId,
    pub assignee: Option<AgentId>,
    /// Ordered list of task IDs belonging to this epic.
    pub task_ids: Vec<ItemId>,
}

impl Epic {
    pub fn new(id: ItemId, title: impl Into<String>, created_by: AgentId) -> Self {
        let now = Utc::now();
        Epic {
            id,
            title: title.into(),
            description: None,
            status: Status::Todo,
            priority: Priority::Medium,
            created_at: now,
            updated_at: now,
            created_by,
            assignee: None,
            task_ids: Vec::new(),
        }
    }
}
