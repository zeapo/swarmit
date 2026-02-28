# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[1.1.0]: https://github.com/zeapo/swarmit/compare/v1.0.2...v1.1.0
[1.0.2]: https://github.com/zeapo/swarmit/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/zeapo/swarmit/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/zeapo/swarmit/releases/tag/v1.0.0
