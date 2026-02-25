use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

use swarmit_core::events::log::{append_operation, read_operations, read_operations_since};
use swarmit_core::events::locking::try_append_with_timeout;
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::{AgentId, ItemId, Priority};
use swarmit_core::state::ProjectState;

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

/// Test: rebuild from log matches state built incrementally.
#[test]
fn rebuild_from_log_matches_incremental() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("operations.log");
    let lock_path = dir.path().join("operations.lock");

    let ops = vec![
        init_op("Test Project"),
        epic_op("EPIC-001", "Auth"),
        task_op("TASK-001", "Login"),
        task_op("TASK-002", "Signup"),
    ];

    // Write all ops
    for op in &ops {
        try_append_with_timeout(&lock_path, || append_operation(&log_path, op)).unwrap();
    }

    // Rebuild from log
    let state = ProjectState::from_log(&log_path).unwrap();

    assert!(state.config.is_some());
    assert_eq!(state.epics.len(), 1);
    assert_eq!(state.tasks.len(), 2);
    assert_eq!(state.sequence, 4);
}

/// Test: concurrent appends from multiple threads don't corrupt the log.
#[test]
fn concurrent_appends_are_safe() {
    let dir = TempDir::new().unwrap();
    let log_path = Arc::new(dir.path().join("operations.log"));
    let lock_path = Arc::new(dir.path().join("operations.lock"));

    const THREADS: usize = 8;
    const OPS_PER_THREAD: usize = 10;

    let barrier = Arc::new(Barrier::new(THREADS));

    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let log = Arc::clone(&log_path);
            let lock = Arc::clone(&lock_path);
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
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
                    try_append_with_timeout(&lock, || append_operation(&log, &op)).unwrap();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // All operations should be readable and valid
    let ops = read_operations(&log_path).unwrap();
    assert_eq!(ops.len(), THREADS * OPS_PER_THREAD);
}

/// Test: incremental read from byte offset.
#[test]
fn incremental_read_returns_new_ops() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("operations.log");
    let lock_path = dir.path().join("operations.lock");

    // Write first batch
    let op1 = init_op("Project");
    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op1)).unwrap();

    let (ops, offset) = read_operations_since(&log_path, 0).unwrap();
    assert_eq!(ops.len(), 1);
    assert!(offset > 0);

    // Write second batch
    let op2 = epic_op("EPIC-001", "Auth");
    try_append_with_timeout(&lock_path, || append_operation(&log_path, &op2)).unwrap();

    let (new_ops, new_offset) = read_operations_since(&log_path, offset).unwrap();
    assert_eq!(new_ops.len(), 1); // Only the new op
    assert!(new_offset > offset);

    // No new ops
    let (no_ops, _) = read_operations_since(&log_path, new_offset).unwrap();
    assert_eq!(no_ops.len(), 0);
}

/// Test: lock timeout when lock is held by another thread.
#[test]
fn lock_timeout_fires() {
    use swarmit_core::events::locking::with_exclusive_lock;

    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("operations.lock");
    let lock_path2 = lock_path.clone();

    let barrier = Arc::new(Barrier::new(2));
    let barrier2 = Arc::clone(&barrier);
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let release2 = Arc::clone(&release);

    // Thread 1: holds the lock for a bit
    let h = thread::spawn(move || {
        with_exclusive_lock(&lock_path2, 5_000, || {
            barrier2.wait(); // Signal: lock acquired
            // Hold for 200ms
            thread::sleep(std::time::Duration::from_millis(200));
            release2.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        })
        .unwrap();
    });

    barrier.wait(); // Wait for thread 1 to acquire lock

    // Thread 2: try with 50ms timeout — should fail
    let result = with_exclusive_lock::<_, ()>(&lock_path, 50, || Ok(()));
    assert!(result.is_err(), "Expected lock timeout error");

    h.join().unwrap();
}

/// Test: round-trip create → claim → done via ProjectState.
#[test]
fn task_lifecycle_round_trip() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("operations.log");
    let lock_path = dir.path().join("operations.lock");

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
        Operation::new(agent("alice"), OperationKind::ClaimTask { id: task_id.clone() }),
        Operation::new(agent("alice"), OperationKind::CompleteTask { id: task_id.clone() }),
    ];

    for op in &ops {
        try_append_with_timeout(&lock_path, || append_operation(&log_path, op)).unwrap();
    }

    let state = ProjectState::from_log(&log_path).unwrap();
    let task = &state.tasks[&task_id];

    assert_eq!(task.status, swarmit_core::models::Status::Done);
    assert!(task.claimed_at.is_some());
    assert!(task.completed_at.is_some());
    assert_eq!(task.assignee, Some(agent("alice")));
}
