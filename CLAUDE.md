# Swarmit — Developer Guide for Claude Code

## Project Structure

```
src/
  main.rs          # Binary entry point (mode detection)
  lib.rs           # Public API, re-exports from state::db
  models/          # Domain types: Task, Epic, ItemId, Status, etc.
  events/          # Event sourcing: Operation, OperationKind
  state/           # DB layer, materializer, index, markdown sync
  cli/             # CLI commands (clap)
    mod.rs         # Cli struct, Commands enum, run()
    output.rs      # JSON envelope, OutputMode
    commands/      # One file per command group
  tui/             # Terminal UI (ratatui + crossterm)
    mod.rs         # TUI entry point, keyboard dispatch, render loop
    app.rs         # App struct, event loop state
    events.rs      # Action, Modal, Screen, Focus enums
    theme.rs       # Catppuccin theme detection
    editor.rs      # External editor integration
    components/    # UI components (tree_list, detail_pane, etc.)
tests/
  integration.rs   # Core event sourcing integration tests
  cli_roundtrip.rs # CLI round-trip tests
justfile           # Build, test, lint, publish recipes
```

Single crate — no workspace. Published to crates.io as `swarmit`.

## Build & Test

```bash
cargo build          # Build
cargo test           # Run all 124 tests
cargo run -- --help  # CLI help
just check           # fmt + lint + test
just publish         # Full publish to crates.io
```

Tests use `tempfile` for isolated directories — no global state.

**Always run `just check` after writing code.** This runs `fmt-check` + `clippy` + `test`
in sequence. Fix any errors before committing. Clippy runs with `-D warnings` so all
warnings are treated as errors.

## Architecture

**Single SQLite database:** All state lives in `.swarmit/state.db` (WAL mode, `busy_timeout=5000`).
Every mutation is a single `BEGIN IMMEDIATE` transaction that atomically:
1. INSERTs the `Operation` into the `operations` table
2. Updates materialized state tables via `apply_to_db()`

No separate log file or lock file. SQLite WAL + `busy_timeout` handles concurrency.

**Tables:** `migrations`, `operations` (event log), `config`, `epics`, `epic_task_ids`,
`tasks`, `relationships`, `comments`, `insights`, `sequences` (materialized state).

**Public API** (re-exported from `src/state/db.rs` via `src/lib.rs`):
- `open_db(project_root)` — open/create DB, run migrations, import legacy if needed
- `load_state(conn)` — SELECT from materialized tables → `ProjectState`
- `write_operation(conn, op)` / `write_operations(conn, ops)` — atomic write
- `read_operations_since(conn, after_rowid)` — incremental read for TUI polling
- `read_all_operations(conn)` / `latest_rowid(conn)` / `compact_db(conn)`

**Materializer:** `ProjectState::apply()` is the in-memory state machine.
`apply_to_db()` mirrors it in SQL. `BTreeMap` for deterministic ordering.

**IDs:** `ItemId` in PREFIX-NNN format (e.g., `TASK-001`). Sequence counters tracked in
the `sequences` table and `ProjectState.epic_seq` / `task_seq`.

**Legacy migration:** On first `open_db()`, if `operations.log` exists it is imported into
the `operations` table and renamed to `.bak`. Old v1 snapshot DBs (with `meta` table) are
also backed up and recreated.

**CLI dispatch:** `src/cli/mod.rs` matches `&cli.command` (by reference to avoid partial moves).
All command `run()` functions take `&XxxArgs, &Cli`.

**TUI mode detection:** In `src/main.rs`:
- No subcommand + TTY → launch TUI
- No subcommand + pipe → error
- Subcommand → CLI dispatch

**Module imports:** Since the workspace was merged into a single crate:
- Core modules live at `crate::models`, `crate::events`, `crate::state`
- CLI modules live at `crate::cli::*`
- TUI modules live at `crate::tui::*`
- The `prof_guard!` macro is defined in `tui/mod.rs` and re-exported at crate root

## Key Files

| File | Purpose |
|------|---------|
| `src/events/operations.rs` | All `OperationKind` variants |
| `src/state/db.rs` | SQLite layer: open, read, write, migrate |
| `src/state/materializer.rs` | `ProjectState::apply()` — the in-memory state machine |
| `src/cli/commands/` | One file per command group |
| `src/tui/app.rs` | TUI `App` struct + event loop state |
| `src/tui/mod.rs` | TUI entry point + keyboard dispatch |

## Adding a New Operation Kind

1. Add variant to `OperationKind` in `src/events/operations.rs`
2. Handle it in `ProjectState::apply()` in `src/state/materializer.rs`
3. Handle it in `apply_to_db()` in `src/state/db.rs`
4. Add unit tests in both `materializer.rs` and `db.rs`
5. Wire up the CLI command if needed

## Changing the Database Schema

The database (`.swarmit/state.db`) is the single source of truth. When modifying
any stored struct (`ProjectConfig`, `Task`, `Epic`, `Comment`, `Insight`, `Relationship`):

1. Add a new migration in `run_migrations()` in `src/state/db.rs`
2. Update the corresponding `read_*` and `apply_to_db()` functions in `db.rs`
3. Add `#[serde(default)]` on the corresponding Rust field (for operations JSON compat)
4. Add a test verifying the new field round-trips through write/load

## Output Format

All CLI mutations require `--agent <ID>` or `SWARMIT_AGENT` env var.
JSON output (`--json`) always uses `{ "ok": bool, "data": ..., "error": ... }`.
TTY auto-detection: piped stdout → JSON; terminal → pretty text.

## Publishing

```bash
just check           # Run fmt, lint, test
just publish-dry-run # Verify packaging
just publish         # Publish to crates.io
just bump 1.2.0      # Bump version in Cargo.toml
```

The `exclude` list in `Cargo.toml` keeps docs, IDE files, and `.swarmit/` out of the package.

**Version sync:** When bumping the version, update all three files together:
- `Cargo.toml` — crate version
- `plugin/skills/swarmit/SKILL.md` — expected version in the "Version Check" section
- `plugin/plugin.json` — plugin version (if present)

## Task Management (Swarmit)

**Use swarmit instead of Claude Code's built-in todo tools.**
Never use `TodoWrite`, `TaskCreate`, `TaskUpdate`, or `TaskList` in this project.
All task tracking goes through the swarmit CLI so it persists to `.swarmit/state.db`
and is visible to all agents and the TUI.

| Instead of | Use |
|------------|-----|
| `TaskCreate` / `TodoWrite` | `swarmit task create --title "..." --agent claude` |
| Mark in-progress | `swarmit task claim TASK-NNN --agent claude` |
| Mark complete | `swarmit task done TASK-NNN --agent claude` |
| `TaskList` | `swarmit task list --status todo --json` |

### Status values

`todo` · `in_progress` (aliases: `wip`, `inprogress`) · `done` · `blocked` · `cancelled`

Cancelled items are hidden from default `task list` and `epic list` output. Use `--all` to include them, or `--status cancelled` to see only cancelled items. Use `task cancel` / `epic cancel` with `--reason` to cancel (epic cancel cascades to all non-terminal tasks).

### Before starting work

1. Check for existing tasks: `swarmit task list --json`
2. If a matching task exists, claim it; otherwise create one
3. For multi-step work, create an epic + tasks

### Planning new work

1. Create an epic: `swarmit epic create --title "..." --description "..." --agent claude`
2. Create tasks with **full descriptions** (every step, file path, command — not just titles)
3. Set up dependencies: `swarmit link add --from TASK-X --to TASK-Y --type blocks --agent claude`
   - The inverse (`blocked_by`) is added automatically — don't create both directions
4. Identify tasks with no blockers — these are candidates for parallel dispatch

### Executing work

**For epics with 3+ independent tasks**, use the `superpowers:dispatching-parallel-agents` skill and execute in waves:

1. Fetch tasks: `swarmit task list --epic EPIC-NNN --status todo --json`
2. Partition into waves by dependency graph:
   - Wave 1: tasks with no blockers → dispatch all as parallel subagents
   - Wave N: tasks unblocked after previous wave → dispatch in parallel
3. Each subagent receives the full task description from `swarmit task show`, claims with a unique agent ID (`claude-1`, `claude-2`, …), and marks done on completion
4. After each wave, re-check for newly unblocked tasks

For smaller work (1–2 tasks), execute sequentially in the current session.

### During execution (single task)

- **Claim**: `swarmit task claim TASK-NNN --agent claude`
- **Progress**: `swarmit comment add TASK-NNN --body "..." --agent claude`
- **Blocked**: `swarmit task update TASK-NNN --status blocked --agent claude` (with a comment explaining why)
- **Done**: `swarmit task done TASK-NNN --agent claude`
- Never leave a task claimed but unfinished without a comment

### Agent identity

Use `--agent claude` for single-session work. For parallel subagents, use numbered (`claude-1`, `claude-2`) or role-based (`claude-backend`, `claude-tests`) IDs.

## Skill Files

`.claude/skills/swarmit/SKILL.md` — Claude Code skill for agents using swarmit.
See `.claude/skills/swarmit/cli-reference.md` for full command reference.
