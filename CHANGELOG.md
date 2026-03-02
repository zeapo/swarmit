# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.2] - 2026-03-02

### Fixed

- Publish atomic ID allocation fix for tasks and epics. Version 1.2.1
  was published before the fix landed, so users on 1.2.1 from crates.io
  still had the TOCTOU bug where sequential `task create` calls could
  produce duplicate IDs.

### Added

- Regression tests for sequential ID allocation without `InitProject`
  (single-connection and cross-connection scenarios).

## [1.2.1] - 2026-03-01

### Fixed

- Atomic ID allocation for tasks and epics. `create_task_op` and
  `create_epic_op` now use `BEGIN IMMEDIATE` transactions to serialize
  the sequence counter increment, ID allocation, and operation write,
  eliminating a TOCTOU race that could produce duplicate IDs under
  concurrent access.
- Epic existence validation moved inside the atomic transaction,
  closing a window where an epic could be deleted between validation
  and task creation.

## [1.2.0] - 2026-03-01

### Added

- `task cancel` and `epic cancel` CLI commands with `--reason` flag.
- Epic cancellation cascades to all non-terminal child tasks.
- Cancelled items hidden from default `task list` and `epic list` output;
  use `--all` or `--status cancelled` to view them.
- Auto-completion logic treats cancelled tasks as terminal alongside done.
- TUI filter and status bar support for cancelled status.

## [1.1.0] - 2026-02-28

### Changed

- **Migrated persistence from JSONL event log to single SQLite database.**
  All state now lives in `.swarmit/state.db` (WAL mode, `busy_timeout=5000`).
  Every mutation is a single `BEGIN IMMEDIATE` transaction that atomically
  inserts the operation into the `operations` table and updates materialized
  state tables via `apply_to_db()`. Replaces the previous JSONL log + fd-lock +
  snapshot cache architecture.
- Legacy `operations.log` files are automatically imported on first open and
  renamed to `.bak`. Old v1 snapshot databases (with `meta` table) are also
  backed up and recreated.
- Markdown materialization is now configurable via `auto_materialize` and
  `materialize_path` in project config.

### Added

- 20 adversarial stress tests covering ghost operations, orphan references,
  concurrency attacks, sequence counter edge cases, double-option semantics,
  and replay safety. Documents 3 real DB/materializer divergences.
- `compact_db()` API to delete the operations log and VACUUM, leaving
  materialized state tables intact.
- `count_operations()` and `read_operations_since()` APIs for incremental
  polling (used by TUI).

### Removed

- `fd-lock`, `notify`, and `notify-debouncer-mini` dependencies (no longer
  needed with SQLite WAL concurrency).
- `snapshot.rs`, `locking.rs`, `log.rs` modules from the state layer.

## [1.0.2] - 2026-02-28

### Fixed

- Resolve all clippy warnings; `just check` now runs `clippy -D warnings`.
- Apply rustfmt across codebase.

## [1.0.1] - 2026-02-27

### Changed

- Merged 4-crate workspace (`swarmit-core`, `swarmit-cli`, `swarmit-tui`,
  `swarmit`) into a single crate for simpler crates.io publishing.
- Added `justfile` with `build`, `test`, `lint`, `check`, and `publish` recipes.

### Added

- Mouse wheel scrolling support in all TUI panels.

## [1.0.0] - 2026-02-26

### Added

- Initial release: local-first project management for multi-agent Claude Code
  workflows.
- CLI with full CRUD for projects, epics, tasks, relationships, comments, and
  insights.
- Terminal UI (ratatui + crossterm) with tree view, detail pane, filtering,
  sorting, and Vim-style navigation.
- Event-sourced architecture with deterministic replay.
- Skill files for Claude Code agent integration.

[1.2.2]: https://github.com/zeapo/swarmit/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/zeapo/swarmit/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/zeapo/swarmit/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/zeapo/swarmit/compare/v1.0.2...v1.1.0
[1.0.2]: https://github.com/zeapo/swarmit/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/zeapo/swarmit/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/zeapo/swarmit/releases/tag/v1.0.0
