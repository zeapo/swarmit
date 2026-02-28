use std::collections::{BTreeMap, HashMap};

use crate::models::{AgentId, ItemId, Status, Task};

/// In-memory lookup indexes for fast queries over project state.
/// Built from `ProjectState` on demand.
pub struct StateIndex<'a> {
    by_status: HashMap<Status, Vec<&'a Task>>,
    by_assignee: HashMap<AgentId, Vec<&'a Task>>,
    by_epic: HashMap<ItemId, Vec<&'a Task>>,
}

impl<'a> StateIndex<'a> {
    pub fn build(tasks: &'a BTreeMap<ItemId, Task>) -> Self {
        let mut by_status: HashMap<Status, Vec<&Task>> = HashMap::new();
        let mut by_assignee: HashMap<AgentId, Vec<&Task>> = HashMap::new();
        let mut by_epic: HashMap<ItemId, Vec<&Task>> = HashMap::new();

        for task in tasks.values() {
            by_status.entry(task.status).or_default().push(task);
            if let Some(a) = &task.assignee {
                by_assignee.entry(a.clone()).or_default().push(task);
            }
            if let Some(eid) = &task.epic_id {
                by_epic.entry(eid.clone()).or_default().push(task);
            }
        }

        StateIndex {
            by_status,
            by_assignee,
            by_epic,
        }
    }

    pub fn by_status(&self, status: Status) -> &[&'a Task] {
        self.by_status.get(&status).map_or(&[], |v| v.as_slice())
    }

    pub fn by_assignee(&self, agent: &AgentId) -> &[&'a Task] {
        self.by_assignee.get(agent).map_or(&[], |v| v.as_slice())
    }

    pub fn by_epic(&self, epic_id: &ItemId) -> &[&'a Task] {
        self.by_epic.get(epic_id).map_or(&[], |v| v.as_slice())
    }
}
