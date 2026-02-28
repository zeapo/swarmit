use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{AgentId, ItemId, Priority, RelationType, Status};

/// A single operation in the event log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// UUID v7 — time-sortable, monotonically increasing.
    pub id: Uuid,
    pub agent: AgentId,
    pub timestamp: DateTime<Utc>,
    pub kind: OperationKind,
}

/// All possible mutations to project state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    // ── Project ──────────────────────────────────────────────────────────
    InitProject {
        name: String,
        description: Option<String>,
        epic_prefix: Option<String>,
        task_prefix: Option<String>,
    },
    UpdateProject {
        name: Option<String>,
        /// Some(x) = set description, None = no change (check clear_description to clear)
        description: Option<String>,
        /// When true, clears the description (takes precedence over `description`)
        #[serde(default)]
        clear_description: bool,
    },

    // ── Epic ─────────────────────────────────────────────────────────────
    CreateEpic {
        id: ItemId,
        title: String,
        description: Option<String>,
        priority: Priority,
    },
    UpdateEpic {
        id: ItemId,
        title: Option<String>,
        description: Option<String>,
        priority: Option<Priority>,
        assignee: Option<AgentId>,
    },
    UpdateEpicStatus {
        id: ItemId,
        status: Status,
    },
    DeleteEpic {
        id: ItemId,
    },

    // ── Task ─────────────────────────────────────────────────────────────
    CreateTask {
        id: ItemId,
        title: String,
        description: Option<String>,
        priority: Priority,
        epic_id: Option<ItemId>,
    },
    UpdateTask {
        id: ItemId,
        title: Option<String>,
        description: Option<String>,
        priority: Option<Priority>,
        epic_id: Option<Option<ItemId>>,
        assignee: Option<Option<AgentId>>,
    },
    UpdateTaskStatus {
        id: ItemId,
        status: Status,
    },
    ClaimTask {
        id: ItemId,
    },
    CompleteTask {
        id: ItemId,
    },
    DeleteTask {
        id: ItemId,
    },

    // ── Relationships ─────────────────────────────────────────────────────
    AddRelationship {
        from: ItemId,
        to: ItemId,
        rel_type: RelationType,
    },
    RemoveRelationship {
        from: ItemId,
        to: ItemId,
        rel_type: RelationType,
    },

    // ── Comments ─────────────────────────────────────────────────────────
    AddComment {
        id: Uuid,
        task_id: ItemId,
        body: String,
    },

    // ── Insights ─────────────────────────────────────────────────────────
    AddInsight {
        id: Uuid,
        task_id: ItemId,
        file_path: String,
        before_snippet: Option<String>,
        after_snippet: Option<String>,
        body: String,
    },
}

impl Operation {
    pub fn new(agent: AgentId, kind: OperationKind) -> Self {
        Operation {
            id: Uuid::now_v7(),
            agent,
            timestamp: Utc::now(),
            kind,
        }
    }
}
