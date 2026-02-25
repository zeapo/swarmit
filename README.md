# Swarmit

**Local-first project management for multi-agent workflows.**

Swarmit lets multiple AI agents collaborate on the same project without stepping on each other. Tasks and epics live in an append-only event log — every agent reads the same state, every write is lock-protected, and the built-in TUI gives you a live view of what's happening.

---

## What it does

- **Append-only event log** — all mutations are JSONL operations written to `.swarmit/operations.log`. State is rebuilt by replaying the log, so nothing is ever silently lost.
- **Concurrent agents** — file-based locking (`fd-lock`) means multiple agents can write safely at the same time.
- **CLI** — create tasks, claim work, mark done, add comments, link items, inspect history.
- **TUI** — a live terminal dashboard that refreshes automatically when agents change state.
- **Claude Code plugin** — teaches agents to use swarmit instead of built-in todos, so task state is always in the log and visible to everyone.

---

## Features

- Event sourcing with `fsync`-safe writes
- Per-write exclusive locking with 5 s timeout / 10 ms retry
- Epics and tasks with priorities, assignees, and statuses
- Typed relationships: `blocks`, `blocked_by`, `parent`, `child`, `relates_to`, `duplicates`
- Comments on any task
- `swarmit compact` to rotate and snapshot the log
- TUI with live refresh, keyboard navigation, sort/filter dialogs
- Catppuccin theme (auto light/dark detection, `SWARMIT_THEME` override)
- JSON output mode for scripting and agent use

---

## Installation

Swarmit is built from source with Cargo:

```bash
git clone https://github.com/zeapo/swarmit
cd swarmit
cargo install --path crates/swarmit
```

Requires Rust 1.80 or newer. Pre-built binaries are not yet published.

---

## Quick Start

```bash
# Initialize a project in the current directory
swarmit init --name "My Project" --agent me

# Create an epic
swarmit epic create --title "Authentication" --agent me

# Create a task inside the epic
swarmit task create --title "Implement login flow" --epic EPIC-001 --agent me

# Claim it (marks as In Progress)
swarmit task claim TASK-001 --agent me

# Mark it done
swarmit task done TASK-001 --agent me
```

---

## CLI Usage

All mutation commands require `--agent <ID>` (or `SWARMIT_AGENT` env var). Output is pretty-printed on a TTY and JSON when piped.

### Command groups

| Group | Description |
|-------|-------------|
| `swarmit init` | Initialize a new project |
| `swarmit epic` | Create, list, show, update, delete epics |
| `swarmit task` | Create, list, show, update, claim, done, delete tasks |
| `swarmit link` | Add / remove / list typed relationships between items |
| `swarmit comment` | Add comments to tasks |
| `swarmit log` | Inspect the raw operation log |
| `swarmit compact` | Snapshot and rotate the log |

### Examples

```bash
# List all open tasks
swarmit task list --status todo

# Show full task detail (relationships + comments)
swarmit task show TASK-007

# Link two tasks
swarmit link add --from TASK-001 --to TASK-002 --type blocks --agent me

# Add a progress comment
swarmit comment add TASK-001 --body "OAuth flow implemented, tests passing" --agent me

# Tail the operation log
swarmit log --tail 20

# JSON output for scripting
swarmit task list --status todo --json
```

See [`plugin/skills/swarmit/cli-reference.md`](plugin/skills/swarmit/cli-reference.md) for the full command reference.

---

## TUI

Run `swarmit` with no arguments in a TTY to open the terminal dashboard:

```bash
swarmit
```

The TUI polls for file changes and refreshes automatically when agents update state.

### Key bindings

| Key | Action |
|-----|--------|
| `Tab` / `Shift-Tab` | Switch panel |
| `j` / `k` or `↑` / `↓` | Navigate list |
| `Enter` | Select / expand |
| `n` | New task |
| `c` | Claim selected task |
| `d` | Mark selected task done |
| `f` | Filter dialog |
| `s` | Sort dialog |
| `?` | Help |
| `q` / `Esc` | Quit / back |

### Theme

Swarmit uses Catppuccin and auto-detects light vs dark terminal background. Override with:

```bash
SWARMIT_THEME=latte swarmit        # light
SWARMIT_THEME=mocha swarmit        # dark
SWARMIT_THEME=frappe swarmit
SWARMIT_THEME=macchiato swarmit
```

---

## Claude Code Plugin

The swarmit plugin teaches Claude Code agents to use `swarmit` instead of the built-in todo tools, so task state persists to the event log and is visible to all agents and the TUI.

### Install

```
/plugin marketplace add zeapo/swarmit
/plugin install swarmit
```

### What it does

Once installed, agents will:
- Create tasks with `swarmit task create` instead of `TodoWrite`
- Claim tasks before starting work
- Mark tasks done with `swarmit task done`
- Add progress comments visible to all other agents in real time

---

## Project Structure

```
crates/
  swarmit-core/   # Models, event sourcing, state materializer
  swarmit-cli/    # CLI commands (clap)
  swarmit-tui/    # Terminal UI (ratatui + crossterm)
  swarmit/        # Binary entry point (mode detection)
plugin/
  skills/
    swarmit/      # Claude Code skill files
```

---

## Development

```bash
cargo build          # Build all crates
cargo test           # Run all tests
cargo run -- --help  # CLI help
```

Tests use `tempfile` for isolated directories — no global state.

---

## License

MIT
