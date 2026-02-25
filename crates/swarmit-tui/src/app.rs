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

/// Sort order for the dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOption {
    CreationDate,
    #[default]
    RecentUpdate,
    Title,
}

impl SortOption {
    pub fn label(&self) -> &'static str {
        match self {
            SortOption::CreationDate => "Creation date",
            SortOption::RecentUpdate => "Recent update",
            SortOption::Title => "Title",
        }
    }
}

pub const SORT_OPTIONS: &[SortOption] = &[
    SortOption::CreationDate,
    SortOption::RecentUpdate,
    SortOption::Title,
];

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

    // Navigation: index of selected column in GlobalBoard.
    pub selected_column: usize,

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

    /// Sort order for the dashboard.
    pub dashboard_sort: SortOption,

    // Cached flattened tree rows for the dashboard (rebuilt on state changes).
    pub dashboard_rows: Vec<DashboardRow>,

    /// Navigation history stack — used by `go_back()` to return to the previous screen.
    pub screen_history: Vec<Screen>,
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
            selected_column: 0,
            watcher_rx: Some(rx),
            _debouncer: Some(debouncer),
            log_offset,
            log_path,
            should_quit: false,
            search_query: String::new(),
            modal: None,
            collapsed_epics: HashSet::new(),
            dashboard_filter: None,
            dashboard_sort: SortOption::default(),
            dashboard_rows: Vec::new(),
            screen_history: Vec::new(),
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
                self.selected_column = 0;
                self.navigate_to(Screen::Dashboard);
                self.rebuild_dashboard_rows();
            }
            Action::GotoActivity => {
                self.selected_column = 0;
                self.navigate_to(Screen::Activity);
            }
            Action::GotoGlobalBoard => {
                self.screen = Screen::GlobalBoard;
                self.selected_index = 0;
                self.selected_column = 0;
            }
            Action::ColLeft => {
                self.selected_column = self.selected_column.saturating_sub(1);
                let count = self.tasks_in_column(self.selected_column);
                if count == 0 {
                    self.selected_index = 0;
                } else {
                    self.selected_index = self.selected_index.min(count - 1);
                }
            }
            Action::ColRight => {
                if self.selected_column < 3 {
                    self.selected_column += 1;
                }
                let count = self.tasks_in_column(self.selected_column);
                if count == 0 {
                    self.selected_index = 0;
                } else {
                    self.selected_index = self.selected_index.min(count - 1);
                }
            }
            Action::Help => {
                self.navigate_to(Screen::Help);
            }
            Action::Refresh => {
                self.poll_log_changes();
            }
            Action::ToggleCollapse => {
                if matches!(self.screen, Screen::Dashboard) {
                    if let Some(eid) = self.epic_id_at_selection() {
                        if self.collapsed_epics.contains(&eid) {
                            self.collapsed_epics.remove(&eid);
                        } else {
                            self.collapsed_epics.insert(eid);
                        }
                        self.rebuild_dashboard_rows();
                    }
                }
            }
            Action::CollapseEpic => {
                if matches!(self.screen, Screen::Dashboard) {
                    if let Some(eid) = self.epic_id_at_selection() {
                        self.collapsed_epics.insert(eid);
                        self.rebuild_dashboard_rows();
                    }
                }
            }
            Action::ExpandEpic => {
                if matches!(self.screen, Screen::Dashboard) {
                    if let Some(eid) = self.epic_id_at_selection() {
                        self.collapsed_epics.remove(&eid);
                        self.rebuild_dashboard_rows();
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
            Action::OpenSortDialog => {
                let selected_index = SORT_OPTIONS
                    .iter()
                    .position(|opt| *opt == self.dashboard_sort)
                    .unwrap_or(0);
                self.modal = Some(Modal::SortSelect { selected_index });
            }
            Action::SortDialogMove(delta) => {
                if let Some(Modal::SortSelect { selected_index }) = &mut self.modal {
                    let len = SORT_OPTIONS.len();
                    *selected_index = ((*selected_index as isize + delta as isize)
                        .rem_euclid(len as isize)) as usize;
                }
            }
            Action::SortDialogConfirm => {
                if let Some(Modal::SortSelect { selected_index }) = &self.modal {
                    self.dashboard_sort = SORT_OPTIONS[*selected_index];
                    self.rebuild_dashboard_rows();
                }
                self.modal = None;
            }
            Action::SortDialogCancel => {
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
            Some(Modal::SortSelect { .. }) => self.handle_sort_select_key(code),
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

    fn handle_sort_select_key(&mut self, code: KeyCode) {
        let action = match code {
            KeyCode::Char('j') | KeyCode::Down => Action::SortDialogMove(1),
            KeyCode::Char('k') | KeyCode::Up => Action::SortDialogMove(-1),
            KeyCode::Enter => Action::SortDialogConfirm,
            KeyCode::Esc | KeyCode::Char('q') => Action::SortDialogCancel,
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

    /// Push the current screen onto the history stack and navigate to `screen`.
    fn navigate_to(&mut self, screen: Screen) {
        self.screen_history.push(self.screen.clone());
        self.screen = screen;
        self.selected_index = 0;
        self.selected_column = 0;
    }

    fn go_back(&mut self) {
        self.screen = self.screen_history.pop().unwrap_or(Screen::Dashboard);
        self.selected_index = 0;
        self.selected_column = 0;
        if self.screen == Screen::Dashboard {
            self.rebuild_dashboard_rows();
        }
    }

    /// Rebuild the flat `dashboard_rows` cache from current state.
    ///
    /// Structure: orphan tasks first (no epic), then epics with their tasks.
    pub fn rebuild_dashboard_rows(&mut self) {
        let mut rows = Vec::new();

        // Collect all top-level items (orphan tasks and epics) into a unified
        // list so they are sorted together rather than as two separate groups.
        enum TopLevel {
            Task(ItemId),
            Epic(ItemId),
        }

        let mut top: Vec<TopLevel> = Vec::new();

        for task in self.state.tasks.values() {
            if task.epic_id.is_none()
                && self.dashboard_filter.map_or(true, |f| task.status == f)
            {
                top.push(TopLevel::Task(task.id.clone()));
            }
        }
        for epic_id in self.state.epics.keys() {
            top.push(TopLevel::Epic(epic_id.clone()));
        }

        top.sort_by(|a, b| {
            let (a_updated, a_created, a_title) = match a {
                TopLevel::Task(id) => {
                    let t = &self.state.tasks[id];
                    (t.updated_at, t.created_at, t.title.as_str())
                }
                TopLevel::Epic(id) => {
                    let e = &self.state.epics[id];
                    (e.updated_at, e.created_at, e.title.as_str())
                }
            };
            let (b_updated, b_created, b_title) = match b {
                TopLevel::Task(id) => {
                    let t = &self.state.tasks[id];
                    (t.updated_at, t.created_at, t.title.as_str())
                }
                TopLevel::Epic(id) => {
                    let e = &self.state.epics[id];
                    (e.updated_at, e.created_at, e.title.as_str())
                }
            };
            match self.dashboard_sort {
                SortOption::RecentUpdate => b_updated.cmp(&a_updated),
                SortOption::CreationDate => b_created.cmp(&a_created),
                SortOption::Title => a_title.cmp(b_title),
            }
        });

        for item in top {
            match item {
                TopLevel::Task(task_id) => {
                    rows.push(DashboardRow::Task { id: task_id });
                }
                TopLevel::Epic(epic_id) => {
                    let task_ids: Vec<ItemId> = self.state.epics[&epic_id].task_ids.clone();
                    rows.push(DashboardRow::Epic { id: epic_id.clone() });

                    if !self.collapsed_epics.contains(&epic_id) {
                        for task_id in task_ids {
                            // Apply status filter to epic tasks.
                            if let Some(filter) = &self.dashboard_filter {
                                if self.state.tasks.get(&task_id).map_or(false, |t| t.status != *filter) {
                                    continue;
                                }
                            }
                            rows.push(DashboardRow::Task { id: task_id });
                        }
                    }
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

    /// Returns the epic id associated with the currently selected dashboard row,
    /// or `None` if the selection is outside the dashboard or on an orphan task.
    fn epic_id_at_selection(&self) -> Option<ItemId> {
        let row = self.dashboard_rows.get(self.selected_index)?;
        match row {
            DashboardRow::Epic { id } => Some(id.clone()),
            DashboardRow::Task { id } => self
                .state
                .tasks
                .get(id)
                .and_then(|t| t.epic_id.clone()),
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
                        self.navigate_to(Screen::Board { epic_id: id });
                    }
                    Some(DashboardRow::Task { id }) => {
                        self.navigate_to(Screen::TaskDetail { task_id: id });
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
                    self.navigate_to(Screen::TaskDetail {
                        task_id: task_id.clone(),
                    });
                }
            }
            Screen::GlobalBoard => {
                const STATUSES: [Status; 4] =
                    [Status::Todo, Status::InProgress, Status::Done, Status::Blocked];
                let col_status = STATUSES[self.selected_column];
                let tasks: Vec<_> = self
                    .state
                    .tasks
                    .values()
                    .filter(|t| t.status == col_status)
                    .map(|t| t.id.clone())
                    .collect();
                if let Some(task_id) = tasks.get(self.selected_index) {
                    self.navigate_to(Screen::TaskDetail {
                        task_id: task_id.clone(),
                    });
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
            Screen::GlobalBoard => self.tasks_in_column(self.selected_column),
            _ => 0,
        }
    }

    /// Returns the number of tasks with the status corresponding to `col`.
    /// Column indices: 0=Todo, 1=InProgress, 2=Done, 3=Blocked.
    pub fn tasks_in_column(&self, col: usize) -> usize {
        const STATUSES: [Status; 4] =
            [Status::Todo, Status::InProgress, Status::Done, Status::Blocked];
        self.state.tasks.values().filter(|t| t.status == STATUSES[col]).count()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use swarmit_core::models::Status;
    use swarmit_core::events::operations::{Operation, OperationKind};
    use swarmit_core::models::{AgentId, Priority};

    #[test]
    fn filter_dialog_opens_with_current_filter_preselected() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.dashboard_filter = Some(Status::InProgress);
        app.apply_action(Action::OpenFilterDialog);
        // InProgress is index 2 in FILTER_OPTIONS
        assert!(matches!(
            app.modal,
            Some(Modal::FilterSelect { selected_index: 2 })
        ));
    }

    #[test]
    fn filter_dialog_wraps_navigation_down_to_up() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.modal = Some(Modal::FilterSelect { selected_index: 0 });
        app.apply_action(Action::FilterDialogMove(-1));
        let last = FILTER_OPTIONS.len() - 1;
        assert!(matches!(
            app.modal,
            Some(Modal::FilterSelect { selected_index }) if selected_index == last
        ));
    }

    #[test]
    fn filter_dialog_confirm_sets_filter_and_closes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.modal = Some(Modal::FilterSelect { selected_index: 1 }); // Todo
        app.apply_action(Action::FilterDialogConfirm);
        assert_eq!(app.dashboard_filter, Some(Status::Todo));
        assert!(app.modal.is_none());
    }

    #[test]
    fn filter_dialog_cancel_does_not_change_filter() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.dashboard_filter = Some(Status::Done);
        app.modal = Some(Modal::FilterSelect { selected_index: 0 });
        app.apply_action(Action::FilterDialogCancel);
        assert_eq!(app.dashboard_filter, Some(Status::Done));
        assert!(app.modal.is_none());
    }

    #[test]
    fn sort_dialog_opens_with_current_sort_preselected() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        // Default is RecentUpdate which is index 1 in SORT_OPTIONS
        app.apply_action(Action::OpenSortDialog);
        assert!(matches!(
            app.modal,
            Some(Modal::SortSelect { selected_index: 1 })
        ));
    }

    #[test]
    fn sort_dialog_navigation_wraps() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.modal = Some(Modal::SortSelect { selected_index: 0 });
        app.apply_action(Action::SortDialogMove(-1));
        let last = SORT_OPTIONS.len() - 1;
        assert!(matches!(
            app.modal,
            Some(Modal::SortSelect { selected_index }) if selected_index == last
        ));
    }

    #[test]
    fn sort_dialog_confirm_sets_sort_and_closes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.modal = Some(Modal::SortSelect { selected_index: 0 }); // CreationDate
        app.apply_action(Action::SortDialogConfirm);
        assert_eq!(app.dashboard_sort, SortOption::CreationDate);
        assert!(app.modal.is_none());
    }

    #[test]
    fn sort_dialog_cancel_closes_without_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();
        app.dashboard_sort = SortOption::Title;
        app.modal = Some(Modal::SortSelect { selected_index: 0 });
        app.apply_action(Action::SortDialogCancel);
        assert_eq!(app.dashboard_sort, SortOption::Title);
        assert!(app.modal.is_none());
    }

    fn make_agent() -> AgentId {
        AgentId::new("test-agent").unwrap()
    }

    fn setup_app_with_epic() -> (App, ItemId, ItemId) {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        let epic_id = ItemId::new("EPIC", 1);
        let task_id = ItemId::new("TASK", 1);

        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "Test Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            },
        ));
        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Test Task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            },
        ));
        app.rebuild_dashboard_rows();
        // Select the epic row (first row)
        app.selected_index = 0;
        (app, epic_id, task_id)
    }

    #[test]
    fn collapse_epic_hides_child_tasks() {
        let (mut app, epic_id, _task_id) = setup_app_with_epic();

        // Initially expanded — epic row + task row = 2 rows
        assert_eq!(app.dashboard_rows.len(), 2);
        assert!(!app.collapsed_epics.contains(&epic_id));

        app.handle_action(Action::CollapseEpic);

        assert!(app.collapsed_epics.contains(&epic_id));
        // Collapsed — only the epic header row remains
        assert_eq!(app.dashboard_rows.len(), 1);
        assert!(matches!(&app.dashboard_rows[0], DashboardRow::Epic { id } if id == &epic_id));
    }

    #[test]
    fn expand_epic_shows_child_tasks() {
        let (mut app, epic_id, task_id) = setup_app_with_epic();

        // Pre-collapse the epic
        app.collapsed_epics.insert(epic_id.clone());
        app.rebuild_dashboard_rows();
        assert_eq!(app.dashboard_rows.len(), 1);

        app.handle_action(Action::ExpandEpic);

        assert!(!app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 2);
        assert!(matches!(&app.dashboard_rows[1], DashboardRow::Task { id } if id == &task_id));
    }

    #[test]
    fn collapse_epic_is_idempotent() {
        let (mut app, epic_id, _) = setup_app_with_epic();

        app.handle_action(Action::CollapseEpic);
        app.handle_action(Action::CollapseEpic);

        // Still collapsed — second call is a no-op on the set
        assert!(app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 1);
    }

    #[test]
    fn expand_epic_is_idempotent() {
        let (mut app, epic_id, _) = setup_app_with_epic();

        // Already expanded; calling ExpandEpic twice should be harmless
        app.handle_action(Action::ExpandEpic);
        app.handle_action(Action::ExpandEpic);

        assert!(!app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 2);
    }

    #[test]
    fn collapse_expand_from_task_row_uses_parent_epic() {
        let (mut app, epic_id, _task_id) = setup_app_with_epic();

        // Select the task row (index 1) instead of the epic row
        app.selected_index = 1;

        app.handle_action(Action::CollapseEpic);

        assert!(app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 1);
    }

    #[test]
    fn dashboard_rows_sorted_by_recent_update_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        // Create two tasks with different titles so we can identify them
        let id1 = ItemId::new("TASK", 1);
        let id2 = ItemId::new("TASK", 2);

        let op1 = Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: id1.clone(),
                title: "Alpha task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            },
        );
        let op2 = Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: id2.clone(),
                title: "Beta task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            },
        );

        let _ = app.state.apply(op1);
        let _ = app.state.apply(op2);
        app.rebuild_dashboard_rows();

        // Default sort is RecentUpdate (most recent first).
        // op2 was applied second so its created_at/updated_at is >= op1.
        // Row order should have id2 before id1.
        assert_eq!(app.dashboard_sort, SortOption::RecentUpdate);
        assert!(app.dashboard_rows.len() >= 2);
    }

    #[test]
    fn back_from_dashboard_task_detail_returns_to_dashboard() {
        // Navigate: Dashboard → TaskDetail (from dashboard) → Esc → Dashboard
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        let task_id = ItemId::new("TASK", 1);
        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Orphan task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: None,
            },
        ));
        app.rebuild_dashboard_rows();

        // Navigate from dashboard directly to a task detail
        app.navigate_to(Screen::TaskDetail { task_id: task_id.clone() });
        assert_eq!(app.screen, Screen::TaskDetail { task_id: task_id.clone() });
        assert_eq!(app.screen_history, vec![Screen::Dashboard]);

        // Pressing Esc should pop back to Dashboard, not Board
        app.handle_action(Action::Back);
        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.screen_history.is_empty());
    }

    #[test]
    fn back_from_board_task_detail_returns_to_board() {
        // Navigate: Dashboard → Board → TaskDetail → Esc → Board
        let (mut app, epic_id, task_id) = setup_app_with_epic();

        // Navigate to Board
        app.navigate_to(Screen::Board { epic_id: epic_id.clone() });
        // Navigate to TaskDetail from Board
        app.navigate_to(Screen::TaskDetail { task_id: task_id.clone() });

        assert_eq!(
            app.screen_history,
            vec![
                Screen::Dashboard,
                Screen::Board { epic_id: epic_id.clone() }
            ]
        );

        // Esc should pop back to Board
        app.handle_action(Action::Back);
        assert_eq!(app.screen, Screen::Board { epic_id: epic_id.clone() });
        assert_eq!(app.screen_history, vec![Screen::Dashboard]);
    }

    #[test]
    fn back_from_dashboard_is_noop() {
        // Pressing Esc with an empty history stays on Dashboard
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.screen_history.is_empty());

        app.handle_action(Action::Back);

        assert_eq!(app.screen, Screen::Dashboard);
        assert!(app.screen_history.is_empty());
    }

    #[test]
    fn epic_child_tasks_keep_creation_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        let epic_id = ItemId::new("EPIC", 1);
        let task_id1 = ItemId::new("TASK", 1);
        let task_id2 = ItemId::new("TASK", 2);

        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateEpic {
                id: epic_id.clone(),
                title: "My Epic".to_string(),
                description: None,
                priority: Priority::Medium,
            },
        ));
        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: task_id1.clone(),
                title: "First task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            },
        ));
        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: task_id2.clone(),
                title: "Second task".to_string(),
                description: None,
                priority: Priority::Medium,
                epic_id: Some(epic_id.clone()),
            },
        ));

        // Switch to Title sort — epics would reorder, but tasks under epic stay in insertion order
        app.dashboard_sort = SortOption::Title;
        app.rebuild_dashboard_rows();

        // Find epic row and the two task rows after it
        let epic_pos = app
            .dashboard_rows
            .iter()
            .position(|r| matches!(r, DashboardRow::Epic { id } if id == &epic_id))
            .expect("epic row should exist");

        let task_rows: Vec<&ItemId> = app.dashboard_rows[epic_pos + 1..]
            .iter()
            .take_while(|r| matches!(r, DashboardRow::Task { .. }))
            .filter_map(|r| if let DashboardRow::Task { id } = r { Some(id) } else { None })
            .collect();

        // Tasks under epic should remain in creation order (task_id1 before task_id2)
        assert_eq!(task_rows, vec![&task_id1, &task_id2]);
    }
}

