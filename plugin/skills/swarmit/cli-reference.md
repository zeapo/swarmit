# Swarmit CLI Reference

## Global Flags

| Flag | Env Var | Description |
|------|---------|-------------|
| `--agent <ID>` | `SWARMIT_AGENT` | Agent identifier (required for mutations) |
| `--json` | — | Force JSON output |
| `--plain` | — | Force plain text output |
| `--dir <PATH>` | — | Project root (default: walk up to find `.swarmit/`) |

Auto-detection: if stdout is a TTY → pretty output; if piped → JSON.

---

## swarmit init

```
swarmit init --name <NAME> [--agent <ID>] [--description <DESC>]
             [--epic-prefix <PREFIX>] [--task-prefix <PREFIX>]
```

Creates `.swarmit/` directory structure and writes the first operation.

---

## swarmit epic

```
swarmit epic create --title <TITLE> [--priority low|medium|high|urgent]
                    [--description <DESC>] --agent <ID>

swarmit epic list [--status todo|in_progress|done|blocked|cancelled]

swarmit epic show <EPIC-ID>

swarmit epic update <EPIC-ID> [--title <T>] [--description <D>]
                   [--priority <P>] [--assignee <AGENT>] [--status <S>]
                   --agent <ID>

swarmit epic delete <EPIC-ID> --agent <ID>

swarmit epic cancel <EPIC-ID> --reason <TEXT> --agent <ID>
  # Sets epic → Cancelled, cascades to all non-terminal tasks

swarmit epic list [--all]
  # --all includes cancelled epics (hidden by default)
```

---

## swarmit task

```
swarmit task create --title <TITLE> [--epic <EPIC-ID>]
                    [--priority low|medium|high|urgent]
                    [--description <DESC>] --agent <ID>

swarmit task list [--status <S>] [--epic <EPIC-ID>] [--assignee <AGENT>]

swarmit task show <TASK-ID>
  # Shows: metadata, description, relationships, comments

swarmit task update <TASK-ID> [--title <T>] [--description <D>]
                   [--priority <P>] [--epic <EPIC-ID>] [--status <S>]
                   [--assignee <AGENT>] --agent <ID>

swarmit task delete <TASK-ID> --agent <ID>

swarmit task claim <TASK-ID> --agent <ID>
  # Sets status → In Progress, assigns to agent

swarmit task done <TASK-ID> --agent <ID>
  # Sets status → Done, records completion timestamp

swarmit task cancel <TASK-ID> --reason <TEXT> --agent <ID>
  # Sets status → Cancelled, auto-adds reason as comment

swarmit task list [--all]
  # --all includes cancelled tasks (hidden by default)
```

---

## swarmit link

```
swarmit link add --from <ID> --to <ID>
                 --type blocks|blocked_by|parent|child|relates_to|duplicates|duplicated_by
                 --agent <ID>
  # Automatically adds the inverse relationship too

swarmit link remove --from <ID> --to <ID> --type <TYPE> --agent <ID>

swarmit link list <ITEM-ID>
```

---

## swarmit comment

```
swarmit comment add <TASK-ID> --body <TEXT> --agent <ID>

swarmit comment list <TASK-ID>
```

---

## swarmit insight

```
swarmit insight add <TASK-ID> --file <PATH> --body <TEXT>
                    [--before <SNIPPET>] [--after <SNIPPET>]
                    --agent <ID>

swarmit insight list <TASK-ID>
```

Structured code-change records: one insight per file changed. `--before` and `--after` are optional snippets showing the code before/after the change. `--body` is the reasoning.

**Examples:**
```bash
# Full insight with before/after
swarmit insight add TASK-007 --file src/auth.rs \
  --before "fn login() { todo!() }" \
  --after "fn login() -> Result<Token> { ... }" \
  --body "Implemented OAuth login" --agent me

# Insight for a new file (no --before)
swarmit insight add TASK-007 --file src/auth/oauth.rs \
  --after "pub struct OAuthClient { ... }" \
  --body "Added new OAuth client module" --agent me

# Minimal insight (reasoning only)
swarmit insight add TASK-007 --file Cargo.toml \
  --body "Added oauth2 dependency" --agent me
```

---

## swarmit log

```
swarmit log [--tail N] [--agent <AGENT-ID>] [--since <RFC3339>]
```

Shows the N most recent operations, optionally filtered by agent.

---

## swarmit compact

```
swarmit compact --agent <ID>
```

Rotates the operations log:
1. Reads all operations and builds current state
2. Backs up `operations.log` → `operations.log.bak`
3. Writes a new `operations.log` with just a `Snapshot` marker

---

## JSON Output Examples

**Task list:**
```json
{
  "ok": true,
  "data": [
    {
      "id": "TASK-001",
      "title": "Implement OAuth",
      "status": "In Progress",
      "priority": "High",
      "epic_id": "EPIC-001",
      "assignee": "claude-auth-1"
    }
  ]
}
```

**Task show:**
```json
{
  "ok": true,
  "data": {
    "id": "TASK-001",
    "title": "Implement OAuth",
    "status": "In Progress",
    "relationships": [
      { "from": "TASK-001", "to": "TASK-002", "type": "blocks" }
    ],
    "comments": [
      { "author": "claude-auth-1", "body": "WIP", "created_at": "..." }
    ]
  }
}
```

**Error:**
```json
{ "ok": false, "error": "Task not found: TASK-999" }
```

---

## Status Values

| Value | Description |
|-------|-------------|
| `todo` | Not started |
| `in_progress` | Being worked on |
| `done` | Completed |
| `blocked` | Waiting on something |
| `cancelled` | Won't do (hidden from default listings, use `--all` to see) |

Aliases: `wip` → `in_progress`, `complete` → `done`, `canceled` → `cancelled`

---

## Storage Layout

```
.swarmit/
  state.db              # Single SQLite database (WAL mode)
                        #   operations table (event log)
                        #   materialized state tables (epics, tasks, etc.)
  state/                # Optional materialized markdown (if auto_materialize enabled)
    epics/
      EPIC-001-auth/
        epic.md         # YAML frontmatter + markdown body
        TASK-001.md
        TASK-002.md
    backlog/
      TASK-010.md       # Tasks with no epic
```
