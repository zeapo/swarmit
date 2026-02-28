pub mod cli;
pub mod events;
pub mod models;
pub mod state;
pub mod tui;

// Re-export the profiling macro so `crate::prof_guard!` works everywhere.
pub(crate) use tui::prof_guard;

pub use models::{
    AgentId, Comment, Epic, Insight, ItemId, Priority, Project, ProjectConfig, RelationType,
    Relationship, Result, Status, SwarmitError, Task,
};

// Re-export the new database-backed public API.
pub use state::{
    compact_db, count_operations, latest_rowid, load_state, open_db, read_all_operations,
    read_operations_since, write_operation, write_operations,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::operations::{Operation, OperationKind};
    use crate::models::AgentId;
    use tempfile::tempdir;

    fn agent() -> AgentId {
        AgentId::new("test-agent").unwrap()
    }

    fn make_op(kind: OperationKind) -> Operation {
        Operation::new(agent(), kind)
    }

    #[test]
    fn load_state_empty_dir() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        std::fs::create_dir_all(project_root.join(".swarmit")).unwrap();

        let conn = open_db(project_root).expect("open_db should succeed");
        let state = load_state(&conn).expect("load_state should succeed");
        assert!(state.tasks.is_empty());
        assert!(state.epics.is_empty());
    }

    #[test]
    fn load_state_reflects_written_operations() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        std::fs::create_dir_all(project_root.join(".swarmit")).unwrap();

        let conn = open_db(project_root).expect("open_db should succeed");

        let task_id: ItemId = "TASK-001".parse().unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::InitProject {
                name: "Test".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
                auto_materialize: None,
                materialize_path: None,
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Hello".to_string(),
                description: None,
                priority: crate::models::Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();

        let state = load_state(&conn).expect("load_state should succeed");
        assert!(state.tasks.contains_key(&task_id));
        assert_eq!(state.tasks[&task_id].title, "Hello");
    }

    #[test]
    fn load_state_is_idempotent() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        std::fs::create_dir_all(project_root.join(".swarmit")).unwrap();

        let conn = open_db(project_root).expect("open_db should succeed");

        let task_id: ItemId = "TASK-001".parse().unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Idempotent".to_string(),
                description: None,
                priority: crate::models::Priority::Low,
                epic_id: None,
            }),
        )
        .unwrap();

        let state1 = load_state(&conn).expect("first load_state");
        let state2 = load_state(&conn).expect("second load_state");

        assert_eq!(state1.tasks.len(), state2.tasks.len());
        assert!(state2.tasks.contains_key(&task_id));
    }
}
