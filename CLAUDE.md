# Swarmit — Developer Guide for Claude Code

## Project Structure

```
crates/
  swarmit-core/   # Models, event sourcing, state materializer
  swarmit-cli/    # CLI commands (clap)
  swarmit-tui/    # Terminal UI (ratatui + crossterm)
  swarmit/        # Binary entry point (mode detection)
```

## Build & Test

```bash
cargo build          # Build all crates
cargo test           # Run all 42 tests
cargo run -- --help  # CLI help
```

Tests use `tempfile` for isolated directories — no global state.

## Architecture

**Event sourcing:** All mutations write an `Operation` (JSONL line) to `.swarmit/operations.log`.
State is rebuilt by replaying the log. The write path:
1. Acquire `fd-lock` on `operations.lock` (5s timeout, 10ms retry)
2. Append serialized `Operation` as a JSONL line
3. `fsync` for durability
4. Release lock

**Materializer:** `ProjectState::from_log()` replays all operations. `apply()` is the state machine.
`BTreeMap` for deterministic ordering. Incremental reads via `read_operations_since(offset)`.

**IDs:** `ItemId` in PREFIX-NNN format (e.g., `TASK-001`). Sequence counters tracked in `ProjectState.epic_seq` / `task_seq`.

**CLI dispatch:** `swarmit-cli/src/lib.rs` matches `&cli.command` (by reference to avoid partial moves).
All command `run()` functions take `&XxxArgs, &Cli`.

**TUI mode detection:** In `swarmit/src/main.rs`:
- No subcommand + TTY → launch TUI
- No subcommand + pipe → error
- Subcommand → CLI dispatch

## Key Files

| File | Purpose |
|------|---------|
| `crates/swarmit-core/src/events/operations.rs` | All `OperationKind` variants |
| `crates/swarmit-core/src/state/materializer.rs` | `ProjectState::apply()` — the state machine |
| `crates/swarmit-core/src/events/locking.rs` | `fd-lock` write path |
| `crates/swarmit-cli/src/commands/` | One file per command group |
| `crates/swarmit-tui/src/app.rs` | TUI `App` struct + event loop state |
| `crates/swarmit-tui/src/lib.rs` | TUI entry point + keyboard dispatch |

## Adding a New Operation Kind

1. Add variant to `OperationKind` in `operations.rs`
2. Handle it in `ProjectState::apply()` in `materializer.rs`
3. Add a unit test in the `tests` block of `materializer.rs`
4. Wire up the CLI command if needed

## Output Format

All CLI mutations require `--agent <ID>` or `SWARMIT_AGENT` env var.
JSON output (`--json`) always uses `{ "ok": bool, "data": ..., "error": ... }`.
TTY auto-detection: piped stdout → JSON; terminal → pretty text.

## Skill Files

`.claude/skills/swarmit/SKILL.md` — Claude Code skill for agents using swarmit.
See `.claude/skills/swarmit/cli-reference.md` for full command reference.
