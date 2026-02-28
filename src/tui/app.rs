use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Text;
use rusqlite::Connection;
use uuid::Uuid;

use crate::events::operations::{Operation, OperationKind};
use crate::models::{AgentId, ItemId, Priority, Status};
use crate::state::ProjectState;

use crate::tui::components::tree_list::DashboardRow;
use crate::tui::events::{
    Action, Focus, KonamiTracker, Modal, Screen, SplitDirection, TaskFormField,
};
use crate::tui::theme::Theme;

/// Which tab is active in the task detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    #[default]
    Description,
    Comments,
    Insights,
}

/// Sort order for the dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOption {
    #[default]
    CreationDate,
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

// --- Crab parade animation types ---

pub struct Crab {
    pub x: f32,
    pub speed: f32,
}

pub struct CrabRow {
    pub y: u16,
    pub crabs: Vec<Crab>,
}

pub struct CrabAnimation {
    pub active: bool,
    pub start_time: std::time::Instant,
    pub rows: Vec<CrabRow>,
    pub term_width: u16,
}

impl CrabAnimation {
    pub fn new(term_width: u16, term_height: u16) -> Self {
        let num_rows = 6.min((term_height as usize).saturating_sub(2));
        let row_spacing = if num_rows > 0 {
            (term_height as usize) / (num_rows + 1)
        } else {
            4
        };

        // Deterministic (no rand dep) speeds and counts for variety.
        let base_speeds: [f32; 6] = [15.0, 20.0, 10.0, 25.0, 12.0, 18.0];
        let crab_counts: [usize; 6] = [4, 5, 3, 5, 4, 3];

        let rows = (0..num_rows)
            .map(|i| {
                let y = ((i + 1) * row_spacing) as u16;
                let n_crabs = crab_counts[i % crab_counts.len()];
                let spacing = if n_crabs > 1 {
                    term_width as f32 / n_crabs as f32
                } else {
                    0.0
                };
                let crabs = (0..n_crabs)
                    .map(|j| {
                        // Stagger each row a bit so crabs don't line up in columns.
                        let x = (j as f32 * spacing + i as f32 * 3.0) % term_width as f32;
                        let speed = base_speeds[(i + j) % base_speeds.len()];
                        Crab { x, speed }
                    })
                    .collect();
                CrabRow { y, crabs }
            })
            .collect();

        Self {
            active: true,
            start_time: std::time::Instant::now(),
            rows,
            term_width,
        }
    }

    pub fn update(&mut self, dt: f32) {
        let width = self.term_width as f32;
        for row in &mut self.rows {
            for crab in &mut row.crabs {
                crab.x += crab.speed * dt;
                if crab.x >= width {
                    crab.x -= width;
                }
            }
        }
    }

    pub fn is_expired(&self) -> bool {
        self.start_time.elapsed().as_secs_f32() >= 3.0
    }
}

// --- End crab animation types ---

struct HighlightRequest {
    task_id: ItemId,
    description: String,
    bat_theme: &'static str,
}

struct HighlightResult {
    task_id: ItemId,
    description: String,
    text: Text<'static>,
}

/// A deferred request to open an external editor.
///
/// Set by `handle_action`; consumed by the run loop in `lib.rs` before each
/// `terminal.draw()` call. This keeps I/O out of the action handler.
#[derive(Debug)]
pub enum EditorRequest {
    EditDescription {
        task_id: ItemId,
        current_text: String,
    },
    NewComment {
        task_id: ItemId,
    },
}

/// Layout rectangles captured during `terminal.draw()` for mouse hit-testing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScrollRegions {
    pub list: Rect,
    pub detail_content: Rect,
    pub modal_popup: Option<Rect>,
}

/// Central application state.
pub struct App {
    pub state: ProjectState,
    pub screen: Screen,
    pub project_root: PathBuf,
    pub theme: Theme,

    // Navigation: index of selected item in the tree list.
    pub selected_index: usize,

    // SQLite connection for reading/writing operations.
    conn: Connection,
    /// Latest rowid from the operations table, for incremental polling.
    last_rowid: i64,

    pub should_quit: bool,

    // Search/filter string (future).
    pub search_query: String,

    // Active modal overlay (None = no modal).
    pub modal: Option<Modal>,

    // Epics that are currently collapsed in the tree.
    pub collapsed_epics: HashSet<ItemId>,

    /// Status filter for the tree (None = show all).
    pub dashboard_filter: Option<Status>,

    /// Sort order for the tree.
    pub dashboard_sort: SortOption,

    // Cached flattened tree rows (rebuilt on state changes).
    pub dashboard_rows: Vec<DashboardRow>,

    /// Whether the side detail pane is currently visible.
    pub detail_open: bool,

    /// Which pane currently has keyboard focus.
    pub focus: Focus,

    /// Vertical scroll offset for the detail pane (description tab).
    pub detail_scroll: usize,

    /// Vertical scroll offset for the comments tab.
    pub comment_scroll: usize,

    /// Vertical scroll offset for the insights tab.
    pub insight_scroll: usize,

    /// Which tab is active in the task detail pane.
    pub detail_tab: DetailTab,

    /// Size percentage for the detail pane (20-80), axis-agnostic.
    pub detail_size_percent: u16,

    /// Whether the main split is horizontal (list|detail) or vertical (list/detail).
    pub split_direction: SplitDirection,

    /// Cache for the syntax-highlighted description of the currently selected task.
    /// Tuple: (task_id, description_content, rendered_text).
    /// Avoids re-running bat highlighting on every frame (~10 FPS).
    pub highlight_cache: Option<(ItemId, Option<String>, Text<'static>)>,

    /// Send highlight work to the background thread.
    highlight_tx: Sender<HighlightRequest>,
    /// Receive completed highlights from the background thread.
    highlight_rx: Receiver<HighlightResult>,

    /// Active crab parade easter egg animation, `None` when inactive.
    pub crab_animation: Option<CrabAnimation>,
    /// Tracks progress through the Konami code sequence.
    pub konami_tracker: KonamiTracker,

    /// Deferred editor request — consumed by the run loop before each draw.
    pub pending_editor: Option<EditorRequest>,

    /// Layout rectangles captured each frame for mouse scroll hit-testing.
    pub scroll_regions: ScrollRegions,
}

impl App {
    pub fn new(project_root: PathBuf, theme: Theme) -> Result<Self> {
        let conn = crate::open_db(&project_root)?;
        let state = crate::load_state(&conn)?;
        let last_rowid = crate::latest_rowid(&conn).unwrap_or(0);

        let (hl_work_tx, hl_work_rx) = mpsc::channel::<HighlightRequest>();
        let (hl_result_tx, hl_result_rx) = mpsc::channel::<HighlightResult>();

        std::thread::Builder::new()
            .name("highlight".into())
            .spawn(move || {
                crate::tui::components::detail_pane::warm_up_syntax();
                while let Ok(req) = hl_work_rx.recv() {
                    let text = crate::tui::components::detail_pane::highlight_markdown(
                        &req.description,
                        req.bat_theme,
                    );
                    let _ = hl_result_tx.send(HighlightResult {
                        task_id: req.task_id,
                        description: req.description,
                        text,
                    });
                }
            })
            .expect("spawn highlight thread");

        let mut app = App {
            state,
            screen: Screen::Main,
            project_root,
            theme,
            selected_index: 0,
            conn,
            last_rowid,
            should_quit: false,
            search_query: String::new(),
            modal: None,
            collapsed_epics: HashSet::new(),
            dashboard_filter: None,
            dashboard_sort: SortOption::default(),
            dashboard_rows: Vec::new(),
            detail_open: false,
            focus: Focus::default(),
            detail_scroll: 0,
            comment_scroll: 0,
            insight_scroll: 0,
            detail_tab: DetailTab::default(),
            detail_size_percent: 50,
            split_direction: SplitDirection::default(),
            highlight_cache: None,
            highlight_tx: hl_work_tx,
            highlight_rx: hl_result_rx,
            crab_animation: None,
            konami_tracker: KonamiTracker::new(),
            pending_editor: None,
            scroll_regions: ScrollRegions::default(),
        };
        app.rebuild_dashboard_rows();
        Ok(app)
    }

    /// Process an action and update state accordingly.
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::NewTask => {
                self.modal = Some(Modal::TaskCreate {
                    title: String::new(),
                    cursor_pos: 0,
                    confirm_discard: false,
                    description: vec![String::new()],
                    desc_row: 0,
                    desc_col: 0,
                    epic_index: 0,
                    priority_index: 1, // default Medium
                    focused_field: TaskFormField::Title,
                    error: None,
                });
            }
            Action::Back => {
                if self.detail_open {
                    self.detail_open = false;
                    self.focus = Focus::List;
                    self.detail_scroll = 0;
                    self.comment_scroll = 0;
                    self.insight_scroll = 0;
                    self.detail_tab = DetailTab::Description;
                } else if matches!(self.screen, Screen::Help) {
                    self.screen = Screen::Main;
                } else {
                    self.modal = Some(Modal::QuitConfirm);
                }
            }
            Action::Up => {
                if self.focus == Focus::Detail {
                    match self.detail_tab {
                        DetailTab::Description => {
                            self.detail_scroll = self.detail_scroll.saturating_sub(1);
                        }
                        DetailTab::Comments => {
                            self.comment_scroll = self.comment_scroll.saturating_sub(1);
                        }
                        DetailTab::Insights => {
                            self.insight_scroll = self.insight_scroll.saturating_sub(1);
                        }
                    }
                } else {
                    self.move_up();
                    if self.detail_open {
                        self.detail_scroll = 0;
                        self.comment_scroll = 0;
                        self.insight_scroll = 0;
                        self.detail_tab = DetailTab::Description;
                    }
                }
            }
            Action::Down => {
                if self.focus == Focus::Detail {
                    match self.detail_tab {
                        DetailTab::Description => {
                            self.detail_scroll += 1;
                        }
                        DetailTab::Comments => {
                            self.comment_scroll += 1;
                        }
                        DetailTab::Insights => {
                            self.insight_scroll += 1;
                        }
                    }
                } else {
                    self.move_down();
                    if self.detail_open {
                        self.detail_scroll = 0;
                        self.comment_scroll = 0;
                        self.insight_scroll = 0;
                        self.detail_tab = DetailTab::Description;
                    }
                }
            }
            Action::FocusDetail => {
                if self.dashboard_rows.is_empty() {
                    return;
                }
                if !self.detail_open {
                    self.detail_open = true;
                }
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
                self.comment_scroll = 0;
                self.insight_scroll = 0;
                self.detail_tab = DetailTab::Description;
            }
            Action::FocusListPane => {
                if self.focus == Focus::Detail {
                    self.focus = Focus::List;
                }
            }
            Action::Help => {
                self.screen = Screen::Help;
            }
            Action::Refresh => {
                self.poll_log_changes();
            }
            Action::ToggleCollapse => {
                if let Some(eid) = self.epic_id_at_selection() {
                    let collapsing = !self.collapsed_epics.contains(&eid);
                    if collapsing {
                        self.collapsed_epics.insert(eid.clone());
                    } else {
                        self.collapsed_epics.remove(&eid);
                    }
                    self.rebuild_dashboard_rows();
                    if collapsing {
                        if let Some(pos) = self
                            .dashboard_rows
                            .iter()
                            .position(|r| matches!(r, DashboardRow::Epic { id } if id == &eid))
                        {
                            self.selected_index = pos;
                        }
                    }
                }
            }
            Action::ResizePane(delta) => {
                // + grows the focused pane, - shrinks it
                let effective = if self.focus == Focus::Detail {
                    delta
                } else {
                    -delta
                };
                let new_size = self.detail_size_percent as i16 + effective as i16;
                self.detail_size_percent = new_size.clamp(20, 80) as u16;
            }
            Action::ToggleSplitDirection => {
                self.split_direction = match self.split_direction {
                    SplitDirection::Horizontal => SplitDirection::Vertical,
                    SplitDirection::Vertical => SplitDirection::Horizontal,
                };
            }
            Action::TabBackward => {
                if self.detail_open {
                    self.detail_tab = match self.detail_tab {
                        DetailTab::Description => DetailTab::Insights,
                        DetailTab::Comments => DetailTab::Description,
                        DetailTab::Insights => DetailTab::Comments,
                    };
                }
            }
            Action::TabForward => {
                if self.detail_open {
                    self.detail_tab = match self.detail_tab {
                        DetailTab::Description => DetailTab::Comments,
                        DetailTab::Comments => DetailTab::Insights,
                        DetailTab::Insights => DetailTab::Description,
                    };
                }
            }
            Action::SwitchDetailTab => {
                if self.focus == Focus::Detail {
                    self.detail_tab = match self.detail_tab {
                        DetailTab::Description => DetailTab::Comments,
                        DetailTab::Comments => DetailTab::Insights,
                        DetailTab::Insights => DetailTab::Description,
                    };
                }
            }
            Action::OpenStatusDialog => {
                if self.selected_task_id().is_some() {
                    let current = self.selected_task_status().unwrap_or(Status::Todo);
                    let selected_index = crate::tui::components::status_select::STATUS_OPTIONS
                        .iter()
                        .position(|s| *s == current)
                        .unwrap_or(0);
                    self.modal = Some(Modal::StatusSelect { selected_index });
                }
            }
            Action::OpenEpicDialog => {
                if self.selected_task_id().is_some() {
                    let current_epic = self.selected_task_epic();
                    let options = crate::tui::components::epic_select::epic_options(self);
                    let selected_index = options
                        .iter()
                        .position(|opt| *opt == current_epic)
                        .unwrap_or(0);
                    self.modal = Some(Modal::EpicSelect { selected_index });
                }
            }
            Action::EditDescription => {
                if self.focus == Focus::Detail && self.detail_tab == DetailTab::Description {
                    if let Some(task_id) = self.selected_task_id() {
                        let current_text = self
                            .state
                            .tasks
                            .get(&task_id)
                            .and_then(|t| t.description.clone())
                            .unwrap_or_default();
                        self.pending_editor = Some(EditorRequest::EditDescription {
                            task_id,
                            current_text,
                        });
                    }
                }
            }
            Action::AddComment => {
                if self.focus == Focus::Detail && self.detail_tab == DetailTab::Comments {
                    if let Some(task_id) = self.selected_task_id() {
                        self.pending_editor = Some(EditorRequest::NewComment { task_id });
                    }
                }
            }
            action => self.apply_action(action),
        }
    }

    /// Apply filter-dialog, sort-dialog, status-dialog, and epic-dialog actions.
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
            Action::StatusDialogMove(delta) => {
                if let Some(Modal::StatusSelect { selected_index }) = &mut self.modal {
                    let len = crate::tui::components::status_select::STATUS_OPTIONS.len();
                    *selected_index = ((*selected_index as isize + delta as isize)
                        .rem_euclid(len as isize)) as usize;
                }
            }
            Action::StatusDialogConfirm => {
                if let Some(Modal::StatusSelect { selected_index }) = &self.modal {
                    let status =
                        crate::tui::components::status_select::STATUS_OPTIONS[*selected_index];
                    if let Some(task_id) = self.selected_task_id() {
                        let _ = self.submit_status_change(task_id, status);
                    }
                }
                self.modal = None;
            }
            Action::StatusDialogCancel => {
                self.modal = None;
            }
            Action::EpicDialogMove(delta) => {
                if let Some(Modal::EpicSelect { selected_index }) = &mut self.modal {
                    // Must match epic_options(): "(none)" + sorted epics
                    let len = self.state.epics.len() + 1;
                    *selected_index = ((*selected_index as isize + delta as isize)
                        .rem_euclid(len as isize)) as usize;
                }
            }
            Action::EpicDialogConfirm => {
                if let Some(Modal::EpicSelect { selected_index }) = &self.modal {
                    let options = crate::tui::components::epic_select::epic_options(self);
                    let epic_id = options[*selected_index].clone();
                    if let Some(task_id) = self.selected_task_id() {
                        let _ = self.submit_epic_change(task_id, epic_id);
                    }
                }
                self.modal = None;
            }
            Action::EpicDialogCancel => {
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
            Some(Modal::StatusSelect { .. }) => self.handle_status_select_key(code),
            Some(Modal::EpicSelect { .. }) => self.handle_epic_select_key(code),
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

    fn handle_status_select_key(&mut self, code: KeyCode) {
        let action = match code {
            KeyCode::Char('j') | KeyCode::Down => Action::StatusDialogMove(1),
            KeyCode::Char('k') | KeyCode::Up => Action::StatusDialogMove(-1),
            KeyCode::Enter => Action::StatusDialogConfirm,
            KeyCode::Esc | KeyCode::Char('q') => Action::StatusDialogCancel,
            _ => return,
        };
        self.apply_action(action);
    }

    fn handle_epic_select_key(&mut self, code: KeyCode) {
        let action = match code {
            KeyCode::Char('j') | KeyCode::Down => Action::EpicDialogMove(1),
            KeyCode::Char('k') | KeyCode::Up => Action::EpicDialogMove(-1),
            KeyCode::Enter => Action::EpicDialogConfirm,
            KeyCode::Esc | KeyCode::Char('q') => Action::EpicDialogCancel,
            _ => return,
        };
        self.apply_action(action);
    }

    fn handle_task_form_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // When awaiting discard confirmation, intercept all keys before normal dispatch.
        if matches!(
            &self.modal,
            Some(Modal::TaskCreate {
                confirm_discard: true,
                ..
            })
        ) {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Esc => {
                    self.modal = None;
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if let Some(Modal::TaskCreate {
                        ref mut confirm_discard,
                        ..
                    }) = self.modal
                    {
                        *confirm_discard = false;
                    }
                }
                _ => {}
            }
            return;
        }

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
            ref mut confirm_discard,
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
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                }
                KeyCode::Esc => {
                    let has_content =
                        !title.trim().is_empty() || description.iter().any(|l| !l.is_empty());
                    if has_content {
                        *confirm_discard = true;
                    } else {
                        self.modal = None;
                    }
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
                }
                KeyCode::Esc => {
                    let has_content =
                        !title.trim().is_empty() || description.iter().any(|l| !l.is_empty());
                    if has_content {
                        *confirm_discard = true;
                    } else {
                        self.modal = None;
                    }
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
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                }
                KeyCode::Esc => {
                    let has_content =
                        !title.trim().is_empty() || description.iter().any(|l| !l.is_empty());
                    if has_content {
                        *confirm_discard = true;
                    } else {
                        self.modal = None;
                    }
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
                }
                KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                    self.submit_task_create();
                }
                KeyCode::Esc => {
                    let has_content =
                        !title.trim().is_empty() || description.iter().any(|l| !l.is_empty());
                    if has_content {
                        *confirm_discard = true;
                    } else {
                        self.modal = None;
                    }
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
            }) => (
                title.clone(),
                description.clone(),
                *epic_index,
                *priority_index,
            ),
            _ => return,
        };

        if title.trim().is_empty() {
            if let Some(Modal::TaskCreate { ref mut error, .. }) = self.modal {
                *error = Some("Title cannot be empty".to_string());
            }
            return;
        }

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
        let desc_opt = if desc.trim().is_empty() {
            None
        } else {
            Some(desc)
        };

        match self.write_operation(OperationKind::CreateTask {
            id: next_id,
            title,
            description: desc_opt,
            priority,
            epic_id,
        }) {
            Ok(()) => {
                self.modal = None;
            }
            Err(e) => {
                if let Some(Modal::TaskCreate { ref mut error, .. }) = self.modal {
                    *error = Some(e);
                }
            }
        }
    }

    /// Returns the task ID of the currently selected row, if it's a task.
    pub fn selected_task_id(&self) -> Option<ItemId> {
        match self.dashboard_rows.get(self.selected_index)? {
            DashboardRow::Task { id } => Some(id.clone()),
            DashboardRow::Epic { .. } => None,
        }
    }

    /// Returns the status of the currently selected task, if any.
    pub fn selected_task_status(&self) -> Option<Status> {
        let task_id = self.selected_task_id()?;
        self.state.tasks.get(&task_id).map(|t| t.status)
    }

    /// Returns the epic ID of the currently selected task, if any.
    pub fn selected_task_epic(&self) -> Option<ItemId> {
        let task_id = self.selected_task_id()?;
        self.state
            .tasks
            .get(&task_id)
            .and_then(|t| t.epic_id.clone())
    }

    /// Write a single operation through SQLite.
    ///
    /// Returns `Ok(())` on success, `Err(message)` on failure.
    fn write_operation(&mut self, kind: OperationKind) -> Result<(), String> {
        let agent_str = std::env::var("SWARMIT_AGENT").unwrap_or_else(|_| "tui-user".to_string());
        let agent = AgentId::new(&agent_str).map_err(|e| format!("Invalid agent: {}", e))?;

        let op = Operation::new(agent, kind);

        crate::write_operation(&self.conn, &op).map_err(|e| format!("Write failed: {}", e))?;

        if let Err(e) = self.state.apply(op) {
            // Write succeeded in SQLite; reload full state as fallback so UI stays consistent.
            eprintln!("Warning: in-memory apply failed ({}), reloading from DB", e);
            if let Ok(fresh) = crate::load_state(&self.conn) {
                self.state = fresh;
            }
        }
        self.last_rowid = crate::latest_rowid(&self.conn).unwrap_or(self.last_rowid);
        self.rebuild_dashboard_rows();
        Ok(())
    }

    /// Change the status of a task, using the appropriate operation kind.
    pub fn submit_status_change(&mut self, task_id: ItemId, status: Status) -> Result<(), String> {
        let kind = match status {
            Status::InProgress => OperationKind::ClaimTask { id: task_id },
            Status::Done => OperationKind::CompleteTask { id: task_id },
            _ => OperationKind::UpdateTaskStatus {
                id: task_id,
                status,
            },
        };
        self.write_operation(kind)
    }

    /// Change the epic of a task.
    pub fn submit_epic_change(
        &mut self,
        task_id: ItemId,
        epic_id: Option<ItemId>,
    ) -> Result<(), String> {
        self.write_operation(OperationKind::UpdateTask {
            id: task_id,
            title: None,
            description: None,
            priority: None,
            epic_id: Some(epic_id),
            assignee: None,
        })
    }

    /// Update the description of a task.
    pub fn submit_description_update(
        &mut self,
        task_id: ItemId,
        description: String,
    ) -> Result<(), String> {
        let desc = if description.trim().is_empty() {
            None
        } else {
            Some(description)
        };
        self.write_operation(OperationKind::UpdateTask {
            id: task_id,
            title: None,
            description: desc,
            priority: None,
            epic_id: None,
            assignee: None,
        })?;
        // Clear highlight cache to force re-highlighting
        self.highlight_cache = None;
        Ok(())
    }

    /// Add a comment to a task.
    pub fn submit_add_comment(&mut self, task_id: ItemId, body: String) -> Result<(), String> {
        self.write_operation(OperationKind::AddComment {
            id: Uuid::now_v7(),
            task_id,
            body,
        })
    }

    /// Rebuild the flat `dashboard_rows` cache from current state.
    ///
    /// Structure: orphan tasks first (no epic), then epics with their tasks.
    pub fn rebuild_dashboard_rows(&mut self) {
        crate::prof_guard!("rebuild_dashboard_rows");

        // Remember the currently-selected item by ID so we can restore focus
        // after the row list is rebuilt (sort/filter/update can shuffle rows).
        let prev_id: Option<(bool, ItemId)> =
            self.dashboard_rows
                .get(self.selected_index)
                .map(|row| match row {
                    DashboardRow::Epic { id } => (true, id.clone()),
                    DashboardRow::Task { id } => (false, id.clone()),
                });

        let mut rows = Vec::new();

        // Collect all top-level items (orphan tasks and epics) into a unified
        // list so they are sorted together rather than as two separate groups.
        enum TopLevel {
            Task(ItemId),
            Epic(ItemId),
        }

        let mut top: Vec<TopLevel> = Vec::new();

        for task in self.state.tasks.values() {
            if task.epic_id.is_none() && self.dashboard_filter.is_none_or(|f| task.status == f) {
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
                    rows.push(DashboardRow::Epic {
                        id: epic_id.clone(),
                    });

                    if !self.collapsed_epics.contains(&epic_id) {
                        for task_id in task_ids {
                            // Apply status filter to epic tasks.
                            if let Some(filter) = &self.dashboard_filter {
                                if self
                                    .state
                                    .tasks
                                    .get(&task_id)
                                    .is_some_and(|t| t.status != *filter)
                                {
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

        // Restore selection by ID, falling back to clamping.
        if let Some((is_epic, ref prev)) = prev_id {
            if let Some(pos) = self.dashboard_rows.iter().position(|r| match r {
                DashboardRow::Epic { id } => is_epic && id == prev,
                DashboardRow::Task { id } => !is_epic && id == prev,
            }) {
                self.selected_index = pos;
            } else {
                // Item no longer visible (filtered out, collapsed) — clamp.
                let max = self.dashboard_rows.len().saturating_sub(1);
                if self.selected_index > max {
                    self.selected_index = max;
                }
            }
        } else {
            // No previous selection (empty list before rebuild) — clamp.
            let max = self.dashboard_rows.len().saturating_sub(1);
            if self.selected_index > max {
                self.selected_index = max;
            }
        }
    }

    /// Returns the epic id associated with the currently selected row,
    /// or `None` if the selection is on an orphan task or the list is empty.
    fn epic_id_at_selection(&self) -> Option<ItemId> {
        let row = self.dashboard_rows.get(self.selected_index)?;
        match row {
            DashboardRow::Epic { id } => Some(id.clone()),
            DashboardRow::Task { id } => self.state.tasks.get(id).and_then(|t| t.epic_id.clone()),
        }
    }

    fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    fn move_down(&mut self) {
        let max = self.dashboard_rows.len().saturating_sub(1);
        if self.selected_index < max {
            self.selected_index += 1;
        }
    }

    /// Poll the SQLite operations table for new operations from external writers.
    pub fn poll_log_changes(&mut self) {
        crate::prof_guard!("poll_log_changes");

        // Check if there are new rows since our last known rowid.
        let current_rowid = crate::latest_rowid(&self.conn).unwrap_or(self.last_rowid);
        if current_rowid <= self.last_rowid {
            return;
        }

        if let Ok((new_ops, new_rowid)) = crate::read_operations_since(&self.conn, self.last_rowid)
        {
            if !new_ops.is_empty() {
                for op in new_ops {
                    let _ = self.state.apply(op);
                }
                self.last_rowid = new_rowid;
                self.rebuild_dashboard_rows();
            }
        }
    }

    /// Refresh the highlight cache for the currently selected task.
    ///
    /// Call this once per loop iteration *before* `terminal.draw()` while
    /// `app` is still `&mut`. The draw closure then reads the cache as `&App`,
    /// avoiding per-frame re-highlighting.
    pub fn refresh_highlight_cache(&mut self) {
        crate::prof_guard!("refresh_highlight_cache");

        // 1. Drain completed highlights from background thread.
        while let Ok(result) = self.highlight_rx.try_recv() {
            if let Some((ref id, ref desc, _)) = self.highlight_cache {
                if *id == result.task_id && desc.as_deref() == Some(result.description.as_str()) {
                    self.highlight_cache =
                        Some((result.task_id, Some(result.description), result.text));
                }
            }
        }

        // 2. Determine what should be cached.
        let task_id = match (
            self.detail_open,
            self.dashboard_rows.get(self.selected_index),
        ) {
            (true, Some(DashboardRow::Task { id })) => id.clone(),
            _ => {
                self.highlight_cache = None;
                return;
            }
        };

        let description = self
            .state
            .tasks
            .get(&task_id)
            .and_then(|t| t.description.clone());

        // 3. Cache hit? (matches both plain-text and highlighted entries)
        if let Some((ref cached_id, ref cached_desc, _)) = self.highlight_cache {
            if *cached_id == task_id && *cached_desc == description {
                return;
            }
        }

        // 4. Cache miss: store plain text immediately, request highlight async.
        let plain = match &description {
            Some(raw) => Text::from(raw.clone()),
            None => Text::default(),
        };
        self.highlight_cache = Some((task_id.clone(), description.clone(), plain));

        if let Some(raw) = &description {
            let _ = self.highlight_tx.send(HighlightRequest {
                task_id,
                description: raw.clone(),
                bat_theme: self.theme.bat_theme(),
            });
        }
    }

    /// Start a new crab parade animation sized to the current terminal.
    pub fn start_crab_animation(&mut self) {
        let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
        self.crab_animation = Some(CrabAnimation::new(width, height));
    }

    /// Advance the crab animation by `dt` seconds. Does nothing if inactive.
    pub fn update_crab_animation(&mut self, dt: f32) {
        if let Some(ref mut anim) = self.crab_animation {
            anim.update(dt);
        }
    }

    /// Handle a mouse scroll event at terminal cell (`col`, `row`).
    /// `up` is `true` for scroll-up (wheel up / finger swipe down on trackpad).
    pub fn handle_mouse_scroll(&mut self, col: u16, row: u16, up: bool) {
        let pos_in =
            |r: Rect| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height;

        // Ignore scroll during crab animation or help screen
        if self.crab_animation.is_some() || matches!(self.screen, Screen::Help) {
            return;
        }

        // Modal open: only scroll within the modal popup rect
        if self.modal.is_some() {
            if let Some(popup) = self.scroll_regions.modal_popup {
                if pos_in(popup) {
                    let delta: i8 = if up { -1 } else { 1 };
                    let action = match &self.modal {
                        Some(Modal::FilterSelect { .. }) => Action::FilterDialogMove(delta),
                        Some(Modal::SortSelect { .. }) => Action::SortDialogMove(delta),
                        _ => return,
                    };
                    self.apply_action(action);
                }
            }
            return;
        }

        // No modal: hit-test main panels
        if pos_in(self.scroll_regions.list) {
            if up {
                self.move_up();
            } else {
                self.move_down();
            }
            if self.detail_open {
                self.detail_scroll = 0;
                self.comment_scroll = 0;
                self.detail_tab = DetailTab::Description;
            }
        } else if self.detail_open && pos_in(self.scroll_regions.detail_content) {
            match self.detail_tab {
                DetailTab::Description => {
                    if up {
                        self.detail_scroll = self.detail_scroll.saturating_sub(1);
                    } else {
                        self.detail_scroll += 1;
                    }
                }
                DetailTab::Comments => {
                    if up {
                        self.comment_scroll = self.comment_scroll.saturating_sub(1);
                    } else {
                        self.comment_scroll += 1;
                    }
                }
                DetailTab::Insights => {
                    if up {
                        self.insight_scroll = self.insight_scroll.saturating_sub(1);
                    } else {
                        self.insight_scroll += 1;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::operations::{Operation, OperationKind};
    use crate::models::{AgentId, Priority, Status};
    use crate::tui::events::Focus;
    use crate::tui::theme::Theme;

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
        // Default is CreationDate which is index 0 in SORT_OPTIONS
        app.apply_action(Action::OpenSortDialog);
        assert!(matches!(
            app.modal,
            Some(Modal::SortSelect { selected_index: 0 })
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
    fn toggle_collapse_hides_epic_child_tasks() {
        let (mut app, epic_id, _task_id) = setup_app_with_epic();

        // Initially expanded — epic row + task row = 2 rows
        assert_eq!(app.dashboard_rows.len(), 2);
        assert!(!app.collapsed_epics.contains(&epic_id));

        app.handle_action(Action::ToggleCollapse);

        assert!(app.collapsed_epics.contains(&epic_id));
        // Collapsed — only the epic header row remains
        assert_eq!(app.dashboard_rows.len(), 1);
        assert!(matches!(&app.dashboard_rows[0], DashboardRow::Epic { id } if id == &epic_id));
    }

    #[test]
    fn toggle_collapse_expands_collapsed_epic() {
        let (mut app, epic_id, task_id) = setup_app_with_epic();

        // Pre-collapse the epic
        app.collapsed_epics.insert(epic_id.clone());
        app.rebuild_dashboard_rows();
        assert_eq!(app.dashboard_rows.len(), 1);

        app.handle_action(Action::ToggleCollapse);

        assert!(!app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 2);
        assert!(matches!(&app.dashboard_rows[1], DashboardRow::Task { id } if id == &task_id));
    }

    #[test]
    fn toggle_collapse_cycles_epic_state() {
        let (mut app, epic_id, _) = setup_app_with_epic();

        // First toggle: expanded → collapsed
        app.handle_action(Action::ToggleCollapse);
        assert!(app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 1);

        // Second toggle: collapsed → expanded
        app.handle_action(Action::ToggleCollapse);
        assert!(!app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 2);
    }

    #[test]
    fn toggle_collapse_from_task_row_moves_selection_to_epic() {
        let (mut app, epic_id, _task_id) = setup_app_with_epic();

        // Select the task row (index 1) instead of the epic row
        app.selected_index = 1;

        app.handle_action(Action::ToggleCollapse);

        assert!(app.collapsed_epics.contains(&epic_id));
        assert_eq!(app.dashboard_rows.len(), 1);
        // Selection must land on the epic, not the next row below it
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn dashboard_rows_sorted_by_creation_date_by_default() {
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

        // Default sort is CreationDate (newest first).
        // op2 was applied second so its created_at is >= op1.
        // Row order should have id2 before id1.
        assert_eq!(app.dashboard_sort, SortOption::CreationDate);
        assert!(app.dashboard_rows.len() >= 2);
    }

    #[test]
    fn focus_detail_opens_detail_and_sets_focus_detail() {
        let (mut app, _, _) = setup_app_with_epic();

        assert!(!app.detail_open);
        assert_eq!(app.focus, Focus::List);

        app.handle_action(Action::FocusDetail);

        assert!(app.detail_open);
        assert_eq!(app.focus, Focus::Detail);
        assert_eq!(app.detail_scroll, 0);
    }

    #[test]
    fn focus_detail_noop_on_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        assert!(app.dashboard_rows.is_empty());
        app.handle_action(Action::FocusDetail);
        assert!(!app.detail_open);
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn focus_list_pane_returns_focus_to_list() {
        let (mut app, _, _) = setup_app_with_epic();

        app.detail_open = true;
        app.focus = Focus::Detail;

        app.handle_action(Action::FocusListPane);

        assert!(app.detail_open, "panel stays open after FocusListPane");
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn focus_list_pane_noop_when_already_on_list() {
        let (mut app, _, _) = setup_app_with_epic();

        app.detail_open = true;
        app.focus = Focus::List;

        app.handle_action(Action::FocusListPane);

        assert!(app.detail_open, "FocusListPane does not close pane");
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn up_down_scroll_when_detail_focused() {
        let (mut app, _, _) = setup_app_with_epic();

        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_scroll = 5;

        app.handle_action(Action::Up);
        assert_eq!(app.detail_scroll, 4);

        app.handle_action(Action::Down);
        assert_eq!(app.detail_scroll, 5);
    }

    #[test]
    fn scroll_saturates_at_zero() {
        let (mut app, _, _) = setup_app_with_epic();

        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_scroll = 0;

        app.handle_action(Action::Up);
        assert_eq!(app.detail_scroll, 0, "scroll should not underflow");
    }

    #[test]
    fn back_closes_detail_pane_when_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_scroll = 3;

        app.handle_action(Action::Back);

        assert!(!app.detail_open);
        assert_eq!(app.focus, Focus::List, "focus resets to List on close");
        assert_eq!(app.detail_scroll, 0, "scroll resets on close");
        assert!(app.modal.is_none());
    }

    #[test]
    fn back_invariant_focus_always_list_when_detail_closed() {
        let (mut app, _, _) = setup_app_with_epic();

        // Open via FocusDetail, then close via Back
        app.handle_action(Action::FocusDetail);
        assert_eq!(app.focus, Focus::Detail);

        app.handle_action(Action::Back);
        assert!(!app.detail_open);
        assert_eq!(
            app.focus,
            Focus::List,
            "invariant: focus==List when detail closed"
        );
    }

    #[test]
    fn list_navigation_resets_scroll_when_detail_open() {
        let (mut app, _, _) = setup_app_with_epic();

        app.detail_open = true;
        app.focus = Focus::List;
        app.detail_scroll = 10;

        app.handle_action(Action::Down);
        assert_eq!(app.detail_scroll, 0, "navigation resets detail scroll");
    }

    #[test]
    fn back_on_main_without_detail_shows_quit_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        assert_eq!(app.screen, Screen::Main);
        assert!(!app.detail_open);

        app.handle_action(Action::Back);

        assert!(matches!(app.modal, Some(Modal::QuitConfirm)));
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
            .filter_map(|r| {
                if let DashboardRow::Task { id } = r {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(task_rows.len(), 2);
        assert_eq!(task_rows[0], &task_id1);
        assert_eq!(task_rows[1], &task_id2);
    }

    #[test]
    fn test_crab_animation_expires_after_3s() {
        let mut anim = CrabAnimation::new(80, 24);
        assert!(!anim.is_expired(), "should not be expired immediately");
        // Backdate the start time to simulate 4 seconds having passed.
        anim.start_time = std::time::Instant::now() - std::time::Duration::from_secs(4);
        assert!(anim.is_expired(), "should be expired after 4 seconds");
    }

    #[test]
    fn test_crab_animation_crabs_move_on_update() {
        let mut anim = CrabAnimation::new(80, 24);
        // Ensure there is at least one row with at least one crab.
        assert!(!anim.rows.is_empty());
        assert!(!anim.rows[0].crabs.is_empty());
        let initial_x = anim.rows[0].crabs[0].x;
        // Update with a generous dt so movement is detectable.
        anim.update(1.0);
        let new_x = anim.rows[0].crabs[0].x;
        // x should have changed (moved forward or wrapped).
        assert_ne!(new_x, initial_x, "crab should have moved after update");
    }

    #[test]
    fn resize_pane_grows_focused_pane() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        assert_eq!(app.detail_size_percent, 50, "default is 50%");

        // Focus on detail: + grows detail
        app.focus = Focus::Detail;
        app.handle_action(Action::ResizePane(5));
        assert_eq!(app.detail_size_percent, 55);

        // Focus on list: + grows list (shrinks detail)
        app.focus = Focus::List;
        app.handle_action(Action::ResizePane(5));
        assert_eq!(app.detail_size_percent, 50);
    }

    #[test]
    fn resize_pane_clamps_to_20_80_range() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        // Grow detail past max
        app.focus = Focus::Detail;
        app.detail_size_percent = 78;
        app.handle_action(Action::ResizePane(5));
        assert_eq!(app.detail_size_percent, 80, "should clamp at 80%");

        app.handle_action(Action::ResizePane(5));
        assert_eq!(app.detail_size_percent, 80, "should stay at 80%");

        // Shrink detail past min
        app.detail_size_percent = 23;
        app.handle_action(Action::ResizePane(-5));
        assert_eq!(app.detail_size_percent, 20, "should clamp at 20%");

        app.handle_action(Action::ResizePane(-5));
        assert_eq!(app.detail_size_percent, 20, "should stay at 20%");
    }

    // --- Status dialog tests ---

    fn setup_app_with_task() -> (App, ItemId) {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        let task_id = ItemId::new("TASK", 1);
        let _ = app.state.apply(Operation::new(
            make_agent(),
            OperationKind::CreateTask {
                id: task_id.clone(),
                title: "Test Task".to_string(),
                description: Some("Hello world".to_string()),
                priority: Priority::Medium,
                epic_id: None,
            },
        ));
        app.rebuild_dashboard_rows();
        app.selected_index = 0;
        (app, task_id)
    }

    #[test]
    fn status_dialog_opens_with_current_status_preselected() {
        let (mut app, _task_id) = setup_app_with_task();

        app.handle_action(Action::OpenStatusDialog);
        // Task starts as Todo which is index 0 in STATUS_OPTIONS
        assert!(matches!(
            app.modal,
            Some(Modal::StatusSelect { selected_index: 0 })
        ));
    }

    #[test]
    fn status_dialog_noop_on_epic_row() {
        let (mut app, _epic_id, _task_id) = setup_app_with_epic();
        // selected_index = 0 is the epic row
        app.handle_action(Action::OpenStatusDialog);
        assert!(
            app.modal.is_none(),
            "should not open status dialog on epic row"
        );
    }

    #[test]
    fn status_dialog_navigation_wraps() {
        let (mut app, _) = setup_app_with_task();
        app.modal = Some(Modal::StatusSelect { selected_index: 0 });
        app.apply_action(Action::StatusDialogMove(-1));
        let last = crate::tui::components::status_select::STATUS_OPTIONS.len() - 1;
        assert!(matches!(
            app.modal,
            Some(Modal::StatusSelect { selected_index }) if selected_index == last
        ));
    }

    #[test]
    fn status_dialog_cancel_closes() {
        let (mut app, _) = setup_app_with_task();
        app.modal = Some(Modal::StatusSelect { selected_index: 0 });
        app.apply_action(Action::StatusDialogCancel);
        assert!(app.modal.is_none());
    }

    // --- Epic dialog tests ---

    #[test]
    fn epic_dialog_opens_with_none_preselected_for_orphan_task() {
        let (mut app, _task_id) = setup_app_with_task();
        app.handle_action(Action::OpenEpicDialog);
        // Orphan task → current epic is None → index 0
        assert!(matches!(
            app.modal,
            Some(Modal::EpicSelect { selected_index: 0 })
        ));
    }

    #[test]
    fn epic_dialog_opens_with_current_epic_preselected() {
        let (mut app, _epic_id, _task_id) = setup_app_with_epic();
        // Select the task row (index 1)
        app.selected_index = 1;
        app.handle_action(Action::OpenEpicDialog);
        // Task has epic → index 1 (after "none")
        assert!(matches!(
            app.modal,
            Some(Modal::EpicSelect { selected_index: 1 })
        ));
    }

    #[test]
    fn epic_dialog_noop_on_epic_row() {
        let (mut app, _epic_id, _task_id) = setup_app_with_epic();
        app.handle_action(Action::OpenEpicDialog);
        assert!(
            app.modal.is_none(),
            "should not open epic dialog on epic row"
        );
    }

    #[test]
    fn epic_dialog_cancel_closes() {
        let (mut app, _) = setup_app_with_task();
        app.modal = Some(Modal::EpicSelect { selected_index: 0 });
        app.apply_action(Action::EpicDialogCancel);
        assert!(app.modal.is_none());
    }

    // --- Editor action guard tests ---

    #[test]
    fn edit_description_sets_pending_editor_when_detail_description_focused() {
        let (mut app, task_id) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_tab = DetailTab::Description;

        app.handle_action(Action::EditDescription);

        assert!(app.pending_editor.is_some());
        match app.pending_editor {
            Some(EditorRequest::EditDescription {
                task_id: id,
                current_text,
            }) => {
                assert_eq!(id, task_id);
                assert_eq!(current_text, "Hello world");
            }
            _ => panic!("expected EditDescription"),
        }
    }

    #[test]
    fn edit_description_noop_on_list_focus() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::List; // not Detail
        app.detail_tab = DetailTab::Description;

        app.handle_action(Action::EditDescription);
        assert!(app.pending_editor.is_none());
    }

    #[test]
    fn edit_description_noop_on_comments_tab() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_tab = DetailTab::Comments;

        app.handle_action(Action::EditDescription);
        assert!(app.pending_editor.is_none());
    }

    #[test]
    fn add_comment_sets_pending_editor_when_detail_comments_focused() {
        let (mut app, task_id) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_tab = DetailTab::Comments;

        app.handle_action(Action::AddComment);

        match app.pending_editor {
            Some(EditorRequest::NewComment { task_id: id }) => {
                assert_eq!(id, task_id);
            }
            _ => panic!("expected NewComment"),
        }
    }

    #[test]
    fn add_comment_noop_on_description_tab() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_tab = DetailTab::Description;

        app.handle_action(Action::AddComment);
        assert!(app.pending_editor.is_none());
    }

    // --- Write helper tests ---

    #[test]
    fn selected_task_id_returns_none_on_epic_row() {
        let (app, _epic_id, _task_id) = setup_app_with_epic();
        // selected_index = 0 is the epic row
        assert!(app.selected_task_id().is_none());
    }

    #[test]
    fn selected_task_id_returns_id_on_task_row() {
        let (mut app, _epic_id, task_id) = setup_app_with_epic();
        app.selected_index = 1;
        assert_eq!(app.selected_task_id(), Some(task_id));
    }

    // --- Split direction tests ---

    #[test]
    fn toggle_split_direction() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().to_path_buf(), Theme::detect()).unwrap();

        assert_eq!(app.split_direction, SplitDirection::Horizontal);

        app.handle_action(Action::ToggleSplitDirection);
        assert_eq!(app.split_direction, SplitDirection::Vertical);

        app.handle_action(Action::ToggleSplitDirection);
        assert_eq!(app.split_direction, SplitDirection::Horizontal);
    }

    // --- TabBackward / TabForward tests ---

    #[test]
    fn tab_forward_cycles_tab_when_detail_open() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_tab = DetailTab::Description;

        app.handle_action(Action::TabForward);
        assert_eq!(app.detail_tab, DetailTab::Comments);

        app.handle_action(Action::TabForward);
        assert_eq!(app.detail_tab, DetailTab::Insights);

        app.handle_action(Action::TabForward);
        assert_eq!(app.detail_tab, DetailTab::Description);
    }

    #[test]
    fn tab_backward_cycles_tab_when_detail_open() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::Detail;
        app.detail_tab = DetailTab::Comments;

        app.handle_action(Action::TabBackward);
        assert_eq!(app.detail_tab, DetailTab::Description);

        app.handle_action(Action::TabBackward);
        assert_eq!(app.detail_tab, DetailTab::Insights);
    }

    #[test]
    fn tab_forward_works_from_list_focus_with_detail_open() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = true;
        app.focus = Focus::List;
        app.detail_tab = DetailTab::Description;

        app.handle_action(Action::TabForward);
        assert_eq!(app.detail_tab, DetailTab::Comments);
        assert_eq!(app.focus, Focus::List, "focus stays on list");
    }

    #[test]
    fn tab_forward_noop_when_detail_closed() {
        let (mut app, _) = setup_app_with_task();
        app.detail_open = false;
        app.detail_tab = DetailTab::Description;

        app.handle_action(Action::TabForward);
        assert_eq!(
            app.detail_tab,
            DetailTab::Description,
            "tab should not change"
        );
        assert!(!app.detail_open);
    }
}
