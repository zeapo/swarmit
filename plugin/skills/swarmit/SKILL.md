---
name: swarmit
description: >
  Use this skill when working on tasks tracked in a swarmit project. Triggers on:
  "pick up a task", "what should I work on", "claim task", "mark done",
  "create task", "task list", "swarmit", task management in a multi-agent workflow.
---

# Swarmit Agent Workflow

Swarmit is a local-first project management tool for multi-agent Claude Code workflows.
Tasks are tracked in `.swarmit/` using an append-only event log. Multiple agents can
work concurrently — all writes are lock-protected.

## Core Workflow

### 1. Find work
```bash
swarmit task list --status todo --json --agent me
```

### 2. Claim a task (do this BEFORE starting work)
```bash
swarmit task claim TASK-007 --agent me
```
Claiming sets status → In Progress and records your agent ID as assignee.
**Never work on a task you haven't claimed — another agent may be doing it.**

### 3. Work on the task

Implement the task. Use `swarmit comment add` to record progress notes:
```bash
swarmit comment add TASK-007 --body "Implemented OAuth flow, tests passing" --agent me
```

### 4. Mark done
```bash
swarmit task done TASK-007 --agent me
```

### 5. Pick the next task
Loop back to step 1.

---

## Quick Reference

| Command | Description |
|---------|-------------|
| `swarmit init --name "Project" --agent me` | Initialize project |
| `swarmit task list --json` | List all tasks |
| `swarmit task list --status todo --json` | List unstarted tasks |
| `swarmit task list --epic EPIC-001 --json` | Tasks in an epic |
| `swarmit task show TASK-007` | Full task detail with relationships & comments |
| `swarmit task claim TASK-007 --agent me` | Claim a task |
| `swarmit task done TASK-007 --agent me` | Mark task complete |
| `swarmit task create --title "..." --epic EPIC-001 --agent me` | Create task |
| `swarmit epic list --json` | List epics |
| `swarmit epic show EPIC-001` | Epic details |
| `swarmit link add --from TASK-001 --to TASK-002 --type blocks --agent me` | Add relationship |
| `swarmit comment add TASK-007 --body "..." --agent me` | Add comment |
| `swarmit log --tail 20` | Recent operations |
| `swarmit compact --agent me` | Compact log |

---

## Rules for Agents

1. **Always use `--agent`** on every mutation command, or set `SWARMIT_AGENT=my-agent-id`.
2. **Always use `--json`** when parsing output programmatically.
3. **Claim before working** — check `task list --status todo`, claim one, then start.
4. **One task at a time** — claim only what you're actively working on.
5. **Comment on progress** — other agents (and humans) can see your notes in the TUI.
6. **Check for blockers** — use `task show` to see if a task is blocked by others.

---

## Output Format

All `--json` responses use this envelope:
```json
{ "ok": true, "data": { ... } }
{ "ok": false, "error": "message" }
```

---

## Agent Identity

Recommended: use a descriptive, stable agent ID that identifies your role:
```bash
export SWARMIT_AGENT="claude-backend-1"
export SWARMIT_AGENT="claude-frontend-2"
export SWARMIT_AGENT="claude-reviewer"
```

---

## Discovering What to Work On

Full decision flow:
```bash
# 1. See the big picture
swarmit epic list --json

# 2. Find unclaimed tasks in a specific epic
swarmit task list --epic EPIC-001 --status todo --json

# 3. Check if a task is blocked
swarmit task show TASK-007 --json | jq '.data.relationships'

# 4. Claim and work
swarmit task claim TASK-007 --agent $SWARMIT_AGENT
```
