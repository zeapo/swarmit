# Swarmit — Multi-Agent Workflow Examples

## Pattern 1: Two agents dividing an epic

**Setup (human or coordinator agent):**
```bash
export SWARMIT_AGENT=coordinator

swarmit init --name "API Rebuild" --agent $SWARMIT_AGENT
swarmit epic create --title "Authentication" --priority high --agent $SWARMIT_AGENT
swarmit task create --title "OAuth2 login endpoint" --epic EPIC-001 --agent $SWARMIT_AGENT
swarmit task create --title "JWT token validation" --epic EPIC-001 --agent $SWARMIT_AGENT
swarmit task create --title "Refresh token rotation" --epic EPIC-001 --agent $SWARMIT_AGENT
swarmit link add --from TASK-003 --to TASK-002 --type blocked_by --agent $SWARMIT_AGENT
```

**Agent A (claude-auth-1):**
```bash
export SWARMIT_AGENT=claude-auth-1

# Find work
swarmit task list --status todo --json

# Claim TASK-001
swarmit task claim TASK-001 --agent $SWARMIT_AGENT

# ... implement ...

swarmit comment add TASK-001 --body "Implemented, tests pass" --agent $SWARMIT_AGENT
swarmit task done TASK-001 --agent $SWARMIT_AGENT
```

**Agent B (claude-auth-2) simultaneously:**
```bash
export SWARMIT_AGENT=claude-auth-2

swarmit task claim TASK-002 --agent $SWARMIT_AGENT
# ... implement JWT validation ...
swarmit task done TASK-002 --agent $SWARMIT_AGENT

# TASK-003 was blocked by TASK-002 — now safe to claim
swarmit task claim TASK-003 --agent $SWARMIT_AGENT
```

---

## Pattern 2: Coordinator + worker agents

**Coordinator creates the plan:**
```bash
swarmit epic create --title "Data Pipeline" --priority urgent --agent coordinator
for title in "Ingest raw events" "Validate schema" "Transform to parquet" "Load to warehouse"; do
  swarmit task create --title "$title" --epic EPIC-001 --agent coordinator
done
# Set up dependency chain
swarmit link add --from TASK-002 --to TASK-001 --type blocked_by --agent coordinator
swarmit link add --from TASK-003 --to TASK-002 --type blocked_by --agent coordinator
swarmit link add --from TASK-004 --to TASK-003 --type blocked_by --agent coordinator
```

**Workers just run this loop:**
```bash
export SWARMIT_AGENT=worker-$RANDOM

while true; do
  # Find an unclaimed, unblocked task
  TASK=$(swarmit task list --status todo --json | \
    jq -r '.data[] | select(.id) | .id' | head -1)

  [ -z "$TASK" ] && echo "No tasks available" && break

  swarmit task claim "$TASK" --agent $SWARMIT_AGENT
  # ... do the work ...
  swarmit task done "$TASK" --agent $SWARMIT_AGENT
done
```

---

## Pattern 3: Code review handoff

```bash
# Developer creates and completes implementation task
swarmit task create --title "Implement checkout flow" --agent dev-1
swarmit task claim TASK-001 --agent dev-1
# ... implement ...
swarmit task done TASK-001 --agent dev-1

# Create a review task that depends on implementation
swarmit task create --title "Review checkout implementation" --agent dev-1
swarmit link add --from TASK-002 --to TASK-001 --type blocked_by --agent dev-1

# Reviewer picks it up
swarmit task claim TASK-002 --agent reviewer-1
swarmit comment add TASK-002 --body "LGTM, minor nit on error handling in line 42" --agent reviewer-1
swarmit task done TASK-002 --agent reviewer-1
```

---

## Viewing progress in the TUI

While agents work, run the TUI in another terminal:
```bash
cd /path/to/project
swarmit   # No subcommand + TTY = launches TUI
```

The TUI auto-refreshes within ~200ms when agents update tasks.
Navigation: `j/k` move, `Enter` drill in, `Esc` back, `?` help, `q` quit.
