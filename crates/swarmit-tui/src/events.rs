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
    QuitRequest,
    NewTask,
    Help,
    Search,
    ClaimTask,
    ChangeStatus,
    GotoDashboard,
    GotoActivity,
    Refresh,
    ToggleCollapse,
    OpenFilterDialog,
    FilterDialogMove(i8),
    FilterDialogConfirm,
    FilterDialogCancel,
    None,
}

/// Which form field has focus in the task-creation modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFormField {
    Title,
    Description,
    Epic,
    Priority,
}

/// Active modal overlay (mutually exclusive with normal screen interaction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    QuitConfirm,
    TaskCreate {
        title: String,
        cursor_pos: usize,
        description: Vec<String>,
        desc_row: usize,
        desc_col: usize,
        epic_index: usize,
        priority_index: usize,
        focused_field: TaskFormField,
        error: Option<String>,
    },
    FilterSelect { selected_index: usize },
}
