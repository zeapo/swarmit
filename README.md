# Swarmit

**Task coordination for multi-agent Claude Code workflows.**

When you run several Claude Code agents in parallel, they each have their own built-in todo list — invisible to every other agent and gone when the session ends. Swarmit replaces that with a shared task board every agent reads and writes through the same CLI. One source of truth. Live terminal dashboard. No duplicated work.

---

## How it works

```
  Agent A          Agent B          Agent C
    │                 │                │
    │  swarmit CLI    │  swarmit CLI   │  swarmit CLI
    └────────┬────────┘                │
             ▼                         │
       shared task state  ◄────────────┘
             │
             ▼
         swarmit TUI
    (live-refreshing dashboard
     you watch while agents run)
```

Agents create tasks, claim them before starting, leave comments as they work, and mark them done — all through the CLI. The TUI reflects the current state in real time.

---

## Install

```bash
git clone https://github.com/zeapo/swarmit
cd swarmit
cargo install --path crates/swarmit
```

Requires Rust 1.80+.

---

## Quick start

```bash
# Initialize a project
swarmit init --name "My Project" --agent me

# Create an epic and a task
swarmit epic create --title "Authentication" --agent me
swarmit task create --title "Implement login flow" --epic EPIC-001 --agent me

# Agents claim before starting, done when finished
swarmit task claim TASK-001 --agent claude-1
swarmit task done  TASK-001 --agent claude-1
```

All mutation commands require `--agent <ID>` (or `SWARMIT_AGENT` env var). Output is pretty-printed on a TTY and JSON when piped — useful for scripting and for agent-to-agent communication.

Or just let Claude Code do it for you ;)

---

## Claude Code plugin

The plugin teaches Claude Code agents to use swarmit instead of the built-in todo tools. Once installed, agents automatically:

- Create tasks with `swarmit task create` (visible to all other agents)
- Claim tasks before starting (prevents duplicate work)
- Leave progress comments visible in the TUI in real time
- Mark tasks done when finished

**Install:**

```
/plugin marketplace add zeapo/swarmit
/plugin install swarmit
```

Or copy the skill file manually into your `.claude/skills/` directory — see [`plugin/skills/swarmit/`](plugin/skills/swarmit/).

---

## CLI

### Command groups

| Command | Description |
|---------|-------------|
| `swarmit init` | Initialize a project in the current directory |
| `swarmit epic` | Create, list, show, update, delete epics |
| `swarmit task` | Create, list, show, update, claim, done, delete tasks |
| `swarmit link` | Add / remove / list typed relationships |
| `swarmit comment` | Add comments to any task |
| `swarmit log` | Inspect the history of all changes |
| `swarmit compact` | Prune and snapshot history |

### Common examples

```bash
# List all open tasks
swarmit task list --status todo

# Show task detail with relationships and comments
swarmit task show TASK-007

# Link tasks
swarmit link add --from TASK-001 --to TASK-002 --type blocks --agent me

# Add a progress comment
swarmit comment add TASK-001 --body "OAuth flow implemented, tests passing" --agent me

# Review recent activity
swarmit log --tail 20

# JSON output for scripting
swarmit task list --status todo --json
```

Full reference: [`plugin/skills/swarmit/cli-reference.md`](plugin/skills/swarmit/cli-reference.md)

---

## TUI

Run `swarmit` with no arguments in a TTY to open the live dashboard:

```bash
swarmit
```

The TUI polls for file changes and refreshes automatically as agents update state. Catppuccin theme with auto light/dark detection.

```bash
SWARMIT_THEME=latte     swarmit   # light
SWARMIT_THEME=mocha     swarmit   # dark
SWARMIT_THEME=frappe    swarmit
SWARMIT_THEME=macchiato swarmit
```

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
| `←` / `→` | Resize detail panel |
| `?` | Help |
| `q` / `Esc` | Quit / back |

---

## What's in the box

- Epics and tasks with priority, assignee, and status
- Typed relationships between items (`blocks`, `blocked_by`, `parent`, `child`, `relates_to`, `duplicates`)
- Comments on any task
- Full history with `swarmit log`
- JSON output on every command — pipe-friendly for agents and scripts
- TUI with sort, filter, live refresh, and resizable panels
- Catppuccin theme with auto dark/light detection
- Claude Code skill file that wires everything together

---

## Project layout

```
crates/
  swarmit-core/   # Models, event sourcing, state materializer
  swarmit-cli/    # CLI commands (clap)
  swarmit-tui/    # Terminal UI (ratatui + crossterm)
  swarmit/        # Binary entry point + mode detection
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