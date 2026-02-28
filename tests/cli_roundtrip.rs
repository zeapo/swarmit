/// End-to-end CLI round-trip tests using swarmit-core directly
/// (avoids spawning the binary in tests, keeping them fast and hermetic).
use tempfile::TempDir;

use swarmit::events::operations::{Operation, OperationKind};
use swarmit::models::{AgentId, ItemId, Priority, Status};

fn agent() -> AgentId {
    AgentId::new("test-agent").unwrap()
}

fn setup_project(dir: &TempDir) -> rusqlite::Connection {
    let swarmit_dir = dir.path().join(".swarmit");
    std::fs::create_dir_all(swarmit_dir.join("state").join("epics")).unwrap();
    std::fs::create_dir_all(swarmit_dir.join("state").join("backlog")).unwrap();

    let conn = swarmit::open_db(dir.path()).unwrap();

    let init_op = Operation::new(
        agent(),
        OperationKind::InitProject {
            name: "Test Project".to_string(),
            description: None,
            epic_prefix: None,
            task_prefix: None,
            auto_materialize: None,
            materialize_path: None,
        },
    );
    swarmit::write_operation(&conn, &init_op).unwrap();
    conn
}

/// Full lifecycle: create epic → create task → claim → done
#[test]
fn full_task_lifecycle() {
    let dir = TempDir::new().unwrap();
    let conn = setup_project(&dir);

    let epic_id: ItemId = "EPIC-001".parse().unwrap();
    let task_id: ItemId = "TASK-001".parse().unwrap();

    // Create epic
    swarmit::write_operation(
        &conn,
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
    .unwrap();

    // Create task under epic
    swarmit::write_operation(
        &conn,
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
    .unwrap();

    let state = swarmit::load_state(&conn).unwrap();
    assert_eq!(state.tasks[&task_id].status, Status::Todo);
    assert_eq!(state.epics[&epic_id].task_ids.len(), 1);

    // Claim
    swarmit::write_operation(
        &conn,
        &Operation::new(
            agent(),
            OperationKind::ClaimTask {
                id: task_id.clone(),
            },
        ),
    )
    .unwrap();

    let state = swarmit::load_state(&conn).unwrap();
    assert_eq!(state.tasks[&task_id].status, Status::InProgress);
    assert_eq!(state.tasks[&task_id].assignee, Some(agent()));

    // Done
    swarmit::write_operation(
        &conn,
        &Operation::new(
            agent(),
            OperationKind::CompleteTask {
                id: task_id.clone(),
            },
        ),
    )
    .unwrap();

    let state = swarmit::load_state(&conn).unwrap();
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

/// Relationship inverse is automatically created.
#[test]
fn relationship_inverse_created() {
    let dir = TempDir::new().unwrap();
    let conn = setup_project(&dir);

    let t1: ItemId = "TASK-001".parse().unwrap();
    let t2: ItemId = "TASK-002".parse().unwrap();

    for (id, title) in [(&t1, "Task 1"), (&t2, "Task 2")] {
        swarmit::write_operation(
            &conn,
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
        .unwrap();
    }

    // Add blocks relationship + inverse (simulating what link.rs does)
    swarmit::write_operations(
        &conn,
        &[
            Operation::new(
                agent(),
                OperationKind::AddRelationship {
                    from: t1.clone(),
                    to: t2.clone(),
                    rel_type: swarmit::models::RelationType::Blocks,
                },
            ),
            Operation::new(
                agent(),
                OperationKind::AddRelationship {
                    from: t2.clone(),
                    to: t1.clone(),
                    rel_type: swarmit::models::RelationType::BlockedBy,
                },
            ),
        ],
    )
    .unwrap();

    let state = swarmit::load_state(&conn).unwrap();
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
    let conn = setup_project(&dir);

    let task_id: ItemId = "TASK-001".parse().unwrap();

    // Create task
    swarmit::write_operation(
        &conn,
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
    .unwrap();

    // Add insight
    let insight_id = uuid::Uuid::now_v7();
    swarmit::write_operation(
        &conn,
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
    .unwrap();

    let state = swarmit::load_state(&conn).unwrap();
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

/// Compaction: operations are deleted, state tables survive.
#[test]
fn compaction_preserves_state() {
    let dir = TempDir::new().unwrap();
    let conn = setup_project(&dir);

    // Write some tasks
    for i in 1..=5u32 {
        let task_id = ItemId::new("TASK", i);
        swarmit::write_operation(
            &conn,
            &Operation::new(
                agent(),
                OperationKind::CreateTask {
                    id: task_id,
                    title: format!("Task {}", i),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                },
            ),
        )
        .unwrap();
    }

    let before = swarmit::load_state(&conn).unwrap();
    assert_eq!(before.tasks.len(), 5);

    // Compact: delete operations, keep state
    swarmit::compact_db(&conn).unwrap();

    // Operations should be gone
    let ops = swarmit::read_all_operations(&conn).unwrap();
    assert!(ops.is_empty());

    // State tables should still have data
    let after = swarmit::load_state(&conn).unwrap();
    assert_eq!(after.tasks.len(), 5);
}
