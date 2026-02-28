use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::events::operations::{Operation, OperationKind};
use crate::models::{
    AgentId, Comment, Epic, Insight, ItemId, Priority, ProjectConfig, RelationType, Relationship,
    Status, SwarmitError, Task,
};
use crate::state::ProjectState;

// ── Conversion helpers ───────────────────────────────────────────────────

/// Serialize a serde enum value to its snake_case string representation.
fn enum_to_str<T: Serialize>(val: &T) -> String {
    let json = serde_json::to_value(val).expect("enum serialization");
    json.as_str()
        .expect("enum should serialize as string")
        .to_string()
}

/// Deserialize a snake_case string to a serde enum value.
fn str_to_enum<T: for<'de> Deserialize<'de>>(s: &str) -> std::result::Result<T, String> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| e.to_string())
}

fn dt_to_str(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn str_to_dt(s: &str) -> std::result::Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| e.to_string())
}

fn sqlite_err(e: rusqlite::Error) -> SwarmitError {
    SwarmitError::Io(std::io::Error::other(e.to_string()))
}

// ── Migration SQL ────────────────────────────────────────────────────────

const MIGRATION_V1_SQL: &str = "
CREATE TABLE IF NOT EXISTS migrations (
    version    INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS operations (
    rowid     INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid      TEXT NOT NULL UNIQUE,
    agent     TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    kind      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS config (
    name              TEXT NOT NULL,
    description       TEXT,
    epic_prefix       TEXT NOT NULL DEFAULT 'EPIC',
    task_prefix       TEXT NOT NULL DEFAULT 'TASK',
    auto_materialize  INTEGER NOT NULL DEFAULT 0,
    materialize_path  TEXT NOT NULL DEFAULT 'state',
    created_at        TEXT NOT NULL,
    created_by        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS epics (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    status      TEXT NOT NULL,
    priority    TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    created_by  TEXT NOT NULL,
    assignee    TEXT
);

CREATE TABLE IF NOT EXISTS epic_task_ids (
    epic_id  TEXT NOT NULL,
    task_id  TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (epic_id, task_id)
);

CREATE TABLE IF NOT EXISTS tasks (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    description  TEXT,
    status       TEXT NOT NULL,
    priority     TEXT NOT NULL,
    epic_id      TEXT,
    assignee     TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    created_by   TEXT NOT NULL,
    claimed_at   TEXT,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS relationships (
    from_id  TEXT NOT NULL,
    to_id    TEXT NOT NULL,
    rel_type TEXT NOT NULL,
    PRIMARY KEY (from_id, to_id, rel_type)
);

CREATE TABLE IF NOT EXISTS comments (
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL,
    author     TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS insights (
    id             TEXT PRIMARY KEY,
    task_id        TEXT NOT NULL,
    author         TEXT NOT NULL,
    file_path      TEXT NOT NULL,
    before_snippet TEXT,
    after_snippet  TEXT,
    body           TEXT NOT NULL,
    created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sequences (
    name  TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
";

// ── Public API ───────────────────────────────────────────────────────────

/// Open (or create) the project database at `<project_root>/.swarmit/state.db`.
///
/// Sets WAL mode, busy_timeout=5000, runs migrations, and imports any legacy
/// `operations.log` if present.
pub fn open_db(project_root: &Path) -> crate::models::Result<Connection> {
    let swarmit_dir = project_root.join(".swarmit");

    // Ensure the .swarmit directory exists
    if !swarmit_dir.exists() {
        std::fs::create_dir_all(&swarmit_dir)?;
    }

    let db_path = swarmit_dir.join("state.db");

    // If an old v1 snapshot DB exists (has `meta` table but no `migrations`), back it up.
    maybe_migrate_v1_snapshot(&db_path);

    let conn = Connection::open(&db_path).map_err(sqlite_err)?;

    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;
         PRAGMA foreign_keys=ON;",
    )
    .map_err(sqlite_err)?;

    run_migrations(&conn)?;

    // Import legacy JSONL log if present
    import_legacy_log(&conn, &swarmit_dir)?;

    Ok(conn)
}

/// Load the full `ProjectState` from the materialized tables.
///
/// Wraps all reads in an explicit transaction so that concurrent writers
/// cannot change the snapshot between individual SELECT statements.
pub fn load_state(conn: &Connection) -> crate::models::Result<ProjectState> {
    // BEGIN DEFERRED pins the snapshot at the first read, giving a consistent
    // view across all the queries below even while writers commit concurrently.
    conn.execute_batch("BEGIN DEFERRED").map_err(sqlite_err)?;

    let result = (|| {
        let config = read_config(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let mut epics = read_epics(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        read_epic_task_ids(conn, &mut epics)
            .map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let tasks = read_tasks(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let relationships =
            read_relationships(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let comments =
            read_comments(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let insights =
            read_insights(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let (epic_seq, task_seq, sequence) =
            read_sequences(conn).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;

        Ok(ProjectState {
            config,
            epics,
            tasks,
            relationships,
            comments,
            insights,
            epic_seq,
            task_seq,
            sequence,
        })
    })();

    // Always end the read transaction, even on error.
    let _ = conn.execute_batch("COMMIT");
    result
}

/// Write a single operation atomically: INSERT into operations + update materialized tables.
pub fn write_operation(conn: &Connection, op: &Operation) -> crate::models::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE").map_err(sqlite_err)?;

    match write_op_inner(conn, op) {
        Ok(()) => {
            conn.execute_batch("COMMIT").map_err(sqlite_err)?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Write multiple operations in a single atomic transaction.
pub fn write_operations(conn: &Connection, ops: &[Operation]) -> crate::models::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE").map_err(sqlite_err)?;

    for op in ops {
        if let Err(e) = write_op_inner(conn, op) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }

    conn.execute_batch("COMMIT").map_err(sqlite_err)?;
    Ok(())
}

/// Read operations with rowid greater than `after_rowid`.
/// Returns `(operations, latest_rowid)`.
pub fn read_operations_since(
    conn: &Connection,
    after_rowid: i64,
) -> crate::models::Result<(Vec<Operation>, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT uuid, agent, timestamp, kind FROM operations WHERE rowid > ?1 ORDER BY rowid",
        )
        .map_err(sqlite_err)?;
    let mut rows = stmt.query(params![after_rowid]).map_err(sqlite_err)?;
    let mut ops = Vec::new();

    while let Some(row) = rows.next().map_err(sqlite_err)? {
        let uuid_str: String = row.get(0).map_err(sqlite_err)?;
        let agent_str: String = row.get(1).map_err(sqlite_err)?;
        let ts_str: String = row.get(2).map_err(sqlite_err)?;
        let kind_json: String = row.get(3).map_err(sqlite_err)?;

        let id: uuid::Uuid = uuid::Uuid::parse_str(&uuid_str)
            .map_err(|e| SwarmitError::Io(std::io::Error::other(e.to_string())))?;
        let agent = AgentId::new(&agent_str)?;
        let timestamp =
            str_to_dt(&ts_str).map_err(|e| SwarmitError::Io(std::io::Error::other(e)))?;
        let kind: OperationKind = serde_json::from_str(&kind_json)?;

        ops.push(Operation {
            id,
            agent,
            timestamp,
            kind,
        });
    }

    let new_rowid = latest_rowid(conn)?;
    Ok((ops, new_rowid))
}

/// Read all operations from the database, ordered by rowid.
pub fn read_all_operations(conn: &Connection) -> crate::models::Result<Vec<Operation>> {
    let (ops, _) = read_operations_since(conn, 0)?;
    Ok(ops)
}

/// Return the maximum rowid from the operations table (0 if empty).
pub fn latest_rowid(conn: &Connection) -> crate::models::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(rowid), 0) FROM operations",
        [],
        |row| row.get(0),
    )
    .map_err(sqlite_err)
}

/// Count total operations in the database.
pub fn count_operations(conn: &Connection) -> crate::models::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
        .map_err(sqlite_err)
}

/// Delete all operations and VACUUM. Materialized state tables are left intact.
pub fn compact_db(conn: &Connection) -> crate::models::Result<()> {
    conn.execute_batch("DELETE FROM operations; VACUUM;")
        .map_err(sqlite_err)
}

// ── Internal: write a single op (must be called inside a transaction) ────

fn write_op_inner(conn: &Connection, op: &Operation) -> crate::models::Result<()> {
    let kind_json = serde_json::to_string(&op.kind)?;

    conn.execute(
        "INSERT INTO operations (uuid, agent, timestamp, kind) VALUES (?1, ?2, ?3, ?4)",
        params![
            op.id.to_string(),
            op.agent.as_str(),
            dt_to_str(&op.timestamp),
            kind_json,
        ],
    )
    .map_err(sqlite_err)?;

    // Update the global sequence counter
    conn.execute(
        "INSERT INTO sequences (name, value) VALUES ('sequence', 1)
         ON CONFLICT(name) DO UPDATE SET value = value + 1",
        [],
    )
    .map_err(sqlite_err)?;

    apply_to_db(conn, op)?;

    Ok(())
}

// ── apply_to_db: mirror of ProjectState::apply() but for SQL ─────────────

fn apply_to_db(conn: &Connection, op: &Operation) -> crate::models::Result<()> {
    match &op.kind {
        OperationKind::InitProject {
            name,
            description,
            epic_prefix,
            task_prefix,
            auto_materialize,
            materialize_path,
        } => {
            conn.execute("DELETE FROM config", []).map_err(sqlite_err)?;
            let ep = epic_prefix.as_deref().unwrap_or("EPIC");
            let tp = task_prefix.as_deref().unwrap_or("TASK");
            let am = auto_materialize.unwrap_or(false) as i32;
            let mp = materialize_path.as_deref().unwrap_or("state");
            conn.execute(
                "INSERT INTO config (name, description, epic_prefix, task_prefix,
                    auto_materialize, materialize_path, created_at, created_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    name,
                    description.as_deref(),
                    ep,
                    tp,
                    am,
                    mp,
                    dt_to_str(&op.timestamp),
                    op.agent.as_str(),
                ],
            )
            .map_err(sqlite_err)?;
            // Seed sequence counters
            conn.execute(
                "INSERT OR IGNORE INTO sequences (name, value) VALUES ('epic_seq', 0)",
                [],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "INSERT OR IGNORE INTO sequences (name, value) VALUES ('task_seq', 0)",
                [],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "INSERT OR IGNORE INTO sequences (name, value) VALUES ('sequence', 0)",
                [],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::UpdateProject {
            name,
            description,
            clear_description,
            auto_materialize,
            materialize_path,
        } => {
            if let Some(n) = name {
                conn.execute("UPDATE config SET name = ?1", params![n])
                    .map_err(sqlite_err)?;
            }
            if *clear_description {
                conn.execute("UPDATE config SET description = NULL", [])
                    .map_err(sqlite_err)?;
            } else if let Some(d) = description {
                conn.execute("UPDATE config SET description = ?1", params![d])
                    .map_err(sqlite_err)?;
            }
            if let Some(v) = auto_materialize {
                conn.execute(
                    "UPDATE config SET auto_materialize = ?1",
                    params![*v as i32],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(p) = materialize_path {
                conn.execute("UPDATE config SET materialize_path = ?1", params![p])
                    .map_err(sqlite_err)?;
            }
        }

        OperationKind::CreateEpic {
            id,
            title,
            description,
            priority,
        } => {
            conn.execute(
                "INSERT INTO epics (id, title, description, status, priority,
                    created_at, updated_at, created_by, assignee)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
                params![
                    id.as_str(),
                    title,
                    description.as_deref(),
                    enum_to_str(&Status::Todo),
                    enum_to_str(priority),
                    dt_to_str(&op.timestamp),
                    dt_to_str(&op.timestamp),
                    op.agent.as_str(),
                ],
            )
            .map_err(sqlite_err)?;
            // Update epic_seq
            if let Some(n) = id.number() {
                conn.execute(
                    "UPDATE sequences SET value = MAX(value, ?1) WHERE name = 'epic_seq'",
                    params![n],
                )
                .map_err(sqlite_err)?;
            }
        }

        OperationKind::UpdateEpic {
            id,
            title,
            description,
            priority,
            assignee,
        } => {
            if let Some(t) = title {
                conn.execute(
                    "UPDATE epics SET title = ?1 WHERE id = ?2",
                    params![t, id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(d) = description {
                conn.execute(
                    "UPDATE epics SET description = ?1 WHERE id = ?2",
                    params![d, id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(p) = priority {
                conn.execute(
                    "UPDATE epics SET priority = ?1 WHERE id = ?2",
                    params![enum_to_str(p), id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(a) = assignee {
                conn.execute(
                    "UPDATE epics SET assignee = ?1 WHERE id = ?2",
                    params![a.as_str(), id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            conn.execute(
                "UPDATE epics SET updated_at = ?1 WHERE id = ?2",
                params![dt_to_str(&op.timestamp), id.as_str()],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::UpdateEpicStatus { id, status } => {
            conn.execute(
                "UPDATE epics SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![enum_to_str(status), dt_to_str(&op.timestamp), id.as_str()],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::DeleteEpic { id } => {
            conn.execute(
                "DELETE FROM epic_task_ids WHERE epic_id = ?1",
                params![id.as_str()],
            )
            .map_err(sqlite_err)?;
            conn.execute("DELETE FROM epics WHERE id = ?1", params![id.as_str()])
                .map_err(sqlite_err)?;
        }

        OperationKind::CreateTask {
            id,
            title,
            description,
            priority,
            epic_id,
        } => {
            conn.execute(
                "INSERT OR IGNORE INTO tasks (id, title, description, status, priority,
                    epic_id, assignee, created_at, updated_at, created_by,
                    claimed_at, completed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, NULL, NULL)",
                params![
                    id.as_str(),
                    title,
                    description.as_deref(),
                    enum_to_str(&Status::Todo),
                    enum_to_str(priority),
                    epic_id.as_ref().map(|e| e.as_str().to_string()),
                    dt_to_str(&op.timestamp),
                    dt_to_str(&op.timestamp),
                    op.agent.as_str(),
                ],
            )
            .map_err(sqlite_err)?;

            // Add to epic's task list (if epic exists)
            if let Some(eid) = epic_id {
                // Get next position (avoid duplicate)
                let already: bool = conn
                    .query_row(
                        "SELECT COUNT(*) > 0 FROM epic_task_ids WHERE epic_id = ?1 AND task_id = ?2",
                        params![eid.as_str(), id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_err)?;
                if !already {
                    let next_pos: i32 = conn
                        .query_row(
                            "SELECT COALESCE(MAX(position), -1) + 1 FROM epic_task_ids WHERE epic_id = ?1",
                            params![eid.as_str()],
                            |row| row.get(0),
                        )
                        .map_err(sqlite_err)?;
                    conn.execute(
                        "INSERT INTO epic_task_ids (epic_id, task_id, position) VALUES (?1, ?2, ?3)",
                        params![eid.as_str(), id.as_str(), next_pos],
                    )
                    .map_err(sqlite_err)?;
                }
                // Re-open a Done epic
                conn.execute(
                    "UPDATE epics SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status = ?4",
                    params![
                        enum_to_str(&Status::InProgress),
                        dt_to_str(&op.timestamp),
                        eid.as_str(),
                        enum_to_str(&Status::Done),
                    ],
                )
                .map_err(sqlite_err)?;
            }

            // Update task_seq
            if let Some(n) = id.number() {
                conn.execute(
                    "UPDATE sequences SET value = MAX(value, ?1) WHERE name = 'task_seq'",
                    params![n],
                )
                .map_err(sqlite_err)?;
            }
        }

        OperationKind::UpdateTask {
            id,
            title,
            description,
            priority,
            epic_id,
            assignee,
        } => {
            // Capture old epic before update
            let old_epic_id: Option<String> = if epic_id.is_some() {
                conn.query_row(
                    "SELECT epic_id FROM tasks WHERE id = ?1",
                    params![id.as_str()],
                    |row| row.get(0),
                )
                .ok()
                .flatten()
            } else {
                None
            };

            if let Some(t) = title {
                conn.execute(
                    "UPDATE tasks SET title = ?1 WHERE id = ?2",
                    params![t, id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(d) = description {
                conn.execute(
                    "UPDATE tasks SET description = ?1 WHERE id = ?2",
                    params![d, id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(p) = priority {
                conn.execute(
                    "UPDATE tasks SET priority = ?1 WHERE id = ?2",
                    params![enum_to_str(p), id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            if let Some(eid_opt) = epic_id {
                conn.execute(
                    "UPDATE tasks SET epic_id = ?1 WHERE id = ?2",
                    params![
                        eid_opt.as_ref().map(|e| e.as_str().to_string()),
                        id.as_str()
                    ],
                )
                .map_err(sqlite_err)?;

                // Remove from old epic's task list
                if let Some(old_eid) = &old_epic_id {
                    conn.execute(
                        "DELETE FROM epic_task_ids WHERE epic_id = ?1 AND task_id = ?2",
                        params![old_eid, id.as_str()],
                    )
                    .map_err(sqlite_err)?;
                }

                // Add to new epic's task list
                if let Some(new_eid) = eid_opt {
                    let already: bool = conn
                        .query_row(
                            "SELECT COUNT(*) > 0 FROM epic_task_ids WHERE epic_id = ?1 AND task_id = ?2",
                            params![new_eid.as_str(), id.as_str()],
                            |row| row.get(0),
                        )
                        .map_err(sqlite_err)?;
                    if !already {
                        let next_pos: i32 = conn
                            .query_row(
                                "SELECT COALESCE(MAX(position), -1) + 1 FROM epic_task_ids WHERE epic_id = ?1",
                                params![new_eid.as_str()],
                                |row| row.get(0),
                            )
                            .map_err(sqlite_err)?;
                        conn.execute(
                            "INSERT INTO epic_task_ids (epic_id, task_id, position) VALUES (?1, ?2, ?3)",
                            params![new_eid.as_str(), id.as_str(), next_pos],
                        )
                        .map_err(sqlite_err)?;
                    }
                    // Re-open a Done epic
                    conn.execute(
                        "UPDATE epics SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status = ?4",
                        params![
                            enum_to_str(&Status::InProgress),
                            dt_to_str(&op.timestamp),
                            new_eid.as_str(),
                            enum_to_str(&Status::Done),
                        ],
                    )
                    .map_err(sqlite_err)?;

                    check_epic_completion_db(conn, new_eid, op.timestamp)?;
                }

                // Check old epic completion
                if let Some(old_eid_str) = &old_epic_id {
                    if let Ok(old_eid) = old_eid_str.parse::<ItemId>() {
                        check_epic_completion_db(conn, &old_eid, op.timestamp)?;
                    }
                }
            }
            if let Some(a) = assignee {
                conn.execute(
                    "UPDATE tasks SET assignee = ?1 WHERE id = ?2",
                    params![a.as_ref().map(|a| a.as_str().to_string()), id.as_str()],
                )
                .map_err(sqlite_err)?;
            }
            conn.execute(
                "UPDATE tasks SET updated_at = ?1 WHERE id = ?2",
                params![dt_to_str(&op.timestamp), id.as_str()],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::UpdateTaskStatus { id, status } => {
            conn.execute(
                "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![enum_to_str(status), dt_to_str(&op.timestamp), id.as_str()],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::ClaimTask { id } => {
            conn.execute(
                "UPDATE tasks SET assignee = ?1, status = ?2, claimed_at = ?3, updated_at = ?4 WHERE id = ?5",
                params![
                    op.agent.as_str(),
                    enum_to_str(&Status::InProgress),
                    dt_to_str(&op.timestamp),
                    dt_to_str(&op.timestamp),
                    id.as_str(),
                ],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::CompleteTask { id } => {
            conn.execute(
                "UPDATE tasks SET status = ?1, completed_at = ?2, updated_at = ?3 WHERE id = ?4",
                params![
                    enum_to_str(&Status::Done),
                    dt_to_str(&op.timestamp),
                    dt_to_str(&op.timestamp),
                    id.as_str(),
                ],
            )
            .map_err(sqlite_err)?;

            // Check epic completion
            let epic_id: Option<String> = conn
                .query_row(
                    "SELECT epic_id FROM tasks WHERE id = ?1",
                    params![id.as_str()],
                    |row| row.get(0),
                )
                .ok()
                .flatten();
            if let Some(eid_str) = &epic_id {
                if let Ok(eid) = eid_str.parse::<ItemId>() {
                    check_epic_completion_db(conn, &eid, op.timestamp)?;
                }
            }
        }

        OperationKind::DeleteTask { id } => {
            // Capture epic before deletion
            let epic_id: Option<String> = conn
                .query_row(
                    "SELECT epic_id FROM tasks WHERE id = ?1",
                    params![id.as_str()],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            conn.execute("DELETE FROM tasks WHERE id = ?1", params![id.as_str()])
                .map_err(sqlite_err)?;
            conn.execute(
                "DELETE FROM relationships WHERE from_id = ?1 OR to_id = ?1",
                params![id.as_str()],
            )
            .map_err(sqlite_err)?;
            conn.execute(
                "DELETE FROM epic_task_ids WHERE task_id = ?1",
                params![id.as_str()],
            )
            .map_err(sqlite_err)?;

            if let Some(eid_str) = &epic_id {
                if let Ok(eid) = eid_str.parse::<ItemId>() {
                    check_epic_completion_db(conn, &eid, op.timestamp)?;
                }
            }
        }

        OperationKind::AddRelationship { from, to, rel_type } => {
            conn.execute(
                "INSERT OR IGNORE INTO relationships (from_id, to_id, rel_type) VALUES (?1, ?2, ?3)",
                params![from.as_str(), to.as_str(), enum_to_str(rel_type)],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::RemoveRelationship { from, to, rel_type } => {
            conn.execute(
                "DELETE FROM relationships WHERE from_id = ?1 AND to_id = ?2 AND rel_type = ?3",
                params![from.as_str(), to.as_str(), enum_to_str(rel_type)],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::AddComment { id, task_id, body } => {
            conn.execute(
                "INSERT INTO comments (id, task_id, author, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id.to_string(),
                    task_id.as_str(),
                    op.agent.as_str(),
                    body,
                    dt_to_str(&op.timestamp),
                ],
            )
            .map_err(sqlite_err)?;
        }

        OperationKind::AddInsight {
            id,
            task_id,
            file_path,
            before_snippet,
            after_snippet,
            body,
        } => {
            conn.execute(
                "INSERT INTO insights (id, task_id, author, file_path,
                    before_snippet, after_snippet, body, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id.to_string(),
                    task_id.as_str(),
                    op.agent.as_str(),
                    file_path,
                    before_snippet.as_deref(),
                    after_snippet.as_deref(),
                    body,
                    dt_to_str(&op.timestamp),
                ],
            )
            .map_err(sqlite_err)?;
        }
    }

    Ok(())
}

/// Auto-close an epic if it has tasks and all are Done.
fn check_epic_completion_db(
    conn: &Connection,
    epic_id: &ItemId,
    timestamp: DateTime<Utc>,
) -> crate::models::Result<()> {
    // Count total tasks in epic
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM epic_task_ids WHERE epic_id = ?1",
            params![epic_id.as_str()],
            |row| row.get(0),
        )
        .map_err(sqlite_err)?;

    if total == 0 {
        return Ok(());
    }

    // Count non-done tasks
    let non_done: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM epic_task_ids et
             JOIN tasks t ON et.task_id = t.id
             WHERE et.epic_id = ?1 AND t.status != ?2",
            params![epic_id.as_str(), enum_to_str(&Status::Done)],
            |row| row.get(0),
        )
        .map_err(sqlite_err)?;

    if non_done == 0 {
        conn.execute(
            "UPDATE epics SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                enum_to_str(&Status::Done),
                dt_to_str(&timestamp),
                epic_id.as_str(),
            ],
        )
        .map_err(sqlite_err)?;
    }

    Ok(())
}

// ── Migrations ───────────────────────────────────────────────────────────

fn run_migrations(conn: &Connection) -> crate::models::Result<()> {
    // Check if migrations table exists
    let has_migrations: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='migrations'",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_err)?;

    let current_version: i64 = if has_migrations {
        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM migrations",
            [],
            |row| row.get(0),
        )
        .map_err(sqlite_err)?
    } else {
        0
    };

    if current_version < 1 {
        conn.execute_batch(MIGRATION_V1_SQL).map_err(sqlite_err)?;
        conn.execute(
            "INSERT OR IGNORE INTO migrations (version, name, applied_at) VALUES (1, 'initial_schema', ?1)",
            params![dt_to_str(&Utc::now())],
        )
        .map_err(sqlite_err)?;
    }

    Ok(())
}

// ── Legacy migration helpers ─────────────────────────────────────────────

/// If `state.db` exists and has the old `meta` table but no `migrations` table,
/// rename it to `state.db.v1.bak.<timestamp>` so we start fresh.
fn maybe_migrate_v1_snapshot(db_path: &Path) {
    if !db_path.exists() {
        return;
    }

    // Try to open and check for old schema
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let has_meta: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    let has_migrations: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='migrations'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    drop(conn);

    if has_meta && !has_migrations {
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let backup_name = format!(
            "{}.v1.bak.{}",
            db_path.file_name().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let backup_path = db_path.with_file_name(&backup_name);
        match std::fs::rename(db_path, &backup_path) {
            Ok(()) => {
                eprintln!(
                    "Migrated v1 snapshot DB: {} → {} (safe to delete)",
                    db_path.display(),
                    backup_name
                );
            }
            Err(e) => {
                eprintln!("Warning: could not rename old snapshot DB: {e}");
            }
        }
    }
}

/// Import operations from legacy `operations.log` JSONL file into the database.
fn import_legacy_log(conn: &Connection, swarmit_dir: &Path) -> crate::models::Result<()> {
    let log_path = swarmit_dir.join("operations.log");
    if !log_path.exists() {
        return Ok(());
    }

    // Check if DB already has operations (skip if already imported)
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
        .map_err(sqlite_err)?;
    if count > 0 {
        // Already have operations, just clean up the legacy file
        rename_legacy_file(&log_path);
        // Also clean up lock file
        let lock_path = swarmit_dir.join("operations.lock");
        if lock_path.exists() {
            let _ = std::fs::remove_file(&lock_path);
        }
        return Ok(());
    }

    // Read all operations from the JSONL file
    use std::io::BufRead;
    let file = std::fs::File::open(&log_path)?;
    let reader = std::io::BufReader::new(file);
    let mut ops = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Operation>(trimmed) {
            Ok(op) => ops.push(op),
            Err(e) => {
                eprintln!("Warning: skipping corrupted operation during import: {e}");
            }
        }
    }

    if !ops.is_empty() {
        eprintln!(
            "Importing {} operations from operations.log into state.db...",
            ops.len()
        );
        // Write all ops in a single transaction
        conn.execute_batch("BEGIN IMMEDIATE").map_err(sqlite_err)?;
        for op in &ops {
            if let Err(e) = write_op_inner(conn, op) {
                eprintln!("Warning: failed to import operation {}: {e}", op.id);
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        conn.execute_batch("COMMIT").map_err(sqlite_err)?;
        eprintln!("Import complete.");
    }

    // Rename legacy files
    rename_legacy_file(&log_path);
    let lock_path = swarmit_dir.join("operations.lock");
    if lock_path.exists() {
        let _ = std::fs::remove_file(&lock_path);
    }
    // Also rename legacy state.snap if present
    let snap_path = swarmit_dir.join("state.snap");
    if snap_path.exists() {
        rename_legacy_file(&snap_path);
    }

    Ok(())
}

fn rename_legacy_file(path: &Path) {
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let backup_name = format!("{}.bak.{}", file_name, timestamp);
    let backup_path = path.with_file_name(&backup_name);
    match std::fs::rename(path, &backup_path) {
        Ok(()) => {
            eprintln!("Renamed {} → {} (safe to delete)", file_name, backup_name);
        }
        Err(e) => {
            eprintln!("Warning: could not rename {}: {e}", file_name);
        }
    }
}

// ── Read helpers (reused from former snapshot.rs) ────────────────────────

fn read_config(conn: &Connection) -> std::result::Result<Option<ProjectConfig>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, description, epic_prefix, task_prefix,
                    auto_materialize, materialize_path, created_at, created_by
             FROM config LIMIT 1",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    let Some(row) = rows.next().map_err(|e| e.to_string())? else {
        return Ok(None);
    };

    Ok(Some(ProjectConfig {
        name: row.get::<_, String>(0).map_err(|e| e.to_string())?,
        description: row.get(1).map_err(|e| e.to_string())?,
        epic_prefix: row.get(2).map_err(|e| e.to_string())?,
        task_prefix: row.get(3).map_err(|e| e.to_string())?,
        auto_materialize: row.get::<_, i32>(4).map_err(|e| e.to_string())? != 0,
        materialize_path: row.get(5).map_err(|e| e.to_string())?,
        created_at: str_to_dt(&row.get::<_, String>(6).map_err(|e| e.to_string())?)?,
        created_by: AgentId::new(&row.get::<_, String>(7).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    }))
}

fn read_epics(conn: &Connection) -> std::result::Result<BTreeMap<ItemId, Epic>, String> {
    let mut epics = BTreeMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, status, priority,
                    created_at, updated_at, created_by, assignee
             FROM epics",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: ItemId = row
            .get::<_, String>(0)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e: SwarmitError| e.to_string())?;
        let assignee: Option<String> = row.get(8).map_err(|e| e.to_string())?;

        epics.insert(
            id.clone(),
            Epic {
                id,
                title: row.get(1).map_err(|e| e.to_string())?,
                description: row.get(2).map_err(|e| e.to_string())?,
                status: str_to_enum(&row.get::<_, String>(3).map_err(|e| e.to_string())?)?,
                priority: str_to_enum::<Priority>(
                    &row.get::<_, String>(4).map_err(|e| e.to_string())?,
                )?,
                created_at: str_to_dt(&row.get::<_, String>(5).map_err(|e| e.to_string())?)?,
                updated_at: str_to_dt(&row.get::<_, String>(6).map_err(|e| e.to_string())?)?,
                created_by: AgentId::new(&row.get::<_, String>(7).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
                assignee: assignee
                    .map(|a| AgentId::new(&a))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                task_ids: Vec::new(), // populated by read_epic_task_ids
            },
        );
    }
    Ok(epics)
}

fn read_epic_task_ids(
    conn: &Connection,
    epics: &mut BTreeMap<ItemId, Epic>,
) -> std::result::Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT epic_id, task_id FROM epic_task_ids ORDER BY epic_id, position")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let epic_id: ItemId = row
            .get::<_, String>(0)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e: SwarmitError| e.to_string())?;
        let task_id: ItemId = row
            .get::<_, String>(1)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e: SwarmitError| e.to_string())?;

        if let Some(epic) = epics.get_mut(&epic_id) {
            epic.task_ids.push(task_id);
        }
    }
    Ok(())
}

fn read_tasks(conn: &Connection) -> std::result::Result<BTreeMap<ItemId, Task>, String> {
    let mut tasks = BTreeMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, title, description, status, priority, epic_id,
                    assignee, created_at, updated_at, created_by,
                    claimed_at, completed_at
             FROM tasks",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let id: ItemId = row
            .get::<_, String>(0)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e: SwarmitError| e.to_string())?;
        let epic_id_str: Option<String> = row.get(5).map_err(|e| e.to_string())?;
        let assignee: Option<String> = row.get(6).map_err(|e| e.to_string())?;
        let claimed_at: Option<String> = row.get(10).map_err(|e| e.to_string())?;
        let completed_at: Option<String> = row.get(11).map_err(|e| e.to_string())?;

        tasks.insert(
            id.clone(),
            Task {
                id,
                title: row.get(1).map_err(|e| e.to_string())?,
                description: row.get(2).map_err(|e| e.to_string())?,
                status: str_to_enum::<Status>(
                    &row.get::<_, String>(3).map_err(|e| e.to_string())?,
                )?,
                priority: str_to_enum::<Priority>(
                    &row.get::<_, String>(4).map_err(|e| e.to_string())?,
                )?,
                epic_id: epic_id_str
                    .map(|s| s.parse())
                    .transpose()
                    .map_err(|e: SwarmitError| e.to_string())?,
                assignee: assignee
                    .map(|a| AgentId::new(&a))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                created_at: str_to_dt(&row.get::<_, String>(7).map_err(|e| e.to_string())?)?,
                updated_at: str_to_dt(&row.get::<_, String>(8).map_err(|e| e.to_string())?)?,
                created_by: AgentId::new(&row.get::<_, String>(9).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
                claimed_at: claimed_at.map(|s| str_to_dt(&s)).transpose()?,
                completed_at: completed_at.map(|s| str_to_dt(&s)).transpose()?,
            },
        );
    }
    Ok(tasks)
}

fn read_relationships(conn: &Connection) -> std::result::Result<Vec<Relationship>, String> {
    let mut rels = Vec::new();
    let mut stmt = conn
        .prepare("SELECT from_id, to_id, rel_type FROM relationships")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        rels.push(Relationship {
            from: row
                .get::<_, String>(0)
                .map_err(|e| e.to_string())?
                .parse()
                .map_err(|e: SwarmitError| e.to_string())?,
            to: row
                .get::<_, String>(1)
                .map_err(|e| e.to_string())?
                .parse()
                .map_err(|e: SwarmitError| e.to_string())?,
            rel_type: str_to_enum::<RelationType>(
                &row.get::<_, String>(2).map_err(|e| e.to_string())?,
            )?,
        });
    }
    Ok(rels)
}

fn read_comments(conn: &Connection) -> std::result::Result<BTreeMap<ItemId, Vec<Comment>>, String> {
    let mut comments: BTreeMap<ItemId, Vec<Comment>> = BTreeMap::new();
    let mut stmt = conn
        .prepare("SELECT id, task_id, author, body, created_at FROM comments ORDER BY id")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let task_id: ItemId = row
            .get::<_, String>(1)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e: SwarmitError| e.to_string())?;

        comments.entry(task_id.clone()).or_default().push(Comment {
            id: uuid::Uuid::parse_str(&row.get::<_, String>(0).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            task_id,
            author: AgentId::new(&row.get::<_, String>(2).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            body: row.get(3).map_err(|e| e.to_string())?,
            created_at: str_to_dt(&row.get::<_, String>(4).map_err(|e| e.to_string())?)?,
        });
    }
    Ok(comments)
}

fn read_insights(conn: &Connection) -> std::result::Result<BTreeMap<ItemId, Vec<Insight>>, String> {
    let mut insights: BTreeMap<ItemId, Vec<Insight>> = BTreeMap::new();
    let mut stmt = conn
        .prepare(
            "SELECT id, task_id, author, file_path, before_snippet,
                    after_snippet, body, created_at
             FROM insights ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let task_id: ItemId = row
            .get::<_, String>(1)
            .map_err(|e| e.to_string())?
            .parse()
            .map_err(|e: SwarmitError| e.to_string())?;

        insights.entry(task_id.clone()).or_default().push(Insight {
            id: uuid::Uuid::parse_str(&row.get::<_, String>(0).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            task_id,
            author: AgentId::new(&row.get::<_, String>(2).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
            file_path: row.get(3).map_err(|e| e.to_string())?,
            before_snippet: row.get(4).map_err(|e| e.to_string())?,
            after_snippet: row.get(5).map_err(|e| e.to_string())?,
            body: row.get(6).map_err(|e| e.to_string())?,
            created_at: str_to_dt(&row.get::<_, String>(7).map_err(|e| e.to_string())?)?,
        });
    }
    Ok(insights)
}

fn read_sequences(conn: &Connection) -> std::result::Result<(u32, u32, u64), String> {
    let mut stmt = conn
        .prepare("SELECT name, value FROM sequences")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    let mut epic_seq: u32 = 0;
    let mut task_seq: u32 = 0;
    let mut sequence: u64 = 0;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let name: String = row.get(0).map_err(|e| e.to_string())?;
        let value: i64 = row.get(1).map_err(|e| e.to_string())?;
        match name.as_str() {
            "epic_seq" => epic_seq = value as u32,
            "task_seq" => task_seq = value as u32,
            "sequence" => sequence = value as u64,
            _ => {}
        }
    }
    Ok((epic_seq, task_seq, sequence))
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::operations::OperationKind;
    use crate::models::Priority;
    use tempfile::tempdir;

    fn agent() -> AgentId {
        AgentId::new("test-agent").unwrap()
    }

    fn make_op(kind: OperationKind) -> Operation {
        Operation::new(agent(), kind)
    }

    fn setup_db() -> (tempfile::TempDir, Connection) {
        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();
        let conn = open_db(dir.path()).unwrap();
        (dir, conn)
    }

    #[test]
    fn test_open_creates_schema() {
        let (_dir, conn) = setup_db();

        // Migrations table should exist with version 1
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);

        // Operations table should exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_round_trip() {
        let (_dir, conn) = setup_db();

        let ops = vec![
            make_op(OperationKind::InitProject {
                name: "Test Project".to_string(),
                description: Some("A project".to_string()),
                epic_prefix: Some("EP".to_string()),
                task_prefix: Some("TK".to_string()),
                auto_materialize: Some(true),
                materialize_path: Some("docs/state".to_string()),
            }),
            make_op(OperationKind::CreateEpic {
                id: "EP-001".parse().unwrap(),
                title: "Auth Epic".to_string(),
                description: Some("Auth stuff".to_string()),
                priority: Priority::High,
            }),
            make_op(OperationKind::CreateTask {
                id: "TK-001".parse().unwrap(),
                title: "Login".to_string(),
                description: Some("Build login".to_string()),
                priority: Priority::High,
                epic_id: Some("EP-001".parse().unwrap()),
            }),
            make_op(OperationKind::CreateTask {
                id: "TK-002".parse().unwrap(),
                title: "Logout".to_string(),
                description: None,
                priority: Priority::Low,
                epic_id: Some("EP-001".parse().unwrap()),
            }),
            make_op(OperationKind::ClaimTask {
                id: "TK-001".parse().unwrap(),
            }),
            make_op(OperationKind::CompleteTask {
                id: "TK-001".parse().unwrap(),
            }),
            make_op(OperationKind::AddRelationship {
                from: "TK-001".parse().unwrap(),
                to: "TK-002".parse().unwrap(),
                rel_type: RelationType::Blocks,
            }),
            make_op(OperationKind::AddComment {
                id: uuid::Uuid::now_v7(),
                task_id: "TK-001".parse().unwrap(),
                body: "Great work!".to_string(),
            }),
            make_op(OperationKind::AddInsight {
                id: uuid::Uuid::now_v7(),
                task_id: "TK-001".parse().unwrap(),
                file_path: "src/login.rs".to_string(),
                before_snippet: Some("fn old()".to_string()),
                after_snippet: Some("fn new()".to_string()),
                body: "Simplified".to_string(),
            }),
        ];

        // Also build in-memory state for comparison
        let mut mem_state = ProjectState::default();
        for op in &ops {
            write_operation(&conn, op).unwrap();
            let _ = mem_state.apply(op.clone());
        }

        let db_state = load_state(&conn).unwrap();

        // Compare key fields
        assert_eq!(db_state.config.is_some(), mem_state.config.is_some());
        assert_eq!(db_state.epics.len(), mem_state.epics.len());
        assert_eq!(db_state.tasks.len(), mem_state.tasks.len());
        assert_eq!(db_state.relationships.len(), mem_state.relationships.len());
        assert_eq!(db_state.epic_seq, mem_state.epic_seq);
        assert_eq!(db_state.task_seq, mem_state.task_seq);

        // Task status
        let tk1: ItemId = "TK-001".parse().unwrap();
        assert_eq!(db_state.tasks[&tk1].status, Status::Done);
        assert!(db_state.tasks[&tk1].completed_at.is_some());
        assert!(db_state.tasks[&tk1].claimed_at.is_some());

        // Epic task_ids
        let ep1: ItemId = "EP-001".parse().unwrap();
        assert_eq!(db_state.epics[&ep1].task_ids.len(), 2);

        // Comments & insights
        assert_eq!(db_state.comments_for(&tk1).len(), 1);
        assert_eq!(db_state.insights_for(&tk1).len(), 1);
    }

    #[test]
    fn test_multi_op_atomicity() {
        let (_dir, conn) = setup_db();

        let ops = vec![
            make_op(OperationKind::CreateTask {
                id: "TASK-001".parse().unwrap(),
                title: "Task 1".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
            make_op(OperationKind::CreateTask {
                id: "TASK-002".parse().unwrap(),
                title: "Task 2".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        ];

        write_operations(&conn, &ops).unwrap();

        let state = load_state(&conn).unwrap();
        assert_eq!(state.tasks.len(), 2);

        let op_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(op_count, 2);
    }

    #[test]
    fn test_read_operations_since() {
        let (_dir, conn) = setup_db();

        let op1 = make_op(OperationKind::InitProject {
            name: "P".to_string(),
            description: None,
            epic_prefix: None,
            task_prefix: None,
            auto_materialize: None,
            materialize_path: None,
        });
        write_operation(&conn, &op1).unwrap();

        let rowid1 = latest_rowid(&conn).unwrap();
        assert!(rowid1 > 0);

        let op2 = make_op(OperationKind::CreateTask {
            id: "TASK-001".parse().unwrap(),
            title: "T1".to_string(),
            description: None,
            priority: Priority::Medium,
            epic_id: None,
        });
        write_operation(&conn, &op2).unwrap();

        let (new_ops, rowid2) = read_operations_since(&conn, rowid1).unwrap();
        assert_eq!(new_ops.len(), 1);
        assert!(rowid2 > rowid1);

        // No new ops
        let (no_ops, _) = read_operations_since(&conn, rowid2).unwrap();
        assert!(no_ops.is_empty());
    }

    #[test]
    fn test_legacy_log_import() {
        use std::io::Write;

        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();

        // Write a legacy JSONL log
        let log_path = swarmit_dir.join("operations.log");
        let lock_path = swarmit_dir.join("operations.lock");
        {
            let mut f = std::fs::File::create(&log_path).unwrap();
            let op1 = make_op(OperationKind::InitProject {
                name: "Legacy".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
                auto_materialize: None,
                materialize_path: None,
            });
            let op2 = make_op(OperationKind::CreateTask {
                id: "TASK-001".parse().unwrap(),
                title: "Imported".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            });
            writeln!(f, "{}", serde_json::to_string(&op1).unwrap()).unwrap();
            writeln!(f, "{}", serde_json::to_string(&op2).unwrap()).unwrap();
        }
        // Also create a lock file
        std::fs::write(&lock_path, b"").unwrap();

        // Open DB triggers import
        let conn = open_db(dir.path()).unwrap();

        // Log file should be renamed
        assert!(!log_path.exists(), "operations.log should be renamed");
        assert!(!lock_path.exists(), "operations.lock should be removed");

        // DB should have the imported operations
        let state = load_state(&conn).unwrap();
        assert!(state.config.is_some());
        assert_eq!(state.config.as_ref().unwrap().name, "Legacy");
        assert_eq!(state.tasks.len(), 1);
        assert!(state
            .tasks
            .contains_key(&"TASK-001".parse::<ItemId>().unwrap()));

        // Backup file should exist
        let entries: Vec<_> = std::fs::read_dir(&swarmit_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("operations.log.bak.")
            })
            .collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_v1_snapshot_migration() {
        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();

        // Create a v1-style DB with meta table
        let db_path = swarmit_dir.join("state.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO meta (key, value) VALUES ('schema_version', '1');",
            )
            .unwrap();
        }

        // Open should detect old schema and back it up
        let conn = open_db(dir.path()).unwrap();

        // Should have fresh migrations table
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);

        // Backup should exist
        let entries: Vec<_> = std::fs::read_dir(&swarmit_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("state.db.v1.bak.")
            })
            .collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_epic_completion_db() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let t1: ItemId = "TASK-001".parse().unwrap();
        let t2: ItemId = "TASK-002".parse().unwrap();

        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();

        for (id, title) in [(&t1, "T1"), (&t2, "T2")] {
            write_operation(
                &conn,
                &make_op(OperationKind::CreateTask {
                    id: id.clone(),
                    title: title.to_string(),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: Some(epic_id.clone()),
                }),
            )
            .unwrap();
        }

        // Complete first task — epic should NOT be done
        write_operation(
            &conn,
            &make_op(OperationKind::CompleteTask { id: t1.clone() }),
        )
        .unwrap();
        let state = load_state(&conn).unwrap();
        assert_ne!(state.epics[&epic_id].status, Status::Done);

        // Complete second task — epic SHOULD be done
        write_operation(
            &conn,
            &make_op(OperationKind::CompleteTask { id: t2.clone() }),
        )
        .unwrap();
        let state = load_state(&conn).unwrap();
        assert_eq!(state.epics[&epic_id].status, Status::Done);
    }

    #[test]
    fn test_concurrent_writes() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();

        // Create the DB first
        let conn = open_db(dir.path()).unwrap();
        drop(conn);

        let db_path = Arc::new(swarmit_dir.join("state.db"));
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let db_path = Arc::clone(&db_path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = Connection::open(&*db_path).unwrap();
                    conn.execute_batch("PRAGMA busy_timeout=5000;").unwrap();

                    barrier.wait();

                    for j in 0..5 {
                        let agent = AgentId::new(&format!("agent-{i}")).unwrap();
                        let op = Operation::new(
                            agent,
                            OperationKind::AddComment {
                                id: uuid::Uuid::now_v7(),
                                task_id: "TASK-001".parse().unwrap(),
                                body: format!("Thread {i} op {j}"),
                            },
                        );
                        write_operation(&conn, &op).unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Verify all operations were written
        let conn = open_db(dir.path()).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 20); // 4 threads * 5 ops

        let state = load_state(&conn).unwrap();
        let tk: ItemId = "TASK-001".parse().unwrap();
        assert_eq!(state.comments_for(&tk).len(), 20);
    }

    #[test]
    fn test_compact_db() {
        let (_dir, conn) = setup_db();

        // Write some ops
        for i in 1..=3 {
            write_operation(
                &conn,
                &make_op(OperationKind::CreateTask {
                    id: ItemId::new("TASK", i),
                    title: format!("Task {i}"),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                }),
            )
            .unwrap();
        }

        // Compact
        compact_db(&conn).unwrap();

        // Operations should be gone
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // But state tables should still have data
        let state = load_state(&conn).unwrap();
        assert_eq!(state.tasks.len(), 3);
    }

    #[test]
    fn test_count_operations() {
        let (_dir, conn) = setup_db();

        assert_eq!(count_operations(&conn).unwrap(), 0);

        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: "TASK-001".parse().unwrap(),
                title: "T1".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();

        assert_eq!(count_operations(&conn).unwrap(), 1);
    }

    // ── Comprehensive OperationKind round-trip tests ────────────────────

    /// Helper: write ops to DB + apply to in-memory state, then compare both.
    fn write_and_compare(conn: &Connection, ops: &[Operation]) -> (ProjectState, ProjectState) {
        let mut mem = ProjectState::default();
        for op in ops {
            write_operation(conn, op).unwrap();
            mem.apply(op.clone()).unwrap();
        }
        let db_state = load_state(conn).unwrap();
        (db_state, mem)
    }

    #[test]
    fn test_update_project_round_trip() {
        let (_dir, conn) = setup_db();

        let ops = vec![
            make_op(OperationKind::InitProject {
                name: "Original".to_string(),
                description: Some("Desc".to_string()),
                epic_prefix: None,
                task_prefix: None,
                auto_materialize: None,
                materialize_path: None,
            }),
            make_op(OperationKind::UpdateProject {
                name: Some("Renamed".to_string()),
                description: Some("New desc".to_string()),
                clear_description: false,
                auto_materialize: Some(true),
                materialize_path: Some("docs".to_string()),
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);
        let db_cfg = db.config.unwrap();
        let mem_cfg = mem.config.unwrap();
        assert_eq!(db_cfg.name, "Renamed");
        assert_eq!(db_cfg.name, mem_cfg.name);
        assert_eq!(db_cfg.description, Some("New desc".to_string()));
        assert_eq!(db_cfg.description, mem_cfg.description);
        assert!(db_cfg.auto_materialize);
        assert_eq!(db_cfg.materialize_path, "docs");
    }

    #[test]
    fn test_update_epic_round_trip() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let new_assignee = AgentId::new("alice").unwrap();
        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Original".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::UpdateEpic {
                id: epic_id.clone(),
                title: Some("Updated Title".to_string()),
                description: Some("A description".to_string()),
                priority: Some(Priority::High),
                assignee: Some(new_assignee.clone()),
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);
        assert_eq!(db.epics[&epic_id].title, "Updated Title");
        assert_eq!(db.epics[&epic_id].title, mem.epics[&epic_id].title);
        assert_eq!(
            db.epics[&epic_id].description,
            Some("A description".to_string())
        );
        assert_eq!(db.epics[&epic_id].priority, Priority::High);
        assert_eq!(db.epics[&epic_id].assignee, Some(new_assignee));
    }

    #[test]
    fn test_update_epic_status_round_trip() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::UpdateEpicStatus {
                id: epic_id.clone(),
                status: Status::Blocked,
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);
        assert_eq!(db.epics[&epic_id].status, Status::Blocked);
        assert_eq!(db.epics[&epic_id].status, mem.epics[&epic_id].status);
    }

    #[test]
    fn test_delete_epic_round_trip() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();
        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task in epic".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }),
            make_op(OperationKind::DeleteEpic {
                id: epic_id.clone(),
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);
        assert!(!db.epics.contains_key(&epic_id));
        assert!(!mem.epics.contains_key(&epic_id));

        // epic_task_ids should be cleaned up in DB
        let etid_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM epic_task_ids WHERE epic_id = ?1",
                params![epic_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(etid_count, 0);
    }

    #[test]
    fn test_update_task_move_epic_round_trip() {
        let (_dir, conn) = setup_db();

        let epic_a: ItemId = "EPIC-001".parse().unwrap();
        let epic_b: ItemId = "EPIC-002".parse().unwrap();
        let t1: ItemId = "TASK-001".parse().unwrap();
        let t2: ItemId = "TASK-002".parse().unwrap();

        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_a.clone(),
                title: "Epic A".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::CreateEpic {
                id: epic_b.clone(),
                title: "Epic B".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            // Create two tasks in epic A
            make_op(OperationKind::CreateTask {
                id: t1.clone(),
                title: "Task 1".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_a.clone()),
            }),
            make_op(OperationKind::CreateTask {
                id: t2.clone(),
                title: "Task 2".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_a.clone()),
            }),
            // Complete task 2 first
            make_op(OperationKind::CompleteTask { id: t2.clone() }),
            // Move task 1 from epic A to epic B — epic A should auto-close
            // (only t2 remains and it's Done)
            make_op(OperationKind::UpdateTask {
                id: t1.clone(),
                title: None,
                description: None,
                priority: None,
                epic_id: Some(Some(epic_b.clone())),
                assignee: None,
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);

        // Task should be in epic B now
        assert_eq!(db.tasks[&t1].epic_id, Some(epic_b.clone()));
        assert_eq!(mem.tasks[&t1].epic_id, Some(epic_b.clone()));

        // epic B should contain t1
        assert!(db.epics[&epic_b].task_ids.contains(&t1));
        assert!(mem.epics[&epic_b].task_ids.contains(&t1));

        // epic A should NOT contain t1, only t2
        assert!(!db.epics[&epic_a].task_ids.contains(&t1));
        assert!(db.epics[&epic_a].task_ids.contains(&t2));

        // epic A should be auto-closed (all remaining tasks Done)
        assert_eq!(db.epics[&epic_a].status, Status::Done);
        assert_eq!(mem.epics[&epic_a].status, Status::Done);
    }

    #[test]
    fn test_update_task_status_round_trip() {
        let (_dir, conn) = setup_db();

        let t: ItemId = "TASK-001".parse().unwrap();
        let ops = vec![
            make_op(OperationKind::CreateTask {
                id: t.clone(),
                title: "Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
            make_op(OperationKind::UpdateTaskStatus {
                id: t.clone(),
                status: Status::Blocked,
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);
        assert_eq!(db.tasks[&t].status, Status::Blocked);
        assert_eq!(mem.tasks[&t].status, Status::Blocked);
        // UpdateTaskStatus should NOT set claimed_at or completed_at
        assert!(db.tasks[&t].claimed_at.is_none());
        assert!(db.tasks[&t].completed_at.is_none());
    }

    #[test]
    fn test_delete_task_round_trip() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let t1: ItemId = "TASK-001".parse().unwrap();
        let t2: ItemId = "TASK-002".parse().unwrap();

        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::CreateTask {
                id: t1.clone(),
                title: "T1".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }),
            make_op(OperationKind::CreateTask {
                id: t2.clone(),
                title: "T2".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }),
            // Add a relationship involving t1
            make_op(OperationKind::AddRelationship {
                from: t1.clone(),
                to: t2.clone(),
                rel_type: RelationType::Blocks,
            }),
            // Complete t2
            make_op(OperationKind::CompleteTask { id: t2.clone() }),
            // Delete t1 — should cascade: remove from epic_task_ids, relationships,
            // and trigger epic auto-close since t2 is Done
            make_op(OperationKind::DeleteTask { id: t1.clone() }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);

        // Task should be gone
        assert!(!db.tasks.contains_key(&t1));
        assert!(!mem.tasks.contains_key(&t1));

        // Relationship should be gone
        assert!(db.relationships.is_empty());
        assert!(mem.relationships.is_empty());

        // Epic should not list t1
        assert!(!db.epics[&epic_id].task_ids.contains(&t1));
        assert!(!mem.epics[&epic_id].task_ids.contains(&t1));

        // Epic should auto-close (only t2 remains and is Done)
        assert_eq!(db.epics[&epic_id].status, Status::Done);
        assert_eq!(mem.epics[&epic_id].status, Status::Done);
    }

    #[test]
    fn test_remove_relationship_round_trip() {
        let (_dir, conn) = setup_db();

        let t1: ItemId = "TASK-001".parse().unwrap();
        let t2: ItemId = "TASK-002".parse().unwrap();
        let ops = vec![
            make_op(OperationKind::AddRelationship {
                from: t1.clone(),
                to: t2.clone(),
                rel_type: RelationType::Blocks,
            }),
            make_op(OperationKind::RemoveRelationship {
                from: t1.clone(),
                to: t2.clone(),
                rel_type: RelationType::Blocks,
            }),
        ];

        let (db, mem) = write_and_compare(&conn, &ops);
        assert!(db.relationships.is_empty());
        assert!(mem.relationships.is_empty());
    }

    #[test]
    fn test_post_compact_continuity() {
        let (_dir, conn) = setup_db();

        // Phase 1: write initial ops
        write_operation(
            &conn,
            &make_op(OperationKind::InitProject {
                name: "P".to_string(),
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
            &make_op(OperationKind::CreateEpic {
                id: "EPIC-001".parse().unwrap(),
                title: "E1".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();

        for i in 1..=3 {
            write_operation(
                &conn,
                &make_op(OperationKind::CreateTask {
                    id: ItemId::new("TASK", i),
                    title: format!("Task {i}"),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: Some("EPIC-001".parse().unwrap()),
                }),
            )
            .unwrap();
        }

        // Compact
        compact_db(&conn).unwrap();
        assert_eq!(count_operations(&conn).unwrap(), 0);

        // Phase 2: write more ops after compact — sequences must continue correctly
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: ItemId::new("TASK", 4),
                title: "Task 4".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some("EPIC-001".parse().unwrap()),
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();
        // All 4 tasks should be present (3 from before compact + 1 after)
        assert_eq!(state.tasks.len(), 4);
        // Sequence counters should be correct
        assert_eq!(state.task_seq, 4);
        assert_eq!(state.epic_seq, 1);
        // Epic task_ids should have all 4
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        assert_eq!(state.epics[&epic_id].task_ids.len(), 4);
    }

    #[test]
    fn test_concurrent_writes_with_open_db() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = tempdir().unwrap();
        let swarmit_dir = dir.path().join(".swarmit");
        std::fs::create_dir_all(&swarmit_dir).unwrap();

        // Create DB via open_db to ensure WAL + busy_timeout
        let conn = open_db(dir.path()).unwrap();
        drop(conn);

        let root_path = Arc::new(dir.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let root = Arc::clone(&root_path);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    // Each thread uses open_db for proper WAL + busy_timeout
                    let conn = open_db(&root).unwrap();
                    barrier.wait();

                    for j in 0..5 {
                        let agent = AgentId::new(&format!("agent-{i}")).unwrap();
                        let op = Operation::new(
                            agent,
                            OperationKind::AddComment {
                                id: uuid::Uuid::now_v7(),
                                task_id: "TASK-001".parse().unwrap(),
                                body: format!("Thread {i} op {j}"),
                            },
                        );
                        write_operation(&conn, &op).unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let conn = open_db(dir.path()).unwrap();
        assert_eq!(count_operations(&conn).unwrap(), 20);
        let state = load_state(&conn).unwrap();
        let tk: ItemId = "TASK-001".parse().unwrap();
        assert_eq!(state.comments_for(&tk).len(), 20);
    }

    // ── TASK-090: Concurrent parallel task creation — no data loss ───────

    #[test]
    fn test_concurrent_task_creation_no_data_loss() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".swarmit")).unwrap();
        let conn = open_db(dir.path()).unwrap();
        drop(conn);

        let root = Arc::new(dir.path().to_path_buf());
        let n_threads = 4;
        let tasks_per_thread = 10;
        let barrier = Arc::new(Barrier::new(n_threads));

        let handles: Vec<_> = (0..n_threads)
            .map(|i| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = open_db(&root).unwrap();
                    barrier.wait();

                    for j in 0..tasks_per_thread {
                        let global_idx = (i * tasks_per_thread + j + 1) as u32;
                        let agent = AgentId::new(&format!("agent-{i}")).unwrap();
                        let op = Operation::new(
                            agent,
                            OperationKind::CreateTask {
                                id: ItemId::new("TASK", global_idx),
                                title: format!("Task from thread {i} #{j}"),
                                description: Some(format!("Created by thread {i}")),
                                priority: Priority::Medium,
                                epic_id: None,
                            },
                        );
                        write_operation(&conn, &op).unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let conn = open_db(dir.path()).unwrap();
        let total_expected = (n_threads * tasks_per_thread) as i64;
        assert_eq!(count_operations(&conn).unwrap(), total_expected);

        let state = load_state(&conn).unwrap();
        assert_eq!(
            state.tasks.len(),
            total_expected as usize,
            "Expected {} tasks, got {}. Some were lost!",
            total_expected,
            state.tasks.len()
        );

        // Verify each task has the right description (not clobbered)
        for i in 0..n_threads {
            for j in 0..tasks_per_thread {
                let global_idx = (i * tasks_per_thread + j + 1) as u32;
                let id = ItemId::new("TASK", global_idx);
                assert!(
                    state.tasks.contains_key(&id),
                    "Task {} missing from state",
                    id
                );
                assert_eq!(
                    state.tasks[&id].description,
                    Some(format!("Created by thread {i}"))
                );
            }
        }
    }

    // ── TASK-091: Racing epic completion ─────────────────────────────────

    #[test]
    fn test_concurrent_epic_completion_race() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".swarmit")).unwrap();
        let conn = open_db(dir.path()).unwrap();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let n_tasks = 8;

        // Setup: create epic + tasks sequentially
        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Race Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();

        for i in 1..=n_tasks {
            write_operation(
                &conn,
                &make_op(OperationKind::CreateTask {
                    id: ItemId::new("TASK", i),
                    title: format!("Task {i}"),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: Some(epic_id.clone()),
                }),
            )
            .unwrap();
        }
        drop(conn);

        // Each thread completes one task concurrently
        let root = Arc::new(dir.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(n_tasks as usize));

        let handles: Vec<_> = (1..=n_tasks)
            .map(|i| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = open_db(&root).unwrap();
                    barrier.wait();
                    let agent = AgentId::new(&format!("completer-{i}")).unwrap();
                    let op = Operation::new(
                        agent,
                        OperationKind::CompleteTask {
                            id: ItemId::new("TASK", i),
                        },
                    );
                    write_operation(&conn, &op).unwrap();
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let conn = open_db(dir.path()).unwrap();
        let state = load_state(&conn).unwrap();

        // All tasks must be Done
        for i in 1..=n_tasks {
            let tid = ItemId::new("TASK", i);
            assert_eq!(
                state.tasks[&tid].status,
                Status::Done,
                "Task {} should be Done",
                tid
            );
            assert!(
                state.tasks[&tid].completed_at.is_some(),
                "Task {} should have completed_at",
                tid
            );
        }

        // Epic must be Done (auto-closed when last task completed)
        assert_eq!(
            state.epics[&epic_id].status,
            Status::Done,
            "Epic should be auto-closed after all tasks completed concurrently"
        );
    }

    // ── TASK-092: Interleaved write + read — no stale/partial reads ─────

    #[test]
    fn test_concurrent_write_read_consistency() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".swarmit")).unwrap();
        let conn = open_db(dir.path()).unwrap();

        // Create an epic so we can verify task<->epic consistency
        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: "EPIC-001".parse().unwrap(),
                title: "Consistency Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();
        drop(conn);

        let root = Arc::new(dir.path().to_path_buf());
        let done = Arc::new(AtomicBool::new(false));

        // Writer thread: creates 50 tasks in the epic
        let writer = {
            let root = Arc::clone(&root);
            let done = Arc::clone(&done);
            thread::spawn(move || {
                let conn = open_db(&root).unwrap();
                for i in 1..=50u32 {
                    let op = Operation::new(
                        AgentId::new("writer").unwrap(),
                        OperationKind::CreateTask {
                            id: ItemId::new("TASK", i),
                            title: format!("Task {i}"),
                            description: None,
                            priority: Priority::Medium,
                            epic_id: Some("EPIC-001".parse().unwrap()),
                        },
                    );
                    write_operation(&conn, &op).unwrap();
                }
                done.store(true, Ordering::Release);
            })
        };

        // Reader threads: continuously load_state and check consistency
        let readers: Vec<_> = (0..3)
            .map(|_| {
                let root = Arc::clone(&root);
                let done = Arc::clone(&done);
                thread::spawn(move || {
                    let conn = open_db(&root).unwrap();
                    let mut reads = 0;
                    while !done.load(Ordering::Acquire) || reads < 5 {
                        let state = load_state(&conn).unwrap();

                        // Consistency check: every task in epic_task_ids must exist in tasks
                        let epic_id: ItemId = "EPIC-001".parse().unwrap();
                        if let Some(epic) = state.epics.get(&epic_id) {
                            for tid in &epic.task_ids {
                                assert!(
                                    state.tasks.contains_key(tid),
                                    "epic_task_ids references {} but task doesn't exist! \
                                     Partial read detected.",
                                    tid
                                );
                            }
                            // And the reverse: every task with this epic_id should be listed
                            for (tid, task) in &state.tasks {
                                if task.epic_id.as_ref() == Some(&epic_id) {
                                    assert!(
                                        epic.task_ids.contains(tid),
                                        "Task {} has epic_id EPIC-001 but is not in epic.task_ids! \
                                         Partial read detected.",
                                        tid
                                    );
                                }
                            }
                        }
                        reads += 1;
                        // Small yield to let writer progress
                        thread::yield_now();
                    }
                    reads
                })
            })
            .collect();

        writer.join().unwrap();
        let total_reads: usize = readers.into_iter().map(|h| h.join().unwrap()).sum();
        assert!(
            total_reads > 0,
            "Reader threads should have performed at least some reads"
        );

        // Final check: all 50 tasks present
        let conn = open_db(dir.path()).unwrap();
        let state = load_state(&conn).unwrap();
        assert_eq!(state.tasks.len(), 50);
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        assert_eq!(state.epics[&epic_id].task_ids.len(), 50);
    }

    // ── TASK-093: INSERT OR IGNORE preserves existing task on dup ────────

    #[test]
    fn test_insert_or_ignore_preserves_claimed_task() {
        let (_dir, conn) = setup_db();

        let task_id: ItemId = "TASK-001".parse().unwrap();

        // Create and claim the task (sets assignee + claimed_at + InProgress)
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Original Title".to_string(),
                description: Some("Original desc".to_string()),
                priority: Priority::High,
                epic_id: None,
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::ClaimTask {
                id: task_id.clone(),
            }),
        )
        .unwrap();

        let before = load_state(&conn).unwrap();
        let task_before = &before.tasks[&task_id];
        assert_eq!(task_before.status, Status::InProgress);
        assert!(task_before.assignee.is_some());
        assert!(task_before.claimed_at.is_some());

        // Now replay a duplicate CreateTask with the same ID — this simulates
        // a TOCTOU race where two agents created the same task ID.
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "CLOBBERED TITLE".to_string(),
                description: Some("CLOBBERED desc".to_string()),
                priority: Priority::Low,
                epic_id: None,
            }),
        )
        .unwrap();

        let after = load_state(&conn).unwrap();
        let task_after = &after.tasks[&task_id];

        // INSERT OR IGNORE should preserve the existing row
        assert_eq!(
            task_after.status,
            Status::InProgress,
            "Status should be preserved (InProgress), not reset to Todo"
        );
        assert!(
            task_after.assignee.is_some(),
            "Assignee should be preserved, not reset to NULL"
        );
        assert!(
            task_after.claimed_at.is_some(),
            "claimed_at should be preserved, not reset to NULL"
        );
        assert_eq!(
            task_after.title, "Original Title",
            "Title should be preserved by INSERT OR IGNORE"
        );
        assert_eq!(
            task_after.priority,
            Priority::High,
            "Priority should be preserved by INSERT OR IGNORE"
        );
    }

    // ── TASK-094: Partial UpdateTask does not clobber unrelated fields ───

    #[test]
    fn test_partial_update_task_preserves_unrelated_fields() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Original Title".to_string(),
                description: Some("Original desc".to_string()),
                priority: Priority::High,
                epic_id: Some(epic_id.clone()),
            }),
            make_op(OperationKind::ClaimTask {
                id: task_id.clone(),
            }),
        ];
        for op in &ops {
            write_operation(&conn, op).unwrap();
        }

        let before = load_state(&conn).unwrap();
        let tb = &before.tasks[&task_id];
        let original_desc = tb.description.clone();
        let original_priority = tb.priority;
        let original_epic = tb.epic_id.clone();
        let original_assignee = tb.assignee.clone();
        let original_claimed = tb.claimed_at;

        // Update ONLY the title
        write_operation(
            &conn,
            &make_op(OperationKind::UpdateTask {
                id: task_id.clone(),
                title: Some("New Title".to_string()),
                description: None,
                priority: None,
                epic_id: None,
                assignee: None,
            }),
        )
        .unwrap();

        let after = load_state(&conn).unwrap();
        let ta = &after.tasks[&task_id];
        assert_eq!(ta.title, "New Title");
        assert_eq!(ta.description, original_desc, "description clobbered!");
        assert_eq!(ta.priority, original_priority, "priority clobbered!");
        assert_eq!(ta.epic_id, original_epic, "epic_id clobbered!");
        assert_eq!(ta.assignee, original_assignee, "assignee clobbered!");
        assert_eq!(ta.claimed_at, original_claimed, "claimed_at clobbered!");
        assert!(
            ta.updated_at > tb.updated_at,
            "updated_at should advance on edit"
        );

        // Also verify materializer matches
        let mut mem = ProjectState::default();
        for op in &ops {
            mem.apply(op.clone()).unwrap();
        }
        mem.apply(make_op(OperationKind::UpdateTask {
            id: task_id.clone(),
            title: Some("New Title".to_string()),
            description: None,
            priority: None,
            epic_id: None,
            assignee: None,
        }))
        .unwrap();
        assert_eq!(mem.tasks[&task_id].title, "New Title");
        assert_eq!(mem.tasks[&task_id].description, original_desc);
        assert_eq!(mem.tasks[&task_id].priority, original_priority);
    }

    // ── TASK-095: Partial UpdateEpic does not clobber unrelated fields ───

    #[test]
    fn test_partial_update_epic_preserves_unrelated_fields() {
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Original Epic".to_string(),
                description: Some("Epic desc".to_string()),
                priority: Priority::High,
            }),
            make_op(OperationKind::UpdateEpic {
                id: epic_id.clone(),
                title: None,
                description: None,
                priority: None,
                assignee: Some(AgentId::new("original-owner").unwrap()),
            }),
        ];
        let (db, mem) = write_and_compare(&conn, &ops);

        let eb = &db.epics[&epic_id];
        let original_title = eb.title.clone();
        let original_desc = eb.description.clone();
        let original_priority = eb.priority;

        // Now update ONLY the assignee to a different value
        let update_op = make_op(OperationKind::UpdateEpic {
            id: epic_id.clone(),
            title: None,
            description: None,
            priority: None,
            assignee: Some(AgentId::new("new-owner").unwrap()),
        });
        write_operation(&conn, &update_op).unwrap();

        let after = load_state(&conn).unwrap();
        let ea = &after.epics[&epic_id];
        assert_eq!(ea.title, original_title, "title clobbered!");
        assert_eq!(ea.description, original_desc, "description clobbered!");
        assert_eq!(ea.priority, original_priority, "priority clobbered!");
        assert_eq!(
            ea.assignee,
            Some(AgentId::new("new-owner").unwrap()),
            "assignee should be updated"
        );
        assert_eq!(
            ea.status, eb.status,
            "status should not change from UpdateEpic"
        );

        // Materializer match
        let mut mem2 = mem;
        mem2.apply(update_op).unwrap();
        assert_eq!(mem2.epics[&epic_id].title, original_title);
        assert_eq!(
            mem2.epics[&epic_id].assignee,
            Some(AgentId::new("new-owner").unwrap())
        );
    }

    // ── TASK-096: Full lifecycle timestamps ──────────────────────────────

    #[test]
    fn test_full_lifecycle_timestamps() {
        use chrono::Duration;

        let (_dir, conn) = setup_db();

        let task_id: ItemId = "TASK-001".parse().unwrap();
        let base = Utc::now();

        // Step 1: Create (t=0)
        let op_create = Operation {
            id: uuid::Uuid::now_v7(),
            agent: agent(),
            timestamp: base,
            kind: OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Lifecycle Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            },
        };
        write_operation(&conn, &op_create).unwrap();

        let s1 = load_state(&conn).unwrap();
        let t1 = &s1.tasks[&task_id];
        assert_eq!(t1.status, Status::Todo);
        assert!(t1.claimed_at.is_none());
        assert!(t1.completed_at.is_none());
        assert_eq!(t1.created_at, base);
        assert_eq!(t1.updated_at, base);

        // Step 2: Claim (t=1s)
        let ts_claim = base + Duration::seconds(1);
        let op_claim = Operation {
            id: uuid::Uuid::now_v7(),
            agent: agent(),
            timestamp: ts_claim,
            kind: OperationKind::ClaimTask {
                id: task_id.clone(),
            },
        };
        write_operation(&conn, &op_claim).unwrap();

        let s2 = load_state(&conn).unwrap();
        let t2 = &s2.tasks[&task_id];
        assert_eq!(t2.status, Status::InProgress);
        assert_eq!(t2.claimed_at, Some(ts_claim));
        assert!(t2.completed_at.is_none());
        assert_eq!(t2.updated_at, ts_claim);

        // Step 3: UpdateTaskStatus to Blocked (t=2s) — should NOT reset claimed_at
        let ts_blocked = base + Duration::seconds(2);
        let op_blocked = Operation {
            id: uuid::Uuid::now_v7(),
            agent: agent(),
            timestamp: ts_blocked,
            kind: OperationKind::UpdateTaskStatus {
                id: task_id.clone(),
                status: Status::Blocked,
            },
        };
        write_operation(&conn, &op_blocked).unwrap();

        let s3 = load_state(&conn).unwrap();
        let t3 = &s3.tasks[&task_id];
        assert_eq!(t3.status, Status::Blocked);
        assert_eq!(
            t3.claimed_at,
            Some(ts_claim),
            "claimed_at must survive status changes"
        );
        assert!(t3.completed_at.is_none());
        assert_eq!(t3.updated_at, ts_blocked);

        // Step 4: UpdateTaskStatus back to InProgress (t=3s) — still no claimed_at reset
        let ts_unblock = base + Duration::seconds(3);
        let op_unblock = Operation {
            id: uuid::Uuid::now_v7(),
            agent: agent(),
            timestamp: ts_unblock,
            kind: OperationKind::UpdateTaskStatus {
                id: task_id.clone(),
                status: Status::InProgress,
            },
        };
        write_operation(&conn, &op_unblock).unwrap();

        let s4 = load_state(&conn).unwrap();
        let t4 = &s4.tasks[&task_id];
        assert_eq!(t4.status, Status::InProgress);
        assert_eq!(
            t4.claimed_at,
            Some(ts_claim),
            "claimed_at must still be the original claim time"
        );
        assert_eq!(t4.updated_at, ts_unblock);

        // Step 5: Complete (t=4s)
        let ts_done = base + Duration::seconds(4);
        let op_done = Operation {
            id: uuid::Uuid::now_v7(),
            agent: agent(),
            timestamp: ts_done,
            kind: OperationKind::CompleteTask {
                id: task_id.clone(),
            },
        };
        write_operation(&conn, &op_done).unwrap();

        let s5 = load_state(&conn).unwrap();
        let t5 = &s5.tasks[&task_id];
        assert_eq!(t5.status, Status::Done);
        assert_eq!(
            t5.claimed_at,
            Some(ts_claim),
            "claimed_at must survive completion"
        );
        assert_eq!(t5.completed_at, Some(ts_done));
        assert_eq!(t5.updated_at, ts_done);

        // Verify materializer produces identical timestamps
        let mut mem = ProjectState::default();
        for op in [&op_create, &op_claim, &op_blocked, &op_unblock, &op_done] {
            mem.apply(op.clone()).unwrap();
        }
        let mt = &mem.tasks[&task_id];
        assert_eq!(mt.claimed_at, t5.claimed_at);
        assert_eq!(mt.completed_at, t5.completed_at);
        assert_eq!(mt.updated_at, t5.updated_at);
        assert_eq!(mt.created_at, t5.created_at);
    }

    // ── TASK-097: All 16 OperationKind variants — DB/materializer equiv ─

    #[test]
    fn test_all_16_variants_db_materializer_equivalence() {
        let (_dir, conn) = setup_db();
        let mut mem = ProjectState::default();

        // Helper to apply to both and compare
        let mut apply_both = |op: Operation| {
            write_operation(&conn, &op).unwrap();
            mem.apply(op).unwrap();
            let db = load_state(&conn).unwrap();

            // Compare structural fields
            assert_eq!(db.config.is_some(), mem.config.is_some(), "config mismatch");
            if let (Some(dc), Some(mc)) = (&db.config, &mem.config) {
                assert_eq!(dc.name, mc.name, "config.name mismatch");
                assert_eq!(
                    dc.description, mc.description,
                    "config.description mismatch"
                );
                assert_eq!(
                    dc.auto_materialize, mc.auto_materialize,
                    "config.auto_materialize mismatch"
                );
            }
            assert_eq!(db.epics.len(), mem.epics.len(), "epics count mismatch");
            for (id, de) in &db.epics {
                let me = &mem.epics[id];
                assert_eq!(de.title, me.title, "epic {} title mismatch", id);
                assert_eq!(de.status, me.status, "epic {} status mismatch", id);
                assert_eq!(
                    de.task_ids.len(),
                    me.task_ids.len(),
                    "epic {} task_ids count mismatch",
                    id
                );
                assert_eq!(de.assignee, me.assignee, "epic {} assignee mismatch", id);
            }
            assert_eq!(db.tasks.len(), mem.tasks.len(), "tasks count mismatch");
            for (id, dt) in &db.tasks {
                let mt = &mem.tasks[id];
                assert_eq!(dt.title, mt.title, "task {} title mismatch", id);
                assert_eq!(dt.status, mt.status, "task {} status mismatch", id);
                assert_eq!(dt.epic_id, mt.epic_id, "task {} epic_id mismatch", id);
                assert_eq!(dt.assignee, mt.assignee, "task {} assignee mismatch", id);
                assert_eq!(dt.priority, mt.priority, "task {} priority mismatch", id);
            }
            assert_eq!(
                db.relationships.len(),
                mem.relationships.len(),
                "relationships count mismatch"
            );
            assert_eq!(db.epic_seq, mem.epic_seq, "epic_seq mismatch");
            assert_eq!(db.task_seq, mem.task_seq, "task_seq mismatch");
        };

        // 1. InitProject
        apply_both(make_op(OperationKind::InitProject {
            name: "Test".to_string(),
            description: Some("Desc".to_string()),
            epic_prefix: Some("EP".to_string()),
            task_prefix: Some("TK".to_string()),
            auto_materialize: Some(true),
            materialize_path: Some("docs".to_string()),
        }));

        // 2. UpdateProject
        apply_both(make_op(OperationKind::UpdateProject {
            name: Some("Renamed".to_string()),
            description: None,
            clear_description: true,
            auto_materialize: Some(false),
            materialize_path: None,
        }));

        // 3. CreateEpic
        apply_both(make_op(OperationKind::CreateEpic {
            id: "EP-001".parse().unwrap(),
            title: "Epic One".to_string(),
            description: Some("Epic desc".to_string()),
            priority: Priority::High,
        }));

        // Create second epic for move test later
        apply_both(make_op(OperationKind::CreateEpic {
            id: "EP-002".parse().unwrap(),
            title: "Epic Two".to_string(),
            description: None,
            priority: Priority::Low,
        }));

        // 4. UpdateEpic
        apply_both(make_op(OperationKind::UpdateEpic {
            id: "EP-001".parse().unwrap(),
            title: Some("Updated Epic".to_string()),
            description: None,
            priority: Some(Priority::Medium),
            assignee: Some(AgentId::new("alice").unwrap()),
        }));

        // 5. UpdateEpicStatus
        apply_both(make_op(OperationKind::UpdateEpicStatus {
            id: "EP-001".parse().unwrap(),
            status: Status::InProgress,
        }));

        // 6. CreateTask (in EP-001)
        apply_both(make_op(OperationKind::CreateTask {
            id: "TK-001".parse().unwrap(),
            title: "Task One".to_string(),
            description: Some("Task desc".to_string()),
            priority: Priority::High,
            epic_id: Some("EP-001".parse().unwrap()),
        }));

        // Create a second task for relationship/delete tests
        apply_both(make_op(OperationKind::CreateTask {
            id: "TK-002".parse().unwrap(),
            title: "Task Two".to_string(),
            description: None,
            priority: Priority::Medium,
            epic_id: Some("EP-001".parse().unwrap()),
        }));

        // Task without epic for move test
        apply_both(make_op(OperationKind::CreateTask {
            id: "TK-003".parse().unwrap(),
            title: "Task Three".to_string(),
            description: None,
            priority: Priority::Low,
            epic_id: None,
        }));

        // 7. UpdateTask (change title + move TK-003 into EP-002)
        apply_both(make_op(OperationKind::UpdateTask {
            id: "TK-003".parse().unwrap(),
            title: Some("Moved Task".to_string()),
            description: Some("Now has desc".to_string()),
            priority: None,
            epic_id: Some(Some("EP-002".parse().unwrap())),
            assignee: Some(Some(AgentId::new("bob").unwrap())),
        }));

        // 8. UpdateTaskStatus
        apply_both(make_op(OperationKind::UpdateTaskStatus {
            id: "TK-001".parse().unwrap(),
            status: Status::Blocked,
        }));

        // 9. ClaimTask
        apply_both(make_op(OperationKind::ClaimTask {
            id: "TK-002".parse().unwrap(),
        }));

        // 10. CompleteTask
        apply_both(make_op(OperationKind::CompleteTask {
            id: "TK-002".parse().unwrap(),
        }));

        // 11. AddRelationship
        apply_both(make_op(OperationKind::AddRelationship {
            from: "TK-001".parse().unwrap(),
            to: "TK-003".parse().unwrap(),
            rel_type: RelationType::Blocks,
        }));

        // 12. RemoveRelationship
        apply_both(make_op(OperationKind::RemoveRelationship {
            from: "TK-001".parse().unwrap(),
            to: "TK-003".parse().unwrap(),
            rel_type: RelationType::Blocks,
        }));

        // 13. AddComment
        let comment_id = uuid::Uuid::now_v7();
        apply_both(make_op(OperationKind::AddComment {
            id: comment_id,
            task_id: "TK-001".parse().unwrap(),
            body: "A comment".to_string(),
        }));

        // 14. AddInsight
        let insight_id = uuid::Uuid::now_v7();
        apply_both(make_op(OperationKind::AddInsight {
            id: insight_id,
            task_id: "TK-001".parse().unwrap(),
            file_path: "src/main.rs".to_string(),
            before_snippet: Some("old".to_string()),
            after_snippet: Some("new".to_string()),
            body: "An insight".to_string(),
        }));

        // 15. DeleteTask
        apply_both(make_op(OperationKind::DeleteTask {
            id: "TK-001".parse().unwrap(),
        }));

        // Verify comment/insight for deleted task is still in DB
        // (comments are keyed by task_id, not FK-cascaded)
        let db = load_state(&conn).unwrap();
        assert!(!db.tasks.contains_key(&"TK-001".parse::<ItemId>().unwrap()));

        // 16. DeleteEpic
        apply_both(make_op(OperationKind::DeleteEpic {
            id: "EP-001".parse().unwrap(),
        }));

        // Final state verification
        let db = load_state(&conn).unwrap();
        assert_eq!(db.epics.len(), 1); // Only EP-002 remains
        assert!(db.epics.contains_key(&"EP-002".parse::<ItemId>().unwrap()));
        assert_eq!(db.tasks.len(), 2); // TK-002, TK-003 remain
        assert_eq!(db.relationships.len(), 0);
        assert_eq!(db.epic_seq, mem.epic_seq);
        assert_eq!(db.task_seq, mem.task_seq);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Adversarial Stress Tests
    //  Intentionally try to break the state: ghost ops, orphan refs,
    //  concurrency races, sequence counter attacks, double-option edge
    //  cases, and replay safety.
    // ══════════════════════════════════════════════════════════════════

    /// Raw SQL count helper for stress tests.
    fn query_count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    // ── Category 1: Ghost Operations ──────────────────────────────────
    // Operations on non-existent entities. apply_to_db silently does
    // UPDATE ... WHERE id=? (0 rows affected), while the materializer
    // would return Err(NotFound).

    #[test]
    fn test_ghost_update_task_silently_noop() {
        let (_dir, conn) = setup_db();

        let ghost_op = make_op(OperationKind::UpdateTask {
            id: "TASK-999".parse().unwrap(),
            title: Some("Ghost".to_string()),
            description: None,
            priority: None,
            epic_id: None,
            assignee: None,
        });

        // DB accepts the ghost op (UPDATE matches 0 rows — silent success)
        write_operation(&conn, &ghost_op).unwrap();

        let state = load_state(&conn).unwrap();
        assert!(
            state.tasks.is_empty(),
            "ghost update should not create a task"
        );

        // Op IS in the log
        assert_eq!(
            query_count(&conn, "SELECT COUNT(*) FROM operations"),
            1,
            "ghost op should be recorded in operations log"
        );

        // Materializer rejects on replay
        let mut mem = ProjectState::default();
        assert!(
            mem.apply(ghost_op).is_err(),
            "materializer should return NotFound for ghost update"
        );
    }

    #[test]
    fn test_ghost_claim_and_complete_silently_noop() {
        let (_dir, conn) = setup_db();

        let claim_op = make_op(OperationKind::ClaimTask {
            id: "TASK-999".parse().unwrap(),
        });
        let complete_op = make_op(OperationKind::CompleteTask {
            id: "TASK-888".parse().unwrap(),
        });

        // Both succeed at DB level
        write_operation(&conn, &claim_op).unwrap();
        write_operation(&conn, &complete_op).unwrap();

        let state = load_state(&conn).unwrap();
        assert!(state.tasks.is_empty());
        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM operations"), 2);

        // Both fail on materializer replay
        let mut mem = ProjectState::default();
        assert!(mem.apply(claim_op).is_err());
        assert!(mem.apply(complete_op).is_err());
    }

    #[test]
    fn test_ghost_epic_ops_silently_noop() {
        let (_dir, conn) = setup_db();

        let update_op = make_op(OperationKind::UpdateEpic {
            id: "EPIC-999".parse().unwrap(),
            title: Some("Ghost Epic".to_string()),
            description: None,
            priority: None,
            assignee: None,
        });
        let status_op = make_op(OperationKind::UpdateEpicStatus {
            id: "EPIC-999".parse().unwrap(),
            status: Status::Done,
        });

        write_operation(&conn, &update_op).unwrap();
        write_operation(&conn, &status_op).unwrap();

        let state = load_state(&conn).unwrap();
        assert!(
            state.epics.is_empty(),
            "ghost epic ops should not create an epic"
        );
        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM operations"), 2);

        let mut mem = ProjectState::default();
        assert!(mem.apply(update_op).is_err());
        assert!(mem.apply(status_op).is_err());
    }

    #[test]
    fn test_ghost_ops_survive_compact_cleanly() {
        let (_dir, conn) = setup_db();

        // Seed sequences
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

        // Valid op
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: "TASK-001".parse().unwrap(),
                title: "Real Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();

        // Ghost op (claim non-existent task)
        write_operation(
            &conn,
            &make_op(OperationKind::ClaimTask {
                id: "TASK-999".parse().unwrap(),
            }),
        )
        .unwrap();

        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM operations"), 3);
        let state = load_state(&conn).unwrap();
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.task_seq, 1);

        // Compact erases all ops (including ghost)
        compact_db(&conn).unwrap();
        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM operations"), 0);

        // State is preserved — real task survives, ghost is gone from log
        let state = load_state(&conn).unwrap();
        assert_eq!(state.tasks.len(), 1, "real task should survive compact");
        assert!(state
            .tasks
            .contains_key(&"TASK-001".parse::<ItemId>().unwrap()));
    }

    // ── Category 2: Orphan References ─────────────────────────────────
    // No FK constraints in schema. Deletes leave dangling references.

    #[test]
    fn test_delete_epic_orphans_task_epic_id() {
        // DeleteEpic removes from `epics` and `epic_task_ids` but does NOT
        // null out `tasks.epic_id`. The task retains a dangling reference.
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Doomed Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Orphan Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::DeleteEpic {
                id: epic_id.clone(),
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();

        // Epic is gone
        assert!(!state.epics.contains_key(&epic_id));

        // epic_task_ids cleaned by DeleteEpic
        assert_eq!(
            query_count(&conn, "SELECT COUNT(*) FROM epic_task_ids"),
            0,
            "epic_task_ids should be cleaned by DeleteEpic"
        );

        // ORPHAN: task still has dangling epic_id
        assert_eq!(
            state.tasks[&task_id].epic_id,
            Some(epic_id.clone()),
            "task retains dangling epic_id after epic deletion"
        );

        // Materializer agrees on the orphan
        let mut mem = ProjectState::default();
        for op in read_all_operations(&conn).unwrap() {
            let _ = mem.apply(op);
        }
        assert_eq!(
            mem.tasks[&task_id].epic_id,
            Some(epic_id),
            "materializer also leaves the dangling epic_id"
        );
    }

    #[test]
    fn test_delete_task_leaves_orphan_comments_and_insights() {
        // DeleteTask removes the task, relationships, and epic_task_ids entries
        // but does NOT remove comments or insights.
        let (_dir, conn) = setup_db();

        let task_id: ItemId = "TASK-001".parse().unwrap();
        let comment_id = uuid::Uuid::now_v7();
        let insight_id = uuid::Uuid::now_v7();

        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task with artifacts".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::AddComment {
                id: comment_id,
                task_id: task_id.clone(),
                body: "Important note".to_string(),
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::AddInsight {
                id: insight_id,
                task_id: task_id.clone(),
                file_path: "src/main.rs".to_string(),
                before_snippet: Some("old".to_string()),
                after_snippet: Some("new".to_string()),
                body: "Code change".to_string(),
            }),
        )
        .unwrap();

        // Delete the task
        write_operation(
            &conn,
            &make_op(OperationKind::DeleteTask {
                id: task_id.clone(),
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();

        // Task is gone
        assert!(!state.tasks.contains_key(&task_id));

        // ORPHANS: comments and insights survive deletion
        assert_eq!(
            state.comments_for(&task_id).len(),
            1,
            "orphan comment should survive task deletion"
        );
        assert_eq!(
            state.insights_for(&task_id).len(),
            1,
            "orphan insight should survive task deletion"
        );

        // Raw DB agrees
        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM comments"), 1);
        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM insights"), 1);
    }

    #[test]
    fn test_relationship_for_nonexistent_tasks() {
        // AddRelationship does not validate entity existence.
        let (_dir, conn) = setup_db();

        write_operation(
            &conn,
            &make_op(OperationKind::AddRelationship {
                from: "TASK-100".parse().unwrap(),
                to: "TASK-200".parse().unwrap(),
                rel_type: RelationType::Blocks,
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();
        assert_eq!(
            state.relationships.len(),
            1,
            "relationship should exist for non-existent tasks"
        );
        assert!(state.tasks.is_empty());
    }

    #[test]
    fn test_create_task_pointing_to_deleted_epic() {
        // Creating a task with epic_id pointing to a deleted epic:
        // - task.epic_id is set (dangling)
        // - DB inserts orphan row into epic_task_ids (no FK constraint)
        // - load_state ignores orphan epic_task_ids (epic not in map)
        // - Materializer skips adding to epic.task_ids (epic not in BTreeMap)
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Ephemeral Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::DeleteEpic {
                id: epic_id.clone(),
            }),
        )
        .unwrap();

        // Create task pointing to the deleted epic
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Orphan at birth".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();

        // Task has dangling epic_id
        assert_eq!(state.tasks[&task_id].epic_id, Some(epic_id.clone()));

        // Epic doesn't exist
        assert!(!state.epics.contains_key(&epic_id));

        // Raw DB has orphan epic_task_ids row (no FK validation)
        assert_eq!(
            query_count(&conn, "SELECT COUNT(*) FROM epic_task_ids"),
            1,
            "orphan epic_task_ids row exists in raw DB"
        );

        // Materializer agrees: task has epic_id but epic doesn't exist
        let mut mem = ProjectState::default();
        for op in read_all_operations(&conn).unwrap() {
            let _ = mem.apply(op);
        }
        assert_eq!(mem.tasks[&task_id].epic_id, Some(epic_id));
    }

    // ── Category 3: Concurrency Attacks ───────────────────────────────

    #[test]
    fn test_compact_during_concurrent_writes() {
        // 4 writer threads (each creates 5 tasks) + 1 compact thread, all behind
        // a barrier. After join: load_state succeeds and all 20 tasks are present,
        // because compact only clears the operations log, not materialized tables.
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".swarmit")).unwrap();
        let conn = open_db(dir.path()).unwrap();
        drop(conn);

        let root = Arc::new(dir.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(5)); // 4 writers + 1 compactor

        let writers: Vec<_> = (0..4)
            .map(|i| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = open_db(&root).unwrap();
                    barrier.wait();
                    for j in 0..5 {
                        let idx = (i * 5 + j + 1) as u32;
                        let agent = AgentId::new(&format!("writer-{i}")).unwrap();
                        let op = Operation::new(
                            agent,
                            OperationKind::CreateTask {
                                id: ItemId::new("TASK", idx),
                                title: format!("Task {idx}"),
                                description: None,
                                priority: Priority::Medium,
                                epic_id: None,
                            },
                        );
                        write_operation(&conn, &op).unwrap();
                    }
                })
            })
            .collect();

        let compactor = {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let conn = open_db(&root).unwrap();
                barrier.wait();
                // Compact may or may not interleave with writes
                let _ = compact_db(&conn);
            })
        };

        for w in writers {
            w.join().unwrap();
        }
        compactor.join().unwrap();

        let conn = open_db(dir.path()).unwrap();
        let state = load_state(&conn).unwrap();

        // All 20 tasks must be present: write_operation atomically inserts into
        // BOTH operations AND tasks. Compact only deletes operations.
        assert_eq!(
            state.tasks.len(),
            20,
            "all 20 tasks must survive concurrent compact"
        );
    }

    #[test]
    fn test_rapid_open_write_close_cycle() {
        // Stress: open/write/close 50 times. No corruption from connection churn.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".swarmit")).unwrap();

        // Seed sequences
        {
            let conn = open_db(dir.path()).unwrap();
            write_operation(
                &conn,
                &make_op(OperationKind::InitProject {
                    name: "Churn Test".to_string(),
                    description: None,
                    epic_prefix: None,
                    task_prefix: None,
                    auto_materialize: None,
                    materialize_path: None,
                }),
            )
            .unwrap();
        }

        for i in 1..=50u32 {
            let conn = open_db(dir.path()).unwrap();
            write_operation(
                &conn,
                &make_op(OperationKind::CreateTask {
                    id: ItemId::new("TASK", i),
                    title: format!("Task {i}"),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                }),
            )
            .unwrap();
            // Connection dropped here
        }

        let conn = open_db(dir.path()).unwrap();
        let state = load_state(&conn).unwrap();
        assert_eq!(state.tasks.len(), 50, "all 50 tasks should survive churn");
        assert_eq!(state.task_seq, 50);
    }

    #[test]
    fn test_concurrent_duplicate_create_task() {
        // 4 threads all create TASK-001 concurrently.
        // INSERT OR IGNORE: first write wins, others are no-ops.
        // All 4 ops land in the log, but only 1 task exists.
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".swarmit")).unwrap();
        let conn = open_db(dir.path()).unwrap();
        drop(conn);

        let root = Arc::new(dir.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(4));

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let root = Arc::clone(&root);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = open_db(&root).unwrap();
                    barrier.wait();
                    let agent = AgentId::new(&format!("racer-{i}")).unwrap();
                    let op = Operation::new(
                        agent,
                        OperationKind::CreateTask {
                            id: "TASK-001".parse().unwrap(),
                            title: format!("Title from thread {i}"),
                            description: None,
                            priority: Priority::Medium,
                            epic_id: None,
                        },
                    );
                    // All threads succeed (INSERT OR IGNORE)
                    write_operation(&conn, &op).unwrap();
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let conn = open_db(dir.path()).unwrap();

        // All 4 ops in the log
        assert_eq!(
            query_count(&conn, "SELECT COUNT(*) FROM operations"),
            4,
            "all 4 duplicate create ops should be in the log"
        );

        // But only 1 task (INSERT OR IGNORE)
        let state = load_state(&conn).unwrap();
        assert_eq!(
            state.tasks.len(),
            1,
            "INSERT OR IGNORE should produce exactly 1 task"
        );
    }

    // ── Category 4: Sequence Counter Attacks ──────────────────────────

    #[test]
    fn test_seq_gap_jump_and_fill() {
        // Sequence counters use MAX semantics. Creating TASK-050 sets task_seq=50.
        // Creating TASK-005 afterwards keeps it at 50. TASK-051 advances to 51.
        let (_dir, conn) = setup_db();

        write_operation(
            &conn,
            &make_op(OperationKind::InitProject {
                name: "Seq Test".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
                auto_materialize: None,
                materialize_path: None,
            }),
        )
        .unwrap();

        // Jump to 50
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: ItemId::new("TASK", 50),
                title: "Task 50".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();
        assert_eq!(load_state(&conn).unwrap().task_seq, 50);

        // Fill lower — seq stays at 50
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: ItemId::new("TASK", 5),
                title: "Task 5".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();
        let state = load_state(&conn).unwrap();
        assert_eq!(
            state.task_seq, 50,
            "task_seq should stay at 50 (MAX semantics)"
        );
        assert_eq!(state.tasks.len(), 2);

        // Advance to 51
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: ItemId::new("TASK", 51),
                title: "Task 51".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();
        assert_eq!(load_state(&conn).unwrap().task_seq, 51);
    }

    #[test]
    fn test_duplicate_create_diverges_db_vs_materializer() {
        // DIVERGENCE: DB uses INSERT OR IGNORE (first-writer-wins) while
        // materializer uses BTreeMap::insert (last-writer-wins).
        let (_dir, conn) = setup_db();

        let task_id: ItemId = "TASK-001".parse().unwrap();

        let op1 = make_op(OperationKind::CreateTask {
            id: task_id.clone(),
            title: "First".to_string(),
            description: None,
            priority: Priority::High,
            epic_id: None,
        });
        let op2 = make_op(OperationKind::CreateTask {
            id: task_id.clone(),
            title: "Second".to_string(),
            description: None,
            priority: Priority::Low,
            epic_id: None,
        });

        write_operation(&conn, &op1).unwrap();
        write_operation(&conn, &op2).unwrap();

        // DB: INSERT OR IGNORE preserves the first row
        let db_state = load_state(&conn).unwrap();
        assert_eq!(
            db_state.tasks[&task_id].title, "First",
            "DB should have 'First' (INSERT OR IGNORE preserves original)"
        );
        assert_eq!(db_state.tasks[&task_id].priority, Priority::High);

        // Materializer: BTreeMap::insert overwrites
        let mut mem = ProjectState::default();
        mem.apply(op1).unwrap();
        mem.apply(op2).unwrap();
        assert_eq!(
            mem.tasks[&task_id].title, "Second",
            "materializer should have 'Second' (BTreeMap overwrites)"
        );
        assert_eq!(mem.tasks[&task_id].priority, Priority::Low);

        // DIVERGENCE: DB and materializer disagree
        assert_ne!(
            db_state.tasks[&task_id].title, mem.tasks[&task_id].title,
            "DIVERGENCE: DB (first-writer-wins) vs materializer (last-writer-wins)"
        );
    }

    #[test]
    fn test_seq_independence_epic_vs_task() {
        // Epic and task sequence counters are independent.
        let (_dir, conn) = setup_db();

        write_operation(
            &conn,
            &make_op(OperationKind::InitProject {
                name: "Seq Independence".to_string(),
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
            &make_op(OperationKind::CreateEpic {
                id: ItemId::new("EPIC", 100),
                title: "Epic 100".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();
        assert_eq!(state.epic_seq, 100);
        assert_eq!(state.task_seq, 0, "task_seq unaffected by epic creation");

        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: ItemId::new("TASK", 5),
                title: "Task 5".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();
        assert_eq!(state.epic_seq, 100, "epic_seq unaffected by task creation");
        assert_eq!(state.task_seq, 5);

        // Lower epic number doesn't decrease seq
        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: ItemId::new("EPIC", 3),
                title: "Epic 3".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();
        assert_eq!(state.epic_seq, 100, "epic_seq stays at 100 (MAX semantics)");
        assert_eq!(state.task_seq, 5, "task_seq stays at 5");
    }

    // ── Category 5: Double-Option Edge Cases ──────────────────────────
    // UpdateTask.epic_id: Option<Option<ItemId>>
    //   None            → don't touch this field
    //   Some(Some(eid)) → set epic to eid
    //   Some(None)      → clear epic (detach)

    #[test]
    fn test_update_task_epic_id_all_variants() {
        let (_dir, conn) = setup_db();

        let epic1: ItemId = "EPIC-001".parse().unwrap();
        let epic2: ItemId = "EPIC-002".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        let setup_ops = vec![
            make_op(OperationKind::CreateEpic {
                id: epic1.clone(),
                title: "Epic 1".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::CreateEpic {
                id: epic2.clone(),
                title: "Epic 2".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
            make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Moving Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic1.clone()),
            }),
        ];
        let mut mem = ProjectState::default();
        for op in &setup_ops {
            write_operation(&conn, op).unwrap();
            mem.apply(op.clone()).unwrap();
        }

        // Step 1: epic_id: None → no change
        let noop_op = make_op(OperationKind::UpdateTask {
            id: task_id.clone(),
            title: None,
            description: None,
            priority: None,
            epic_id: None,
            assignee: None,
        });
        write_operation(&conn, &noop_op).unwrap();
        mem.apply(noop_op).unwrap();

        let db = load_state(&conn).unwrap();
        assert_eq!(
            db.tasks[&task_id].epic_id,
            Some(epic1.clone()),
            "epic_id: None should not change the epic"
        );
        assert_eq!(db.tasks[&task_id].epic_id, mem.tasks[&task_id].epic_id);

        // Step 2: Some(Some(EPIC-002)) → move to epic2
        let move_op = make_op(OperationKind::UpdateTask {
            id: task_id.clone(),
            title: None,
            description: None,
            priority: None,
            epic_id: Some(Some(epic2.clone())),
            assignee: None,
        });
        write_operation(&conn, &move_op).unwrap();
        mem.apply(move_op).unwrap();

        let db = load_state(&conn).unwrap();
        assert_eq!(db.tasks[&task_id].epic_id, Some(epic2.clone()));
        assert_eq!(db.tasks[&task_id].epic_id, mem.tasks[&task_id].epic_id);
        assert!(db.epics[&epic2].task_ids.contains(&task_id));
        assert!(!db.epics[&epic1].task_ids.contains(&task_id));

        // Step 3: Some(None) → clear epic
        let clear_op = make_op(OperationKind::UpdateTask {
            id: task_id.clone(),
            title: None,
            description: None,
            priority: None,
            epic_id: Some(None),
            assignee: None,
        });
        write_operation(&conn, &clear_op).unwrap();
        mem.apply(clear_op).unwrap();

        let db = load_state(&conn).unwrap();
        assert_eq!(
            db.tasks[&task_id].epic_id, None,
            "Some(None) should clear epic"
        );
        assert_eq!(db.tasks[&task_id].epic_id, mem.tasks[&task_id].epic_id);
        assert!(!db.epics[&epic2].task_ids.contains(&task_id));
    }

    #[test]
    fn test_update_task_assignee_all_variants() {
        // UpdateTask.assignee: Option<Option<AgentId>>
        //   None               → don't touch
        //   Some(Some(agent))  → set
        //   Some(None)         → clear to NULL
        let (_dir, conn) = setup_db();

        let task_id: ItemId = "TASK-001".parse().unwrap();

        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Assignee Test".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        )
        .unwrap();

        // Claim sets assignee to "test-agent"
        write_operation(
            &conn,
            &make_op(OperationKind::ClaimTask {
                id: task_id.clone(),
            }),
        )
        .unwrap();
        assert_eq!(
            load_state(&conn).unwrap().tasks[&task_id].assignee,
            Some(agent())
        );

        // assignee: None → no change
        write_operation(
            &conn,
            &make_op(OperationKind::UpdateTask {
                id: task_id.clone(),
                title: None,
                description: None,
                priority: None,
                epic_id: None,
                assignee: None,
            }),
        )
        .unwrap();
        assert_eq!(
            load_state(&conn).unwrap().tasks[&task_id].assignee,
            Some(agent()),
            "assignee: None should not change"
        );

        // assignee: Some(Some("bob")) → change
        let bob = AgentId::new("bob").unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::UpdateTask {
                id: task_id.clone(),
                title: None,
                description: None,
                priority: None,
                epic_id: None,
                assignee: Some(Some(bob.clone())),
            }),
        )
        .unwrap();
        assert_eq!(
            load_state(&conn).unwrap().tasks[&task_id].assignee,
            Some(bob)
        );

        // assignee: Some(None) → clear
        write_operation(
            &conn,
            &make_op(OperationKind::UpdateTask {
                id: task_id.clone(),
                title: None,
                description: None,
                priority: None,
                epic_id: None,
                assignee: Some(None),
            }),
        )
        .unwrap();
        assert_eq!(
            load_state(&conn).unwrap().tasks[&task_id].assignee,
            None,
            "Some(None) should clear assignee to NULL"
        );
    }

    #[test]
    fn test_detach_last_task_from_done_epic() {
        // When all tasks in an epic are Done, the epic auto-closes. If we then
        // detach the last task via UpdateTask { epic_id: Some(None) },
        // check_epic_completion returns early (empty task list → no status change).
        // The epic stays Done.
        let (_dir, conn) = setup_db();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        write_operation(
            &conn,
            &make_op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Auto-close Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }),
        )
        .unwrap();
        write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Only Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }),
        )
        .unwrap();

        // Complete task → epic auto-closes
        write_operation(
            &conn,
            &make_op(OperationKind::CompleteTask {
                id: task_id.clone(),
            }),
        )
        .unwrap();
        assert_eq!(
            load_state(&conn).unwrap().epics[&epic_id].status,
            Status::Done
        );

        // Detach task from epic
        write_operation(
            &conn,
            &make_op(OperationKind::UpdateTask {
                id: task_id.clone(),
                title: None,
                description: None,
                priority: None,
                epic_id: Some(None),
                assignee: None,
            }),
        )
        .unwrap();

        let state = load_state(&conn).unwrap();

        // Task list is empty
        assert!(state.epics[&epic_id].task_ids.is_empty());

        // Epic stays Done (check_epic_completion returns early for empty task list)
        assert_eq!(
            state.epics[&epic_id].status,
            Status::Done,
            "epic stays Done after detaching last task (empty list → early return)"
        );

        // DIVERGENCE: materializer replay from log does NOT agree.
        // Option<Option<ItemId>> with value Some(None) serializes to JSON `null`,
        // which deserializes back as None ("don't touch") instead of Some(None)
        // ("clear"). So replaying from the log doesn't detach the task.
        let mut mem = ProjectState::default();
        for op in read_all_operations(&conn).unwrap() {
            let _ = mem.apply(op);
        }
        assert_eq!(
            mem.epics[&epic_id].status,
            Status::Done,
            "epic status agrees (Done from auto-close)"
        );
        assert!(
            !mem.epics[&epic_id].task_ids.is_empty(),
            "DIVERGENCE: replay keeps task in epic (Some(None) → null → None on serde roundtrip)"
        );
    }

    // ── Category 6: Replay Safety ─────────────────────────────────────

    #[test]
    fn test_replay_mixed_valid_and_ghost_ops() {
        // Write a mix of valid and ghost ops. Replay onto fresh ProjectState:
        // valid ops succeed, ghost ops return Err. No panics.
        let (_dir, conn) = setup_db();

        let valid_ops = [
            make_op(OperationKind::InitProject {
                name: "Replay Test".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
                auto_materialize: None,
                materialize_path: None,
            }),
            make_op(OperationKind::CreateTask {
                id: "TASK-001".parse().unwrap(),
                title: "Real 1".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
            make_op(OperationKind::CreateTask {
                id: "TASK-002".parse().unwrap(),
                title: "Real 2".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        ];

        let ghost_ops = [
            make_op(OperationKind::UpdateTask {
                id: "TASK-999".parse().unwrap(),
                title: Some("Ghost".to_string()),
                description: None,
                priority: None,
                epic_id: None,
                assignee: None,
            }),
            make_op(OperationKind::ClaimTask {
                id: "TASK-888".parse().unwrap(),
            }),
            make_op(OperationKind::CompleteTask {
                id: "TASK-777".parse().unwrap(),
            }),
        ];

        // All 6 succeed at DB level
        for op in valid_ops.iter().chain(ghost_ops.iter()) {
            write_operation(&conn, op).unwrap();
        }
        assert_eq!(query_count(&conn, "SELECT COUNT(*) FROM operations"), 6);

        // Replay onto fresh state
        let mut mem = ProjectState::default();
        let mut ok_count = 0;
        let mut err_count = 0;
        for op in read_all_operations(&conn).unwrap() {
            match mem.apply(op) {
                Ok(()) => ok_count += 1,
                Err(_) => err_count += 1,
            }
        }

        assert_eq!(ok_count, 3, "3 valid ops should succeed");
        assert_eq!(err_count, 3, "3 ghost ops should fail");
        assert_eq!(mem.tasks.len(), 2, "only 2 real tasks in replayed state");
    }

    #[test]
    fn test_compact_makes_log_replay_impossible() {
        // After compact, the log is empty. Replaying from the log produces an
        // empty state. But load_state returns the full materialized state.
        // This documents that post-compact the system cannot be rebuilt from log alone.
        let (_dir, conn) = setup_db();

        write_operation(
            &conn,
            &make_op(OperationKind::InitProject {
                name: "Compact Replay".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
                auto_materialize: None,
                materialize_path: None,
            }),
        )
        .unwrap();

        for i in 1..=5 {
            write_operation(
                &conn,
                &make_op(OperationKind::CreateTask {
                    id: ItemId::new("TASK", i),
                    title: format!("Task {i}"),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: None,
                }),
            )
            .unwrap();
        }

        compact_db(&conn).unwrap();

        // Log is empty
        let ops = read_all_operations(&conn).unwrap();
        assert!(ops.is_empty(), "log should be empty after compact");

        // Replay from empty log → empty state
        let mut replayed = ProjectState::default();
        for op in &ops {
            let _ = replayed.apply(op.clone());
        }
        assert!(replayed.config.is_none());
        assert!(replayed.tasks.is_empty());

        // load_state → full materialized state
        let db_state = load_state(&conn).unwrap();
        assert!(db_state.config.is_some());
        assert_eq!(db_state.tasks.len(), 5);
    }

    #[test]
    fn test_rollback_on_inner_failure() {
        // If apply_to_db fails (e.g., table dropped), the transaction should
        // rollback: no partial log entry.
        let (_dir, conn) = setup_db();

        // Sabotage: drop the tasks table
        conn.execute_batch("DROP TABLE tasks").unwrap();

        // write_operation should fail
        let result = write_operation(
            &conn,
            &make_op(OperationKind::CreateTask {
                id: "TASK-001".parse().unwrap(),
                title: "Doomed".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }),
        );
        assert!(
            result.is_err(),
            "write should fail when tasks table is dropped"
        );

        // Rollback prevented the log entry
        assert_eq!(
            query_count(&conn, "SELECT COUNT(*) FROM operations"),
            0,
            "rollback should prevent partial log entry"
        );
    }
}
