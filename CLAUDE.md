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

## Task Management (Swarmit)

**Use swarmit instead of Claude Code's built-in todo tools.**
Never use `TodoWrite`, `TaskCreate`, `TaskUpdate`, or `TaskList` in this project.
All task tracking goes through the swarmit CLI so it persists to `.swarmit/operations.log`
and is visible to all agents and the TUI.

| Instead of | Use |
|------------|-----|
| `TaskCreate` / `TodoWrite` | `swarmit task create --title "..." --agent claude` |
| Mark in-progress | `swarmit task claim TASK-NNN --agent claude` |
| Mark complete | `swarmit task done TASK-NNN --agent claude` |
| `TaskList` | `swarmit task list --status todo --json` |

### Status values

`todo` · `in_progress` (aliases: `wip`, `inprogress`) · `done` · `blocked` · `cancelled`

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
