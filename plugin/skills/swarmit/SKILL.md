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

## Version Check

Before doing anything else, verify the installed swarmit version:

```bash
swarmit --version
```

Expected output: `swarmit 1.2.1` (or newer).

- **Command not found** → ask the user to install: `cargo install swarmit`
- **Version older than 1.2.1** → ask the user to update: `cargo install swarmit`
- **Version 1.2.1 or newer** → proceed silently

Do not continue with task management commands until the version check passes.

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

### 4. Record insights

After completing work, add structured insights for each file you changed:
```bash
swarmit insight add TASK-007 --file src/auth/oauth.rs \
  --before "fn login() { todo!() }" \
  --after "fn login() -> Result<Token> { ... }" \
  --body "Implemented OAuth login with token refresh" --agent me
```
Add one insight per file changed. `--before` and `--after` are optional.

### 5. Mark done
```bash
swarmit task done TASK-007 --agent me
```

### 6. Cancel tasks that are no longer relevant
If a task becomes unnecessary, cancel it with a reason instead of leaving it:
```bash
swarmit task cancel TASK-007 --reason "superseded by new approach" --agent me
```
Cancelled tasks are hidden from default listings. Use `--all` or `--status cancelled` to see them.

### 7. Pick the next task
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
| `swarmit task cancel TASK-007 --reason "..." --agent me` | Cancel task with reason |
| `swarmit task create --title "..." --description "..." --epic EPIC-001 --agent me` | Create task |
| `swarmit task list --all` | List all tasks including cancelled |
| `swarmit epic list --json` | List epics |
| `swarmit epic cancel EPIC-001 --reason "..." --agent me` | Cancel epic + cascade to tasks |
| `swarmit epic show EPIC-001` | Epic details |
| `swarmit link add --from TASK-001 --to TASK-002 --type blocks --agent me` | Add relationship |
| `swarmit comment add TASK-007 --body "..." --agent me` | Add comment |
| `swarmit insight add TASK-007 --file path --body "..." --agent me` | Add insight |
| `swarmit insight list TASK-007` | List insights on a task |
| `swarmit log --tail 20` | Recent operations |
| `swarmit compact --agent me` | Compact log |

---

## Creating Epics and Tasks from a Plan

### Check before you create

Before creating any epic or task, **always check whether it already exists**. A previous session (e.g. a planning session) may have already created the tracking items.

```bash
# Check existing epics first
swarmit epic list --json

# Check existing tasks in a specific epic
swarmit task list --epic EPIC-001 --json
```

If a matching epic/task already exists, **use it** — claim it and start working. Do not create a duplicate.

### Populate descriptions

When creating tasks from an implementation plan, **always populate `--description` with the full plan task body** — every step, file path, command, and expected output. The title alone is not enough; future agents claiming the task must not need to re-read the plan file.

```bash
swarmit task create \
  --title "Add OAuth login endpoint" \
  --description "Files:
- Create: src/auth/oauth.rs
- Modify: src/routes.rs:45-60

Step 1: Write failing test
  cargo test auth::oauth::test_login_redirect -- expected FAIL

Step 2: Implement minimal handler
  Add OAuthHandler struct with redirect() method

Step 3: Run tests - expect PASS
  cargo test auth::oauth

Step 4: Commit
  git add src/auth/oauth.rs src/routes.rs
  git commit -m 'feat: add OAuth login endpoint'" \
  --agent me
```

**Rule:** `--description` = complete plan task text, verbatim. Never summarize.

---

## Keeping Status Up to Date

Status updates are visible in the TUI and to all other agents — keep them accurate.

**When you start a task:**
```bash
swarmit task claim TASK-NNN --agent me
```

**When you finish a task:**
```bash
swarmit task done TASK-NNN --agent me
```

Never leave a task claimed but unfinished without a comment explaining why. Never mark a task done before its work is complete and verified.

---

## Rules for Agents

1. **Always use `--agent`** on every mutation command, or set `SWARMIT_AGENT=my-agent-id`.
2. **Always use `--json`** when parsing output programmatically.
3. **Claim before working** — check `task list --status todo`, claim one, then start.
4. **One task at a time** — claim only what you're actively working on.
5. **Comment on progress** — other agents (and humans) can see your notes in the TUI.
6. **Add insights when completing tasks** — one per file changed, documenting what and why.
7. **Check for blockers** — use `task show` to see if a task is blocked by others.
8. **Cancel stale tasks** — if a task is no longer relevant, cancel it with `task cancel --reason` rather than leaving it open. Cancelled tasks are hidden from default listings.

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
