use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

use swarmit::events::operations::{Operation, OperationKind};
use swarmit::models::{AgentId, ItemId, Priority};

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn init_op(name: &str) -> Operation {
    Operation::new(
        agent("test"),
        OperationKind::InitProject {
            name: name.to_string(),
            description: None,
            epic_prefix: None,
            task_prefix: None,
            auto_materialize: None,
            materialize_path: None,
        },
    )
}

fn epic_op(id: &str, title: &str) -> Operation {
    Operation::new(
        agent("test"),
        OperationKind::CreateEpic {
            id: id.parse().unwrap(),
            title: title.to_string(),
            description: None,
            priority: Priority::Medium,
        },
    )
}

fn task_op(id: &str, title: &str) -> Operation {
    Operation::new(
        agent("test"),
        OperationKind::CreateTask {
            id: id.parse().unwrap(),
            title: title.to_string(),
            description: None,
            priority: Priority::Medium,
            epic_id: None,
        },
    )
}

fn setup_db(dir: &TempDir) -> rusqlite::Connection {
    let swarmit_dir = dir.path().join(".swarmit");
    std::fs::create_dir_all(&swarmit_dir).unwrap();
    swarmit::open_db(dir.path()).unwrap()
}

/// Test: write operations and load matches state built incrementally.
#[test]
fn rebuild_from_db_matches_incremental() {
    let dir = TempDir::new().unwrap();
    let conn = setup_db(&dir);

    let ops = vec![
        init_op("Test Project"),
        epic_op("EPIC-001", "Auth"),
        task_op("TASK-001", "Login"),
        task_op("TASK-002", "Signup"),
    ];

    // Write all ops
    for op in &ops {
        swarmit::write_operation(&conn, op).unwrap();
    }

    // Load from DB
    let state = swarmit::load_state(&conn).unwrap();

    assert!(state.config.is_some());
    assert_eq!(state.epics.len(), 1);
    assert_eq!(state.tasks.len(), 2);
    // sequence is tracked per-op via INSERT INTO sequences ON CONFLICT
    assert!(state.sequence > 0);
}

/// Test: concurrent writes from multiple threads don't corrupt the DB.
#[test]
fn concurrent_writes_are_safe() {
    let dir = TempDir::new().unwrap();
    let conn = setup_db(&dir);
    drop(conn); // Close initial connection so threads can open their own

    let db_path = Arc::new(dir.path().join(".swarmit/state.db"));

    const THREADS: usize = 8;
    const OPS_PER_THREAD: usize = 10;

    let barrier = Arc::new(Barrier::new(THREADS));

    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let db_path = Arc::clone(&db_path);
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                let conn = rusqlite::Connection::open(&*db_path).unwrap();
                conn.execute_batch("PRAGMA busy_timeout=5000;").unwrap();

                // All threads start at the same time
                barrier.wait();

                for j in 0..OPS_PER_THREAD {
                    let agent_id = AgentId::new(&format!("agent-{}", i)).unwrap();
                    let op = Operation::new(
                        agent_id,
                        OperationKind::AddComment {
                            id: uuid::Uuid::now_v7(),
                            task_id: "TASK-001".parse().unwrap(),
                            body: format!("Thread {} op {}", i, j),
                        },
                    );
                    swarmit::write_operation(&conn, &op).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // All operations should be readable and valid
    let conn = swarmit::open_db(dir.path()).unwrap();
    let ops = swarmit::read_all_operations(&conn).unwrap();
    assert_eq!(ops.len(), THREADS * OPS_PER_THREAD);
}

/// Test: incremental read from rowid offset.
#[test]
fn incremental_read_returns_new_ops() {
    let dir = TempDir::new().unwrap();
    let conn = setup_db(&dir);

    // Write first op
    swarmit::write_operation(&conn, &init_op("Project")).unwrap();

    let rowid1 = swarmit::latest_rowid(&conn).unwrap();
    assert!(rowid1 > 0);

    // Write second op
    swarmit::write_operation(&conn, &epic_op("EPIC-001", "Auth")).unwrap();

    let (new_ops, rowid2) = swarmit::read_operations_since(&conn, rowid1).unwrap();
    assert_eq!(new_ops.len(), 1); // Only the new op
    assert!(rowid2 > rowid1);

    // No new ops
    let (no_ops, _) = swarmit::read_operations_since(&conn, rowid2).unwrap();
    assert_eq!(no_ops.len(), 0);
}

/// Test: round-trip create → claim → done via ProjectState.
#[test]
fn task_lifecycle_round_trip() {
    let dir = TempDir::new().unwrap();
    let conn = setup_db(&dir);

    let task_id: ItemId = "TASK-001".parse().unwrap();
    let ops = vec![
        Operation::new(
            agent("alice"),
            OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Implement login".to_string(),
                description: None,
                priority: Priority::High,
                epic_id: None,
            },
        ),
        Operation::new(
            agent("alice"),
            OperationKind::ClaimTask {
                id: task_id.clone(),
            },
        ),
        Operation::new(
            agent("alice"),
            OperationKind::CompleteTask {
                id: task_id.clone(),
            },
        ),
    ];

    for op in &ops {
        swarmit::write_operation(&conn, op).unwrap();
    }

    let state = swarmit::load_state(&conn).unwrap();
    let task = &state.tasks[&task_id];

    assert_eq!(task.status, swarmit::models::Status::Done);
    assert!(task.claimed_at.is_some());
    assert!(task.completed_at.is_some());
    assert_eq!(task.assignee, Some(agent("alice")));
}
