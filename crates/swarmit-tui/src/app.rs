use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use swarmit_core::events::locking::try_append_with_timeout;
use swarmit_core::events::log::{append_operation, read_operations_since};
use swarmit_core::events::operations::{Operation, OperationKind};
use swarmit_core::models::{AgentId, ItemId, Priority, Status};
use swarmit_core::state::ProjectState;

use crate::components::dashboard::DashboardRow;
use crate::events::{Action, Modal, Screen, TaskFormField};
use crate::theme::Theme;

/// The ordered list of filter options shown in the FilterSelect dialog.
pub const FILTER_OPTIONS: &[Option<Status>] = &[
    None,
    Some(Status::Todo),
    Some(Status::InProgress),
    Some(Status::Blocked),
    Some(Status::Done),
    Some(Status::Cancelled),
];

/// Central application state.
pub struct App {
    pub state: ProjectState,
    pub screen: Screen,
    pub project_root: PathBuf,
    pub theme: Theme,

    // Navigation: index of selected item in the current list.
    pub selected_index: usize,

    // File watcher for live refresh.
    watcher_rx: Option<Receiver<DebounceEventResult>>,
    // Suppress the Drop warning — the watcher must be kept alive.
    _debouncer: Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>,

    // Byte offset for incremental log reads.
    pub log_offset: u64,

    pub log_path: PathBuf,
    pub should_quit: bool,

    // Search/filter string (future).
    pub search_query: String,

    // Active modal overlay (None = no modal).
    pub modal: Option<Modal>,

    // Epics that are currently collapsed in the dashboard tree.
    pub collapsed_epics: HashSet<ItemId>,

    /// Status filter for the dashboard (None = show all).
    pub dashboard_filter: Option<Status>,

    // Cached flattened tree rows for the dashboard (rebuilt on state changes).
    pub dashboard_rows: Vec<DashboardRow>,
}

impl App {
    pub fn new(project_root: PathBuf, theme: Theme) -> Result<Self> {
        let log_path = project_root.join(".swarmit").join("operations.log");
        let state = ProjectState::from_log(&log_path)?;

        // Seed the byte offset so incremental reads start from end of file.
        let log_offset = if log_path.exists() {
            std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        let (tx, rx) = mpsc::channel();

        let mut debouncer = new_debouncer(Duration::from_millis(200), tx)?;

        if log_path.exists() {
            use notify::RecursiveMode;
            debouncer
                .watcher()
                .watch(&log_path, RecursiveMode::NonRecursive)?;
        }

        let mut app = App {
            state,
            screen: Screen::Dashboard,
            project_root,
            theme,
            selected_index: 0,
            watcher_rx: Some(rx),
            _debouncer: Some(debouncer),
            log_offset,
            log_path,
            should_quit: false,
            search_query: String::new(),
            modal: None,
            collapsed_epics: HashSet::new(),
            dashboard_filter: None,
            dashboard_rows: Vec::new(),
        };
        app.rebuild_dashboard_rows();
        Ok(app)
    }

    /// Process an action and update state accordingly.
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::QuitRequest => {
                self.modal = Some(Modal::QuitConfirm);
            }
            Action::NewTask => {
                self.modal = Some(Modal::TaskCreate {
                    title: String::new(),
                    cursor_pos: 0,
                    description: vec![String::new()],
                    desc_row: 0,
                    desc_col: 0,
                    epic_index: 0,
                    priority_index: 1, // default Medium
                    focused_field: TaskFormField::Title,
                    error: None,
                });
            }
            Action::Back => self.go_back(),
            Action::Up => self.move_up(),
            Action::Down => self.move_down(),
            Action::Select => self.select_item(),
            Action::GotoDashboard => {
                self.screen = Screen::Dashboard;
                self.selected_index = 0;
                self.rebuild_dashboard_rows();
            }
            Action::GotoActivity => {
                self.screen = Screen::Activity;
                self.selected_index = 0;
            }
            Action::Help => {
                self.screen = Screen::Help;
            }
            Action::Refresh => {
                self.poll_log_changes();
            }
            Action::ToggleCollapse => {
                if matches!(self.screen, Screen::Dashboard) {
                    // Find the epic at the current selection position.
                    if let Some(row) = self.dashboard_rows.get(self.selected_index).cloned() {
                        let epic_id = match row {
                            DashboardRow::Epic { id } => Some(id),
                            DashboardRow::Task { id } => {
                                // Find the epic that owns this task.
                                self.state
                                    .tasks
                                    .get(&id)
                                    .and_then(|t| t.epic_id.clone())
                            }
                        };
                        if let Some(eid) = epic_id {
                            if self.collapsed_epics.contains(&eid) {
                                self.collapsed_epics.remove(&eid);
                            } else {
                                self.collapsed_epics.insert(eid);
                            }
                            self.rebuild_dashboard_rows();
                        }
                    }
                }
            }
            action => self.apply_action(action),
        }
    }

    /// Apply filter-dialog actions (also called from `handle_filter_select_key`).
    fn apply_action(&mut self, action: Action) {
        match action {
            Action::OpenFilterDialog => {
                let selected_index = FILTER_OPTIONS
                    .iter()
                    .position(|opt| *opt == self.dashboard_filter)
                    .unwrap_or(0);
                self.modal = Some(Modal::FilterSelect { selected_index });
            }
            Action::FilterDialogMove(delta) => {
                if let Some(Modal::FilterSelect { selected_index }) = &mut self.modal {
                    let len = FILTER_OPTIONS.len();
                    *selected_index = ((*selected_index as isize + delta as isize)
                        .rem_euclid(len as isize)) as usize;
                }
            }
            Action::FilterDialogConfirm => {
                if let Some(Modal::FilterSelect { selected_index }) = &self.modal {
                    self.dashboard_filter = FILTER_OPTIONS[*selected_index];
                    self.rebuild_dashboard_rows();
                }
                self.modal = None;
            }
            Action::FilterDialogCancel => {
                self.modal = None;
            }
            _ => {}
        }
    }

    /// Route key events to the active modal handler.
    pub fn handle_modal_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match &self.modal {
            Some(Modal::QuitConfirm) => self.handle_quit_confirm_key(code),
            Some(Modal::TaskCreate { .. }) => self.handle_task_form_key(code, modifiers),
            Some(Modal::FilterSelect { .. }) => self.handle_filter_select_key(code),
            None => {}
        }
    }

    fn handle_quit_confirm_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.should_quit = true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.modal = None;
            }
            _ => {}
        }
    }

    fn handle_filter_select_key(&mut self, code: KeyCode) {
        let action = match code {
            KeyCode::Char('j') | KeyCode::Down => Action::FilterDialogMove(1),
            KeyCode::Char('k') | KeyCode::Up => Action::FilterDialogMove(-1),
            KeyCode::Enter => Action::FilterDialogConfirm,
            KeyCode::Esc | KeyCode::Char('q') => Action::FilterDialogCancel,
            _ => return,
        };
        self.apply_action(action);
    }

    fn handle_task_form_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let Some(Modal::TaskCreate {
            ref mut title,
            ref mut cursor_pos,
            ref mut description,
            ref mut desc_row,
            ref mut desc_col,
            ref mut epic_index,
            ref mut priority_index,
            ref mut focused_field,
            ref mut error,
        }) = self.modal
        else {
            return;
        };

        // Clear previous error on any keypress
        *error = None;

        let epic_count = self.state.epics.len() + 1; // +1 for "(none)"

        match focused_field {
            TaskFormField::Title => match code {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    title.insert(*cursor_pos, c);
                    *cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if *cursor_pos > 0 {
                        *cursor_pos -= 1;
                        title.remove(*cursor_pos);
                    }
                }
                KeyCode::Left => {
                    *cursor_pos = cursor_pos.saturating_sub(1);
                }
                KeyCode::Right => {
                    if *cursor_pos < title.len() {
                        *cursor_pos += 1;
                    }
                }
                KeyCode::Tab | KeyCode::Down => {
                    *focused_field = TaskFormField::Description;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    *focused_field = TaskFormField::Priority;
                }
                KeyCode::Enter => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Esc => {
                    self.modal = None;
                    return;
                }
                _ => {}
            },
            TaskFormField::Description => match code {
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    description[*desc_row].insert(*desc_col, c);
                    *desc_col += c.len_utf8();
                }
                KeyCode::Enter => {
                    let rest = description[*desc_row][*desc_col..].to_string();
                    description[*desc_row].truncate(*desc_col);
                    *desc_row += 1;
                    description.insert(*desc_row, rest);
                    *desc_col = 0;
                }
                KeyCode::Backspace => {
                    if *desc_col > 0 {
                        let prev = description[*desc_row][..*desc_col]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        description[*desc_row].drain(prev..*desc_col);
                        *desc_col = prev;
                    } else if *desc_row > 0 {
                        let current = description.remove(*desc_row);
                        *desc_row -= 1;
                        *desc_col = description[*desc_row].len();
                        description[*desc_row].push_str(&current);
                    }
                }
                KeyCode::Left => {
                    if *desc_col > 0 {
                        let prev = description[*desc_row][..*desc_col]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        *desc_col = prev;
                    } else if *desc_row > 0 {
                        *desc_row -= 1;
                        *desc_col = description[*desc_row].len();
                    }
                }
                KeyCode::Right => {
                    let line_len = description[*desc_row].len();
                    if *desc_col < line_len {
                        let next = description[*desc_row][*desc_col..]
                            .char_indices()
                            .nth(1)
                            .map(|(i, _)| *desc_col + i)
                            .unwrap_or(line_len);
                        *desc_col = next;
                    } else if *desc_row + 1 < description.len() {
                        *desc_row += 1;
                        *desc_col = 0;
                    }
                }
                KeyCode::Up => {
                    if *desc_row > 0 {
                        *desc_row -= 1;
                        *desc_col = (*desc_col).min(description[*desc_row].len());
                    } else {
                        *focused_field = TaskFormField::Title;
                    }
                }
                KeyCode::Down => {
                    if *desc_row + 1 < description.len() {
                        *desc_row += 1;
                        *desc_col = (*desc_col).min(description[*desc_row].len());
                    } else {
                        *focused_field = TaskFormField::Epic;
                    }
                }
                KeyCode::Tab => {
                    *focused_field = TaskFormField::Epic;
                }
                KeyCode::BackTab => {
                    *focused_field = TaskFormField::Title;
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Esc => {
                    self.modal = None;
                    return;
                }
                _ => {}
            },
            TaskFormField::Epic => match code {
                KeyCode::Left => {
                    if *epic_index > 0 {
                        *epic_index -= 1;
                    }
                }
                KeyCode::Right => {
                    if *epic_index + 1 < epic_count {
                        *epic_index += 1;
                    }
                }
                KeyCode::Tab | KeyCode::Down => {
                    *focused_field = TaskFormField::Priority;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    *focused_field = TaskFormField::Description;
                }
                KeyCode::Enter => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Esc => {
                    self.modal = None;
                    return;
                }
                _ => {}
            },
            TaskFormField::Priority => match code {
                KeyCode::Left => {
                    if *priority_index > 0 {
                        *priority_index -= 1;
                    }
                }
                KeyCode::Right => {
                    if *priority_index < 3 {
                        *priority_index += 1;
                    }
                }
                KeyCode::Tab | KeyCode::Down => {
                    *focused_field = TaskFormField::Title;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    *focused_field = TaskFormField::Epic;
                }
                KeyCode::Enter => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                    return;
                }
                KeyCode::Esc => {
                    self.modal = None;
                    return;
                }
                _ => {}
            },
        }
    }

    fn submit_task_create(&mut self) {
        let (title, description, epic_index, priority_index) = match &self.modal {
            Some(Modal::TaskCreate {
                title,
                description,
                epic_index,
                priority_index,
                ..
            }) => (title.clone(), description.clone(), *epic_index, *priority_index),
            _ => return,
        };

        if title.trim().is_empty() {
            if let Some(Modal::TaskCreate { ref mut error, .. }) = self.modal {
                *error = Some("Title cannot be empty".to_string());
            }
            return;
        }

        let agent_str = std::env::var("SWARMIT_AGENT").unwrap_or_else(|_| "tui-user".to_string());
        let agent = match AgentId::new(&agent_str) {
            Ok(a) => a,
            Err(e) => {
                if let Some(Modal::TaskCreate { ref mut error, .. }) = self.modal {
                    *error = Some(format!("Invalid agent: {}", e));
                }
                return;
            }
        };

        let task_prefix = self
            .state
            .config
            .as_ref()
            .map(|c| c.task_prefix.clone())
            .unwrap_or_else(|| "TASK".to_string());

        let next_id = ItemId::new(&task_prefix, self.state.task_seq + 1);

        let epic_id: Option<ItemId> = if epic_index == 0 {
            None
        } else {
            let epics: Vec<ItemId> = self.state.epics.keys().cloned().collect();
            epics.get(epic_index - 1).cloned()
        };

        let priority = match priority_index {
            0 => Priority::Low,
            1 => Priority::Medium,
            2 => Priority::High,
            _ => Priority::Urgent,
        };

        let desc = description.join("\n");
        let desc_opt = if desc.trim().is_empty() { None } else { Some(desc) };

        let op = Operation::new(
            agent,
            OperationKind::CreateTask {
                id: next_id,
                title,
                description: desc_opt,
                priority,
                epic_id,
            },
        );

        let swarmit_dir = self.project_root.join(".swarmit");
        let log_path = swarmit_dir.join("operations.log");
        let lock_path = swarmit_dir.join("operations.lock");

        match try_append_with_timeout(&lock_path, || append_operation(&log_path, &op)) {
            Ok(()) => {
                let _ = self.state.apply(op);
                // Advance log_offset to current file size to skip the op we just wrote
                self.log_offset = std::fs::metadata(&log_path)
                    .map(|m| m.len())
                    .unwrap_or(self.log_offset);
                self.modal = None;
                self.rebuild_dashboard_rows();
            }
            Err(e) => {
                if let Some(Modal::TaskCreate { ref mut error, .. }) = self.modal {
                    *error = Some(format!("Write failed: {}", e));
                }
            }
        }
    }

    fn go_back(&mut self) {
        self.screen = match &self.screen {
            Screen::Help => Screen::Dashboard,
            Screen::TaskDetail { task_id } => self
                .state
                .tasks
                .get(task_id)
                .and_then(|t| t.epic_id.clone())
                .map(|epic_id| Screen::Board { epic_id })
                .unwrap_or(Screen::Dashboard),
            Screen::Board { .. } => Screen::Dashboard,
            Screen::Activity => Screen::Dashboard,
            Screen::Dashboard => Screen::Dashboard,
        };
        self.selected_index = 0;
        if self.screen == Screen::Dashboard {
            self.rebuild_dashboard_rows();
        }
    }

    /// Rebuild the flat `dashboard_rows` cache from current state.
    ///
    /// Structure: orphan tasks first (no epic), then epics with their tasks.
    pub fn rebuild_dashboard_rows(&mut self) {
        let mut rows = Vec::new();

        // 1. Orphan tasks first (no epic), applying status filter.
        let orphans: Vec<ItemId> = self
            .state
            .tasks
            .values()
            .filter(|t| {
                t.epic_id.is_none()
                    && self.dashboard_filter.map_or(true, |f| t.status == f)
            })
            .map(|t| t.id.clone())
            .collect();

        for task_id in orphans {
            rows.push(DashboardRow::Task { id: task_id });
        }

        // 2. Epics (with their tasks when expanded), in BTreeMap order (deterministic).
        for (epic_id, epic) in &self.state.epics {
            rows.push(DashboardRow::Epic { id: epic_id.clone() });

            if !self.collapsed_epics.contains(epic_id) {
                for task_id in &epic.task_ids {
                    // Apply status filter to epic tasks.
                    if let Some(filter) = &self.dashboard_filter {
                        if self.state.tasks.get(task_id).map_or(false, |t| t.status != *filter) {
                            continue;
                        }
                    }
                    rows.push(DashboardRow::Task { id: task_id.clone() });
                }
            }
        }

        self.dashboard_rows = rows;

        // Clamp selection to valid bounds.
        let max = self.dashboard_rows.len().saturating_sub(1);
        if self.selected_index > max {
            self.selected_index = max;
        }
    }

    fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    fn move_down(&mut self) {
        let max = self.current_list_len().saturating_sub(1);
        if self.selected_index < max {
            self.selected_index += 1;
        }
    }

    fn select_item(&mut self) {
        match self.screen.clone() {
            Screen::Dashboard => {
                match self.dashboard_rows.get(self.selected_index).cloned() {
                    Some(DashboardRow::Epic { id }) => {
                        self.screen = Screen::Board { epic_id: id };
                        self.selected_index = 0;
                    }
                    Some(DashboardRow::Task { id }) => {
                        self.screen = Screen::TaskDetail { task_id: id };
                        self.selected_index = 0;
                    }
                    None => {}
                }
            }
            Screen::Board { epic_id } => {
                // Select a task to view details
                let tasks: Vec<_> = self
                    .state
                    .tasks
                    .values()
                    .filter(|t| t.epic_id.as_ref() == Some(&epic_id))
                    .map(|t| t.id.clone())
                    .collect();
                if let Some(task_id) = tasks.get(self.selected_index) {
                    self.screen = Screen::TaskDetail {
                        task_id: task_id.clone(),
                    };
                    self.selected_index = 0;
                }
            }
            _ => {}
        }
    }

    fn current_list_len(&self) -> usize {
        match &self.screen {
            Screen::Dashboard => self.dashboard_rows.len(),
            Screen::Board { epic_id } => self
                .state
                .tasks
                .values()
                .filter(|t| t.epic_id.as_ref() == Some(epic_id))
                .count(),
            Screen::Activity => {
                // Show up to 200 recent operations — list length for scrolling
                200
            }
            _ => 0,
        }
    }

    /// Poll the file watcher channel and apply any new operations.
    pub fn poll_log_changes(&mut self) {
        let Some(rx) = &self.watcher_rx else { return };

        // Drain all pending events (non-blocking).
        while rx.try_recv().is_ok() {
            // Events are batched by the debouncer; one drain is enough.
        }

        // Read new operations from the last known byte offset.
        if let Ok((new_ops, new_offset)) =
            read_operations_since(&self.log_path, self.log_offset)
        {
            if !new_ops.is_empty() {
                for op in new_ops {
                    let _ = self.state.apply(op);
                }
                self.log_offset = new_offset;
                self.rebuild_dashboard_rows();
            }
        }
    }
}

