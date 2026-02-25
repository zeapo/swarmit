use swarmit_core::models::ItemId;

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Board { epic_id: ItemId },
    TaskDetail { task_id: ItemId },
    Activity,
    Help,
}

/// User input actions, decoupled from raw key events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Select,
    Back,
    Quit,
    Help,
    Search,
    ClaimTask,
    ChangeStatus,
    GotoDashboard,
    GotoBacklog,
    GotoActivity,
    Refresh,
    None,
}
