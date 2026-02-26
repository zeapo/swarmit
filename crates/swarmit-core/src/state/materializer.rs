use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::events::log::read_operations;
use crate::events::operations::{Operation, OperationKind};
use crate::models::{
    AgentId, Comment, Epic, ItemId, ProjectConfig, Relationship, Result,
    Status, SwarmitError, Task,
};

/// Full in-memory projection of all project state.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub config: Option<ProjectConfig>,
    pub epics: BTreeMap<ItemId, Epic>,
    pub tasks: BTreeMap<ItemId, Task>,
    pub relationships: Vec<Relationship>,
    pub comments: BTreeMap<ItemId, Vec<Comment>>,
    /// Next sequence number for epic IDs.
    pub epic_seq: u32,
    /// Next sequence number for task IDs.
    pub task_seq: u32,
    /// Total number of operations applied.
    pub sequence: u64,
}

impl ProjectState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds state by replaying all operations in the log.
    pub fn from_log(log_path: &Path) -> Result<Self> {
        let ops = read_operations(log_path)?;
        let mut state = Self::new();
        for op in ops {
            state.apply(op)?;
        }
        Ok(state)
    }

    /// Applies a single operation, mutating state in place.
    pub fn apply(&mut self, op: Operation) -> Result<()> {
        self.sequence += 1;
        match op.kind {
            OperationKind::InitProject {
                name,
                description,
                epic_prefix,
                task_prefix,
            } => {
                let mut config = ProjectConfig::new(name, op.agent);
                config.description = description;
                if let Some(p) = epic_prefix {
                    config.epic_prefix = p;
                }
                if let Some(p) = task_prefix {
                    config.task_prefix = p;
                }
                config.created_at = op.timestamp;
                self.config = Some(config);
            }

            OperationKind::UpdateProject { name, description, clear_description } => {
                let config = self.config.as_mut().ok_or_else(|| {
                    SwarmitError::NotInitialized("Cannot update project before init".into())
                })?;
                if let Some(n) = name {
                    config.name = n;
                }
                if clear_description {
                    config.description = None;
                } else if let Some(d) = description {
                    config.description = Some(d);
                }
            }

            OperationKind::CreateEpic {
                id,
                title,
                description,
                priority,
            } => {
                let mut epic = Epic::new(id.clone(), title, op.agent);
                epic.description = description;
                epic.priority = priority;
                epic.created_at = op.timestamp;
                epic.updated_at = op.timestamp;
                // Track sequence counter
                if let Some(n) = id.number() {
                    if n > self.epic_seq {
                        self.epic_seq = n;
                    }
                }
                self.epics.insert(id, epic);
            }

            OperationKind::UpdateEpic {
                id,
                title,
                description,
                priority,
                assignee,
            } => {
                let epic = self
                    .epics
                    .get_mut(&id)
                    .ok_or_else(|| SwarmitError::NotFound(id.clone()))?;
                if let Some(t) = title {
                    epic.title = t;
                }
                if let Some(d) = description {
                    epic.description = Some(d);
                }
                if let Some(p) = priority {
                    epic.priority = p;
                }
                if let Some(a) = assignee {
                    epic.assignee = Some(a);
                }
                epic.updated_at = op.timestamp;
            }

            OperationKind::UpdateEpicStatus { id, status } => {
                let epic = self
                    .epics
                    .get_mut(&id)
                    .ok_or_else(|| SwarmitError::NotFound(id.clone()))?;
                epic.status = status;
                epic.updated_at = op.timestamp;
            }

            OperationKind::DeleteEpic { id } => {
                self.epics.remove(&id);
            }

            OperationKind::CreateTask {
                id,
                title,
                description,
                priority,
                epic_id,
            } => {
                let mut task = Task::new(id.clone(), title, op.agent);
                task.description = description;
                task.priority = priority;
                task.epic_id = epic_id.clone();
                task.created_at = op.timestamp;
                task.updated_at = op.timestamp;
                // Track sequence counter
                if let Some(n) = id.number() {
                    if n > self.task_seq {
                        self.task_seq = n;
                    }
                }
                // Add task to epic's task list; re-open a Done epic
                if let Some(eid) = &epic_id {
                    if let Some(epic) = self.epics.get_mut(eid) {
                        if !epic.task_ids.contains(&id) {
                            epic.task_ids.push(id.clone());
                        }
                        if epic.status == Status::Done {
                            epic.status = Status::InProgress;
                            epic.updated_at = op.timestamp;
                        }
                    }
                }
                self.tasks.insert(id, task);
            }

            OperationKind::UpdateTask {
                id,
                title,
                description,
                priority,
                epic_id,
                assignee,
            } => {
                // Capture old epic before mutating, so we can update task_ids.
                let old_epic_id = if epic_id.is_some() {
                    self.tasks.get(&id).and_then(|t| t.epic_id.clone())
                } else {
                    None
                };
                let new_epic_id: Option<Option<ItemId>> = epic_id.clone();

                let task = self
                    .tasks
                    .get_mut(&id)
                    .ok_or_else(|| SwarmitError::NotFound(id.clone()))?;
                if let Some(t) = title {
                    task.title = t;
                }
                if let Some(d) = description {
                    task.description = Some(d);
                }
                if let Some(p) = priority {
                    task.priority = p;
                }
                if let Some(eid) = epic_id {
                    task.epic_id = eid;
                }
                if let Some(a) = assignee {
                    task.assignee = a;
                }
                task.updated_at = op.timestamp;

                // Keep epic.task_ids in sync when the task's epic changes.
                if let Some(new_eid_opt) = new_epic_id {
                    // Remove from old epic's task list.
                    if let Some(old_eid) = old_epic_id {
                        if let Some(epic) = self.epics.get_mut(&old_eid) {
                            epic.task_ids.retain(|tid| tid != &id);
                        }
                    }
                    // Add to new epic's task list.
                    if let Some(new_eid) = &new_eid_opt {
                        if let Some(epic) = self.epics.get_mut(new_eid) {
                            if !epic.task_ids.contains(&id) {
                                epic.task_ids.push(id.clone());
                            }
                            // Re-open a Done epic, same as CreateTask.
                            if epic.status == Status::Done {
                                epic.status = Status::InProgress;
                                epic.updated_at = op.timestamp;
                            }
                        }
                        self.check_epic_completion(new_eid, op.timestamp);
                    }
                }
            }

            OperationKind::UpdateTaskStatus { id, status } => {
                let task = self
                    .tasks
                    .get_mut(&id)
                    .ok_or_else(|| SwarmitError::NotFound(id.clone()))?;
                task.status = status;
                task.updated_at = op.timestamp;
            }

            OperationKind::ClaimTask { id } => {
                let task = self
                    .tasks
                    .get_mut(&id)
                    .ok_or_else(|| SwarmitError::NotFound(id.clone()))?;
                task.assignee = Some(op.agent);
                task.status = Status::InProgress;
                task.claimed_at = Some(op.timestamp);
                task.updated_at = op.timestamp;
            }

            OperationKind::CompleteTask { id } => {
                let epic_id = {
                    let task = self
                        .tasks
                        .get_mut(&id)
                        .ok_or_else(|| SwarmitError::NotFound(id.clone()))?;
                    task.status = Status::Done;
                    task.completed_at = Some(op.timestamp);
                    task.updated_at = op.timestamp;
                    task.epic_id.clone()
                };
                if let Some(eid) = epic_id {
                    self.check_epic_completion(&eid, op.timestamp);
                }
            }

            OperationKind::DeleteTask { id } => {
                let epic_id = if let Some(task) = self.tasks.remove(&id) {
                    // Remove from epic task list
                    if let Some(eid) = &task.epic_id {
                        if let Some(epic) = self.epics.get_mut(eid) {
                            epic.task_ids.retain(|tid| tid != &id);
                        }
                    }
                    task.epic_id.clone()
                } else {
                    None
                };
                // Remove all relationships involving this task
                self.relationships
                    .retain(|r| r.from != id && r.to != id);
                if let Some(eid) = epic_id {
                    self.check_epic_completion(&eid, op.timestamp);
                }
            }

            OperationKind::AddRelationship {
                from,
                to,
                rel_type,
            } => {
                let rel = Relationship::new(from, to, rel_type);
                if !self.relationships.contains(&rel) {
                    self.relationships.push(rel);
                }
            }

            OperationKind::RemoveRelationship {
                from,
                to,
                rel_type,
            } => {
                self.relationships
                    .retain(|r| !(r.from == from && r.to == to && r.rel_type == rel_type));
            }

            OperationKind::AddComment { id, task_id, body } => {
                let comment = Comment {
                    id,
                    task_id: task_id.clone(),
                    author: op.agent,
                    body,
                    created_at: op.timestamp,
                };
                self.comments
                    .entry(task_id)
                    .or_default()
                    .push(comment);
            }

        }

        Ok(())
    }

    /// Auto-transitions an epic to Done if it has tasks and all of them are Done.
    fn check_epic_completion(&mut self, epic_id: &ItemId, timestamp: chrono::DateTime<chrono::Utc>) {
        let Some(epic) = self.epics.get(epic_id) else {
            return;
        };
        if epic.task_ids.is_empty() {
            return;
        }
        let all_done = epic
            .task_ids
            .iter()
            .all(|tid| self.tasks.get(tid).map_or(false, |t| t.status == Status::Done));
        if all_done {
            if let Some(epic) = self.epics.get_mut(epic_id) {
                epic.status = Status::Done;
                epic.updated_at = timestamp;
            }
        }
    }

    /// Returns all relationships involving a given item (from or to).
    pub fn relationships_for(&self, id: &ItemId) -> Vec<&Relationship> {
        self.relationships
            .iter()
            .filter(|r| &r.from == id || &r.to == id)
            .collect()
    }

    /// Returns comments for a task, sorted by UUID (time order).
    pub fn comments_for(&self, task_id: &ItemId) -> Vec<&Comment> {
        self.comments
            .get(task_id)
            .map(|cs| cs.iter().collect())
            .unwrap_or_default()
    }

    /// Returns tasks with a given status.
    pub fn tasks_by_status(&self, status: Status) -> Vec<&Task> {
        self.tasks.values().filter(|t| t.status == status).collect()
    }

    /// Returns tasks assigned to a given agent.
    pub fn tasks_by_assignee(&self, agent: &AgentId) -> Vec<&Task> {
        self.tasks
            .values()
            .filter(|t| t.assignee.as_ref() == Some(agent))
            .collect()
    }

    /// Returns tasks belonging to an epic.
    pub fn tasks_for_epic(&self, epic_id: &ItemId) -> Vec<&Task> {
        self.tasks
            .values()
            .filter(|t| t.epic_id.as_ref() == Some(epic_id))
            .collect()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::operations::OperationKind;
    use crate::models::Priority;

    fn agent() -> AgentId {
        AgentId::new("test-agent").unwrap()
    }

    fn op(kind: OperationKind) -> Operation {
        Operation::new(agent(), kind)
    }

    #[test]
    fn init_project() {
        let mut state = ProjectState::new();
        state
            .apply(op(OperationKind::InitProject {
                name: "Test Project".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
            }))
            .unwrap();
        assert_eq!(state.config.as_ref().unwrap().name, "Test Project");
    }

    #[test]
    fn create_epic() {
        let mut state = ProjectState::new();
        state
            .apply(op(OperationKind::InitProject {
                name: "Test".to_string(),
                description: None,
                epic_prefix: None,
                task_prefix: None,
            }))
            .unwrap();

        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Auth Epic".to_string(),
                description: None,
                priority: Priority::High,
            }))
            .unwrap();

        assert!(state.epics.contains_key(&epic_id));
        assert_eq!(state.epics[&epic_id].title, "Auth Epic");
        assert_eq!(state.epics[&epic_id].priority, Priority::High);
    }

    #[test]
    fn create_task_adds_to_epic() {
        let mut state = ProjectState::new();
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }))
            .unwrap();

        state
            .apply(op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }))
            .unwrap();

        assert!(state.tasks.contains_key(&task_id));
        assert!(state.epics[&epic_id].task_ids.contains(&task_id));
    }

    #[test]
    fn claim_task() {
        let mut state = ProjectState::new();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        state
            .apply(op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }))
            .unwrap();

        state
            .apply(op(OperationKind::ClaimTask {
                id: task_id.clone(),
            }))
            .unwrap();

        let task = &state.tasks[&task_id];
        assert_eq!(task.status, Status::InProgress);
        assert_eq!(task.assignee, Some(agent()));
    }

    #[test]
    fn complete_task() {
        let mut state = ProjectState::new();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        state
            .apply(op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }))
            .unwrap();

        state
            .apply(op(OperationKind::CompleteTask {
                id: task_id.clone(),
            }))
            .unwrap();

        assert_eq!(state.tasks[&task_id].status, Status::Done);
        assert!(state.tasks[&task_id].completed_at.is_some());
    }

    #[test]
    fn add_relationship() {
        let mut state = ProjectState::new();
        let t1: ItemId = "TASK-001".parse().unwrap();
        let t2: ItemId = "TASK-002".parse().unwrap();

        state
            .apply(op(OperationKind::AddRelationship {
                from: t1.clone(),
                to: t2.clone(),
                rel_type: crate::models::RelationType::Blocks,
            }))
            .unwrap();

        assert_eq!(state.relationships.len(), 1);
        assert_eq!(state.relationships_for(&t1).len(), 1);
    }

    #[test]
    fn add_comment() {
        let mut state = ProjectState::new();
        let task_id: ItemId = "TASK-001".parse().unwrap();
        let comment_id = uuid::Uuid::now_v7();

        state
            .apply(op(OperationKind::AddComment {
                id: comment_id,
                task_id: task_id.clone(),
                body: "Great work!".to_string(),
            }))
            .unwrap();

        assert_eq!(state.comments_for(&task_id).len(), 1);
        assert_eq!(state.comments_for(&task_id)[0].body, "Great work!");
    }

    // ── Auto epic status transition tests ────────────────────────────────

    fn make_epic_with_tasks(state: &mut ProjectState, task_ids: &[&str]) -> (ItemId, Vec<ItemId>) {
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }))
            .unwrap();
        let mut ids = Vec::new();
        for raw in task_ids {
            let tid: ItemId = raw.parse().unwrap();
            state
                .apply(op(OperationKind::CreateTask {
                    id: tid.clone(),
                    title: "Task".to_string(),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: Some(epic_id.clone()),
                }))
                .unwrap();
            ids.push(tid);
        }
        (epic_id, ids)
    }

    #[test]
    fn complete_all_tasks_auto_closes_epic() {
        let mut state = ProjectState::new();
        let (epic_id, task_ids) =
            make_epic_with_tasks(&mut state, &["TASK-001", "TASK-002"]);

        for tid in &task_ids {
            state
                .apply(op(OperationKind::CompleteTask { id: tid.clone() }))
                .unwrap();
        }

        assert_eq!(state.epics[&epic_id].status, Status::Done);
    }

    #[test]
    fn partial_completion_does_not_close_epic() {
        let mut state = ProjectState::new();
        let (epic_id, task_ids) =
            make_epic_with_tasks(&mut state, &["TASK-001", "TASK-002"]);

        // Complete only the first task
        state
            .apply(op(OperationKind::CompleteTask {
                id: task_ids[0].clone(),
            }))
            .unwrap();

        assert_ne!(state.epics[&epic_id].status, Status::Done);
    }

    #[test]
    fn delete_last_non_done_task_closes_epic() {
        let mut state = ProjectState::new();
        let (epic_id, task_ids) =
            make_epic_with_tasks(&mut state, &["TASK-001", "TASK-002"]);

        // Complete the first task, then delete the second
        state
            .apply(op(OperationKind::CompleteTask {
                id: task_ids[0].clone(),
            }))
            .unwrap();
        state
            .apply(op(OperationKind::DeleteTask {
                id: task_ids[1].clone(),
            }))
            .unwrap();

        assert_eq!(state.epics[&epic_id].status, Status::Done);
    }

    #[test]
    fn add_task_to_done_epic_reopens_it() {
        let mut state = ProjectState::new();
        let (epic_id, task_ids) =
            make_epic_with_tasks(&mut state, &["TASK-001"]);

        state
            .apply(op(OperationKind::CompleteTask {
                id: task_ids[0].clone(),
            }))
            .unwrap();
        assert_eq!(state.epics[&epic_id].status, Status::Done);

        let new_task: ItemId = "TASK-002".parse().unwrap();
        state
            .apply(op(OperationKind::CreateTask {
                id: new_task,
                title: "New task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }))
            .unwrap();

        assert_eq!(state.epics[&epic_id].status, Status::InProgress);
    }

    #[test]
    fn add_task_to_non_done_epic_leaves_status_unchanged() {
        let mut state = ProjectState::new();
        let (epic_id, _) = make_epic_with_tasks(&mut state, &["TASK-001"]);

        let new_task: ItemId = "TASK-002".parse().unwrap();
        state
            .apply(op(OperationKind::CreateTask {
                id: new_task,
                title: "New task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }))
            .unwrap();

        assert_eq!(state.epics[&epic_id].status, Status::Todo);
    }

    #[test]
    fn update_task_epic_syncs_task_ids_and_auto_closes() {
        // Tasks created without an epic, then retroactively assigned to one that
        // should auto-close because all tasks are already Done.
        let mut state = ProjectState::new();
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }))
            .unwrap();

        let task_id: ItemId = "TASK-001".parse().unwrap();
        state
            .apply(op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }))
            .unwrap();
        state
            .apply(op(OperationKind::CompleteTask { id: task_id.clone() }))
            .unwrap();

        // Assign the already-Done task to the epic via UpdateTask.
        state
            .apply(op(OperationKind::UpdateTask {
                id: task_id.clone(),
                title: None,
                description: None,
                priority: None,
                epic_id: Some(Some(epic_id.clone())),
                assignee: None,
            }))
            .unwrap();

        assert!(state.epics[&epic_id].task_ids.contains(&task_id));
        assert_eq!(state.epics[&epic_id].status, Status::Done);
    }

    #[test]
    fn empty_epic_stays_unchanged_on_complete() {
        let mut state = ProjectState::new();
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Empty Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }))
            .unwrap();

        // Create and complete a standalone task (no epic)
        let task_id: ItemId = "TASK-001".parse().unwrap();
        state
            .apply(op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Standalone".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            }))
            .unwrap();
        state
            .apply(op(OperationKind::CompleteTask {
                id: task_id,
            }))
            .unwrap();

        assert_eq!(state.epics[&epic_id].status, Status::Todo);
    }

    #[test]
    fn delete_task_removes_from_epic() {
        let mut state = ProjectState::new();
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-001".parse().unwrap();

        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }))
            .unwrap();
        state
            .apply(op(OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            }))
            .unwrap();
        state
            .apply(op(OperationKind::DeleteTask {
                id: task_id.clone(),
            }))
            .unwrap();

        assert!(!state.tasks.contains_key(&task_id));
        assert!(!state.epics[&epic_id].task_ids.contains(&task_id));
    }

    // ── UpdateProject tests ───────────────────────────────────────────────

    fn init_op() -> Operation {
        op(OperationKind::InitProject {
            name: "Original Name".to_string(),
            description: None,
            epic_prefix: None,
            task_prefix: None,
        })
    }

    #[test]
    fn update_project_name() {
        let mut state = ProjectState::new();
        state.apply(init_op()).unwrap();
        assert_eq!(state.config.as_ref().unwrap().name, "Original Name");

        state
            .apply(op(OperationKind::UpdateProject {
                name: Some("New Name".to_string()),
                description: None,
                clear_description: false,
            }))
            .unwrap();

        assert_eq!(state.config.as_ref().unwrap().name, "New Name");
    }

    #[test]
    fn update_project_description() {
        let mut state = ProjectState::new();
        state.apply(init_op()).unwrap();
        assert!(state.config.as_ref().unwrap().description.is_none());

        // Set description
        state
            .apply(op(OperationKind::UpdateProject {
                name: None,
                description: Some("A description".to_string()),
                clear_description: false,
            }))
            .unwrap();
        assert_eq!(
            state.config.as_ref().unwrap().description,
            Some("A description".to_string())
        );

        // Clear description with clear_description: true
        state
            .apply(op(OperationKind::UpdateProject {
                name: None,
                description: None,
                clear_description: true,
            }))
            .unwrap();
        assert!(state.config.as_ref().unwrap().description.is_none());
    }

    #[test]
    fn update_project_before_init_fails() {
        let mut state = ProjectState::new();
        let result = state.apply(op(OperationKind::UpdateProject {
            name: Some("Name".to_string()),
            description: None,
            clear_description: false,
        }));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SwarmitError::NotInitialized(_)));
    }

    #[test]
    fn duplicate_create_task_does_not_add_id_twice_to_epic() {
        // Regression test: two concurrent agents can race on task_seq and emit
        // two CreateTask operations with the same ID. Replaying the log must not
        // result in the task ID appearing more than once in epic.task_ids.
        let mut state = ProjectState::new();
        let epic_id: ItemId = "EPIC-001".parse().unwrap();
        let task_id: ItemId = "TASK-058".parse().unwrap();

        state
            .apply(op(OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            }))
            .unwrap();

        // Apply the same CreateTask operation twice (simulating a TOCTOU race).
        for _ in 0..2 {
            state
                .apply(op(OperationKind::CreateTask {
                    id: task_id.clone(),
                    title: "Duplicate Task".to_string(),
                    description: None,
                    priority: Priority::Medium,
                    epic_id: Some(epic_id.clone()),
                }))
                .unwrap();
        }

        let task_ids = &state.epics[&epic_id].task_ids;
        assert_eq!(
            task_ids.iter().filter(|id| *id == &task_id).count(),
            1,
            "task_ids should contain TASK-058 exactly once, got: {:?}",
            task_ids
        );
    }
}
