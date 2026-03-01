# Swarmit

![Swarmit demo](docs/demo.webp)

Persistent task coordination for AI coding agents. Break projects into epics and tasks with dependency graphs, let multiple agents claim and complete work in parallel, and watch progress in a live terminal dashboard. State persists across sessions in a single SQLite database.

## Install

### From crates.io (recommended)

```bash
cargo install swarmit
```

Requires Rust 1.80+. Published at [crates.io/crates/swarmit](https://crates.io/crates/swarmit).

### Pre-built binaries

Download the latest release from [GitHub Releases](https://github.com/zeapo/swarmit/releases):

- **Linux x86_64**: `swarmit-linux-x86_64.tar.gz`
- **macOS ARM64**: `swarmit-macos-arm64.tar.gz`

```bash
# Example: macOS ARM64
tar xzf swarmit-macos-arm64.tar.gz
mv swarmit /usr/local/bin/
```

### From source

```bash
git clone https://github.com/zeapo/swarmit
cd swarmit
cargo install --path .
```

## Quick start

```bash
# Initialize a project
swarmit init --name "My Project" --agent me

# Plan the work
swarmit epic create --title "Authentication" --agent me
swarmit task create --title "Implement login flow" --epic EPIC-001 --agent me
swarmit task create --title "Add session middleware" --epic EPIC-001 --agent me
swarmit link add --from TASK-001 --to TASK-002 --type blocks --agent me

# Agents claim before starting, comment as they go, done when finished
swarmit task claim TASK-002 --agent claude-1
swarmit comment add TASK-002 --body "OAuth flow implemented, tests passing" --agent claude-1
swarmit task done  TASK-002 --agent claude-1
```

All mutation commands require `--agent <ID>` (or `SWARMIT_AGENT` env var). Output is pretty-printed on a TTY and JSON when piped — useful for scripting and agent-to-agent communication.

## Claude Code plugin

Teaches Claude Code agents to use swarmit instead of the built-in todo tools.

```
/plugin marketplace add zeapo/swarmit
/plugin install swarmit
```

Or copy the skill file manually into your `.claude/skills/` directory — see [`plugin/skills/swarmit/`](plugin/skills/swarmit/).

## CLI

| Command | Description |
|---------|-------------|
| `swarmit init` | Initialize a project in the current directory |
| `swarmit epic` | Create, list, show, update, delete epics |
| `swarmit task` | Create, list, show, update, claim, done, delete tasks |
| `swarmit link` | Add / remove / list typed relationships |
| `swarmit comment` | Add comments to any task |
| `swarmit insight` | Attach code insights to tasks |
| `swarmit log` | Inspect the history of all changes |
| `swarmit compact` | Prune and snapshot history |
| `swarmit project` | Show or update project metadata |
| `swarmit sync` | Regenerate markdown files from state |

```bash
# List all open tasks
swarmit task list --status todo

# Tasks for a specific epic, as JSON
swarmit task list --epic EPIC-001 --status todo --json

# Show task detail with relationships and comments
swarmit task show TASK-007

# Link tasks
swarmit link add --from TASK-001 --to TASK-002 --type blocks --agent me

# Add a progress comment
swarmit comment add TASK-001 --body "OAuth flow implemented, tests passing" --agent me

# Review recent activity
swarmit log --tail 20
```

Full reference: [`plugin/skills/swarmit/cli-reference.md`](plugin/skills/swarmit/cli-reference.md)

## TUI

Run `swarmit` with no arguments to open the live dashboard. It polls for changes and refreshes automatically as agents update state.

Catppuccin theme with automatic light/dark detection (`SWARMIT_THEME=latte|mocha|frappe|macchiato`).

### Key bindings

| Key | Action |
|-----|--------|
| `j`/`k` or `↑`/`↓` | Navigate list / scroll detail |
| `Enter` | Open detail pane |
| `Space` | Collapse / expand epic group |
| `Tab` | Cycle detail tabs |
| `h`/`l` or `←`/`→` | Cycle detail tabs |
| `Shift+H/J/K/L` | Switch pane focus |
| `n` | New task |
| `e` | Edit description in `$EDITOR` |
| `a` | Add comment in `$EDITOR` |
| `S` | Change task status |
| `E` | Change task epic |
| `f` | Filter |
| `s` | Sort |
| `r` | Refresh |
| `\|` | Toggle split direction |
| `=` / `-` | Resize detail pane |
| `?` | Help |
| `q` / `Esc` | Quit / back |

## Markdown export

One-way export of state as markdown files (one per epic and task). Editing the files has no effect on swarmit state.

```bash
# Enable auto-export on init
swarmit init --name "My Project" --auto-materialize --agent me

# Or toggle on an existing project
swarmit project update --auto-materialize true --agent me

# Manual one-off export
swarmit sync                        # writes to .swarmit/state/ (default)
swarmit sync --path ./docs/tasks    # writes to a custom directory
```

## Development

```bash
cargo build          # Build
cargo test           # Run all tests
cargo run -- --help  # CLI help
```

Tests use `tempfile` for isolated directories — no global state.

## License

Apache 2.0
