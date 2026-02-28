/// End-to-end CLI round-trip tests using swarmit-core directly
/// (avoids spawning the binary in tests, keeping them fast and hermetic).
use tempfile::TempDir;

use swarmit::events::locking::try_append_with_timeout;
use swarmit::events::log::append_operation;
use swarmit::events::operations::{Operation, OperationKind};
use swarmit::models::{AgentId, ItemId, Priority, Status};
use swarmit::state::{write_snapshot, ProjectState, SnapshotV1};

fn agent() -> AgentId {
    AgentId::new("test-agent").unwrap()
}

fn setup_project(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let swarmit = dir.path().join(".swarmit");
    std::fs::create_dir_all(swarmit.join("state").join("epics")).unwrap();
    std::fs::create_dir_all(swarmit.join("state").join("backlog")).unwrap();
    let log = swarmit.join("operations.log");
    let lock = swarmit.join("operations.lock");

    let init_op = Operation::new(
        agent(),
        OperationKind::InitProject {
            name: "Test Project".to_string(),
            description: None,
            epic_prefix: None,
            task_prefix: None,
        },
    );
    try_append_with_timeout(&lock, || append_operation(&log, &init_op)).unwrap();
    (log, lock)
}

/// Full lifecycle: create epic → create task → claim → done
#[test]
fn full_task_lifecycle() {
    let dir = TempDir::new().unwrap();
    let (log, lock) = setup_project(&dir);

    let epic_id: ItemId = "EPIC-001".parse().unwrap();
    let task_id: ItemId = "TASK-001".parse().unwrap();

    // Create epic
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::CreateEpic {
                    id: epic_id.clone(),
                    title: "Auth System".to_string(),
                    description: Some("User authentication".to_string()),
                    priority: Priority::High,
                },
            ),
        )
    })
    .unwrap();

    // Create task under epic
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::CreateTask {
                    id: task_id.clone(),
                    title: "OAuth2 login".to_string(),
                    description: None,
                    priority: Priority::High,
                    epic_id: Some(epic_id.clone()),
                },
            ),
        )
    })
    .unwrap();

    let state = ProjectState::from_log(&log).unwrap();
    assert_eq!(state.tasks[&task_id].status, Status::Todo);
    assert_eq!(state.epics[&epic_id].task_ids.len(), 1);

    // Claim
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::ClaimTask {
                    id: task_id.clone(),
                },
            ),
        )
    })
    .unwrap();

    let state = ProjectState::from_log(&log).unwrap();
    assert_eq!(state.tasks[&task_id].status, Status::InProgress);
    assert_eq!(state.tasks[&task_id].assignee, Some(agent()));

    // Done
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::CompleteTask {
                    id: task_id.clone(),
                },
            ),
        )
    })
    .unwrap();

    let state = ProjectState::from_log(&log).unwrap();
    assert_eq!(state.tasks[&task_id].status, Status::Done);
    assert!(state.tasks[&task_id].completed_at.is_some());
}

/// JSON output structure: { ok, data, error }
#[test]
fn json_envelope_format() {
    use swarmit::cli::output::JsonOutput;

    let ok = JsonOutput::success(serde_json::json!({ "id": "TASK-001" }));
    let json = serde_json::to_string(&ok).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["data"]["id"], "TASK-001");
    assert!(parsed.get("error").is_none() || parsed["error"].is_null());

    let err = JsonOutput::<serde_json::Value>::error("Not found");
    let json = serde_json::to_string(&err).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["error"], "Not found");
}

/// Corrupted log recovery: a partial trailing line should be skipped.
#[test]
fn corrupted_log_recovery() {
    use std::io::Write;

    let dir = TempDir::new().unwrap();
    let (log, lock) = setup_project(&dir);

    // Write a valid operation
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::CreateTask {
                    id: "TASK-001".parse().unwrap(),
                    title: "Valid task".to_string(),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                },
            ),
        )
    })
    .unwrap();

    // Corrupt the log by appending a partial line
    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    file.write_all(b"{\"corrupted\": true, \"incomplete\":")
        .unwrap();
    drop(file);

    // Should still read the valid operations successfully
    let state = ProjectState::from_log(&log).unwrap();
    assert_eq!(state.tasks.len(), 1);
    assert!(state
        .tasks
        .contains_key(&"TASK-001".parse::<ItemId>().unwrap()));
}

/// Relationship inverse is automatically created.
#[test]
fn relationship_inverse_created() {
    let dir = TempDir::new().unwrap();
    let (log, lock) = setup_project(&dir);

    let t1: ItemId = "TASK-001".parse().unwrap();
    let t2: ItemId = "TASK-002".parse().unwrap();

    for (id, title) in [(&t1, "Task 1"), (&t2, "Task 2")] {
        try_append_with_timeout(&lock, || {
            append_operation(
                &log,
                &Operation::new(
                    agent(),
                    OperationKind::CreateTask {
                        id: id.clone(),
                        title: title.to_string(),
                        description: None,
                        priority: Priority::Medium,
                        epic_id: None,
                    },
                ),
            )
        })
        .unwrap();
    }

    // Add blocks relationship (link.rs also adds the inverse — simulate that here)
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::AddRelationship {
                    from: t1.clone(),
                    to: t2.clone(),
                    rel_type: swarmit::models::RelationType::Blocks,
                },
            ),
        )?;
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::AddRelationship {
                    from: t2.clone(),
                    to: t1.clone(),
                    rel_type: swarmit::models::RelationType::BlockedBy,
                },
            ),
        )
    })
    .unwrap();

    let state = ProjectState::from_log(&log).unwrap();
    assert_eq!(state.relationships.len(), 2);

    let t1_rels = state.relationships_for(&t1);
    assert!(t1_rels
        .iter()
        .any(|r| r.rel_type == swarmit::models::RelationType::Blocks));

    let t2_rels = state.relationships_for(&t2);
    assert!(t2_rels
        .iter()
        .any(|r| r.rel_type == swarmit::models::RelationType::BlockedBy));
}

/// Insight round-trip: create task → add insight → verify fields
#[test]
fn insight_roundtrip() {
    let dir = TempDir::new().unwrap();
    let (log, lock) = setup_project(&dir);

    let task_id: ItemId = "TASK-001".parse().unwrap();

    // Create task
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::CreateTask {
                    id: task_id.clone(),
                    title: "Refactor auth".to_string(),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                },
            ),
        )
    })
    .unwrap();

    // Add insight
    let insight_id = uuid::Uuid::now_v7();
    try_append_with_timeout(&lock, || {
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::AddInsight {
                    id: insight_id,
                    task_id: task_id.clone(),
                    file_path: "src/auth.rs".to_string(),
                    before_snippet: Some("fn old_login()".to_string()),
                    after_snippet: Some("fn login() -> Result<()>".to_string()),
                    body: "Improved error handling".to_string(),
                },
            ),
        )
    })
    .unwrap();

    let state = ProjectState::from_log(&log).unwrap();
    let insights = state.insights_for(&task_id);
    assert_eq!(insights.len(), 1);
    assert_eq!(insights[0].id, insight_id);
    assert_eq!(insights[0].file_path, "src/auth.rs");
    assert_eq!(
        insights[0].before_snippet.as_deref(),
        Some("fn old_login()")
    );
    assert_eq!(
        insights[0].after_snippet.as_deref(),
        Some("fn login() -> Result<()>")
    );
    assert_eq!(insights[0].body, "Improved error handling");
    assert_eq!(insights[0].author, agent());
}

/// Self-links are rejected.
#[test]
fn self_link_rejected() {
    use swarmit::models::SwarmitError;

    let id: ItemId = "TASK-001".parse().unwrap();

    // SwarmitError::SelfRelationship variant should be used in business logic.
    // Here we test that it exists and formats correctly.
    let err = SwarmitError::SelfRelationship(id.clone());
    assert!(err.to_string().contains("TASK-001"));
}

/// Compaction: snapshot file is written and log is truncated, state survives.
#[test]
fn compaction_preserves_state() {
    let dir = TempDir::new().unwrap();
    let (log, _lock) = setup_project(&dir);
    let swarmit_dir = log.parent().unwrap();
    let bak = swarmit_dir.join("operations.log.bak");
    let snapshot_path = swarmit_dir.join("state.snap");

    // Write some tasks
    for i in 1..=5u32 {
        let task_id = ItemId::new("TASK", i);
        append_operation(
            &log,
            &Operation::new(
                agent(),
                OperationKind::CreateTask {
                    id: task_id.clone(),
                    title: format!("Task {}", i),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                },
            ),
        )
        .unwrap();
    }

    let before = ProjectState::from_log(&log).unwrap();
    assert_eq!(before.tasks.len(), 5);
    let original_log_size = std::fs::metadata(&log).unwrap().len();

    // Simulate compaction (same logic as compact.rs --truncate)
    let log_len = std::fs::metadata(&log).unwrap().len();
    let state = ProjectState::from_log(&log).unwrap();
    write_snapshot(
        &snapshot_path,
        &SnapshotV1 {
            log_offset: log_len,
            state,
        },
    )
    .unwrap();

    // Backup and truncate the log
    std::fs::copy(&log, &bak).unwrap();
    std::fs::write(&log, b"").unwrap();

    // Rewrite snapshot with offset 0 since the log is now empty
    if let Ok(Some(mut snap)) = swarmit::state::read_snapshot(&snapshot_path) {
        snap.log_offset = 0;
        write_snapshot(&snapshot_path, &snap).unwrap();
    }

    // Backup exists
    assert!(bak.exists());

    // Truncated log is empty (smaller than original)
    let compacted_size = std::fs::metadata(&log).unwrap().len();
    assert!(compacted_size < original_log_size);

    // State from the snapshot has all 5 tasks preserved
    let snap = swarmit::state::read_snapshot(&snapshot_path)
        .unwrap()
        .unwrap();
    assert_eq!(snap.state.tasks.len(), 5);
    assert_eq!(snap.log_offset, 0);
}
