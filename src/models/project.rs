use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::id::{AgentId, ItemId};

/// The main project configuration, stored in `.swarmit/project.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub description: Option<String>,
    /// Prefix used for epic IDs (default: "EPIC")
    #[serde(default = "default_epic_prefix")]
    pub epic_prefix: String,
    /// Prefix used for task IDs (default: "TASK")
    #[serde(default = "default_task_prefix")]
    pub task_prefix: String,
    /// Whether to auto-materialize markdown on every mutation (default: false)
    #[serde(default)]
    pub auto_materialize: bool,
    /// Path for materialized markdown, relative to .swarmit/ (default: "state")
    #[serde(default = "default_materialize_path")]
    pub materialize_path: String,
    pub created_at: DateTime<Utc>,
    pub created_by: AgentId,
}

fn default_epic_prefix() -> String {
    "EPIC".to_string()
}

fn default_task_prefix() -> String {
    "TASK".to_string()
}

fn default_materialize_path() -> String {
    "state".to_string()
}

/// In-memory project state summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ItemId,
    pub config: ProjectConfig,
    pub epic_count: u32,
    pub task_count: u32,
}

impl ProjectConfig {
    pub fn new(name: impl Into<String>, created_by: AgentId) -> Self {
        ProjectConfig {
            name: name.into(),
            description: None,
            epic_prefix: default_epic_prefix(),
            task_prefix: default_task_prefix(),
            auto_materialize: false,
            materialize_path: default_materialize_path(),
            created_at: Utc::now(),
            created_by,
        }
    }
}
