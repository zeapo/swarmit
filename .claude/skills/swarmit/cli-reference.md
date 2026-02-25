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
| `cancelled` | Won't do |

Aliases: `wip` → `in_progress`, `complete` → `done`, `canceled` → `cancelled`

---

## Storage Layout

```
.swarmit/
  project.toml          # Project configuration
  operations.log        # Append-only JSONL event log
  operations.lock       # Exclusive write lock (fd-lock)
  operations.log.bak    # Backup after compaction
  state/
    epics/
      EPIC-001-auth/
        epic.md         # YAML frontmatter + markdown body
        TASK-001.md
        TASK-002.md
    backlog/
      TASK-010.md       # Tasks with no epic
```
