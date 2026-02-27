pub mod events;
pub mod models;
pub mod state;

pub use models::{
    AgentId, Comment, Epic, Insight, ItemId, Priority, Project, ProjectConfig, RelationType,
    Relationship, Result, Status, SwarmitError, Task,
};

use std::path::Path;

use crate::events::log::read_operations_since;
use crate::state::{read_snapshot, should_snapshot, write_snapshot, ProjectState, SnapshotV1};

/// Load project state by reading snapshot (if any) then applying the oplog tail.
/// Returns `(state, log_offset)` where `log_offset` is the current end of the log.
pub fn load_state(project_root: &Path) -> Result<(ProjectState, u64)> {
    let log_path = project_root.join(".swarmit/operations.log");
    let snapshot_path = project_root.join(".swarmit/state.snap");

    let (mut state, log_offset) = match read_snapshot(&snapshot_path)? {
        Some(snap) => (snap.state, snap.log_offset),
        None => (ProjectState::default(), 0u64),
    };

    let (new_ops, new_offset) = read_operations_since(&log_path, log_offset)?;
    for op in new_ops {
        let _ = state.apply(op);
    }

    Ok((state, new_offset))
}

/// After writing an operation and applying it to state, call this to auto-snapshot if the
/// threshold is met.
pub fn check_and_write_snapshot(
    _log_path: &Path,
    snapshot_path: &Path,
    log_len: u64,
    snapshot_offset: u64,
    state: &ProjectState,
) -> Result<()> {
    if should_snapshot(log_len, snapshot_offset) {
        write_snapshot(
            snapshot_path,
            &SnapshotV1 {
                log_offset: log_len,
                state: state.clone(),
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::log::append_operation;
    use crate::events::operations::{Operation, OperationKind};
    use crate::models::AgentId;
    use tempfile::tempdir;

    fn agent() -> AgentId {
        AgentId::new("test-agent").unwrap()
    }

    fn make_op(kind: OperationKind) -> Operation {
        Operation::new(agent(), kind)
    }

    /// Write operations directly to the log (no lock needed in single-threaded tests).
    fn write_ops(project_root: &Path, ops: Vec<Operation>) {
        let log_path = project_root.join(".swarmit/operations.log");
        for op in ops {
            append_operation(&log_path, &op).expect("append_operation failed");
        }
    }

    #[test]
    fn load_state_empty_dir() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        std::fs::create_dir_all(project_root.join(".swarmit")).unwrap();

        let (state, offset) = load_state(project_root).expect("load_state should succeed");
        assert_eq!(offset, 0);
        assert!(state.tasks.is_empty());
        assert!(state.epics.is_empty());
    }

    #[test]
    fn load_state_reflects_written_operations() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        std::fs::create_dir_all(project_root.join(".swarmit")).unwrap();

        let task_id: ItemId = "TASK-001".parse().unwrap();
        write_ops(
            project_root,
            vec![
                make_op(OperationKind::InitProject {
                    name: "Test".to_string(),
                    description: None,
                    epic_prefix: None,
                    task_prefix: None,
                }),
                make_op(OperationKind::CreateTask {
                    id: task_id.clone(),
                    title: "Hello".to_string(),
                    description: None,
                    priority: crate::models::Priority::Medium,
                    epic_id: None,
                }),
            ],
        );

        let (state, offset) = load_state(project_root).expect("load_state should succeed");
        assert!(offset > 0);
        assert!(state.tasks.contains_key(&task_id));
        assert_eq!(state.tasks[&task_id].title, "Hello");
    }

    #[test]
    fn load_state_is_idempotent() {
        let dir = tempdir().unwrap();
        let project_root = dir.path();
        std::fs::create_dir_all(project_root.join(".swarmit")).unwrap();

        let task_id: ItemId = "TASK-001".parse().unwrap();
        write_ops(
            project_root,
            vec![make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Idempotent".to_string(),
                description: None,
                priority: crate::models::Priority::Low,
                epic_id: None,
            })],
        );

        let (state1, offset1) = load_state(project_root).expect("first load_state");
        let (state2, offset2) = load_state(project_root).expect("second load_state");

        assert_eq!(offset1, offset2);
        assert_eq!(state1.tasks.len(), state2.tasks.len());
        assert!(state2.tasks.contains_key(&task_id));
    }

    #[test]
    fn check_and_write_snapshot_writes_when_threshold_met() {
        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();

        let log_path = swarmit_dir.join("operations.log");
        let snapshot_path = swarmit_dir.join("state.snap");

        let state = ProjectState::default();

        // log_len >> snapshot_offset so should_snapshot returns true
        check_and_write_snapshot(&log_path, &snapshot_path, 200_000, 0, &state)
            .expect("check_and_write_snapshot should succeed");

        assert!(snapshot_path.exists(), "snapshot file should have been written");

        let loaded = read_snapshot(&snapshot_path)
            .expect("read_snapshot should succeed")
            .expect("snapshot should be present");
        assert_eq!(loaded.log_offset, 200_000);
    }

    #[test]
    fn check_and_write_snapshot_skips_when_below_threshold() {
        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();

        let log_path = swarmit_dir.join("operations.log");
        let snapshot_path = swarmit_dir.join("state.snap");

        let state = ProjectState::default();

        // log_len too small — should_snapshot returns false
        check_and_write_snapshot(&log_path, &snapshot_path, 1_000, 0, &state)
            .expect("check_and_write_snapshot should succeed");

        assert!(
            !snapshot_path.exists(),
            "snapshot file should NOT have been written when below threshold"
        );
    }
}
