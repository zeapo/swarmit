use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::id::{AgentId, ItemId};
use super::status::{Priority, Status};

/// A single unit of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: ItemId,
    pub title: String,
    pub description: Option<String>,
    pub status: Status,
    pub priority: Priority,
    pub epic_id: Option<ItemId>,
    pub assignee: Option<AgentId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: AgentId,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// A comment on a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    /// UUID v7 for time-sortable ordering.
    pub id: Uuid,
    pub task_id: ItemId,
    pub author: AgentId,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl Task {
    /// A backlog task is "archived" when it has no epic and is in a terminal status.
    pub fn is_archived_backlog(&self) -> bool {
        self.epic_id.is_none() && self.status.is_terminal()
    }

    pub fn new(id: ItemId, title: impl Into<String>, created_by: AgentId) -> Self {
        let now = Utc::now();
        Task {
            id,
            title: title.into(),
            description: None,
            status: Status::Todo,
            priority: Priority::Medium,
            epic_id: None,
            assignee: None,
            created_at: now,
            updated_at: now,
            created_by,
            claimed_at: None,
            completed_at: None,
        }
    }
}

impl Comment {
    pub fn new(task_id: ItemId, author: AgentId, body: impl Into<String>) -> Self {
        Comment {
            id: Uuid::now_v7(),
            task_id,
            author,
            body: body.into(),
            created_at: Utc::now(),
        }
    }
}
