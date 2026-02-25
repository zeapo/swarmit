use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use swarmit_core::events::log::read_operations_since;
use swarmit_core::models::ItemId;
use swarmit_core::state::ProjectState;

use crate::events::{Action, Screen};

/// Central application state.
pub struct App {
    pub state: ProjectState,
    pub screen: Screen,
    pub project_root: PathBuf,

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
}

impl App {
    pub fn new(project_root: PathBuf) -> Result<Self> {
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

        Ok(App {
            state,
            screen: Screen::Dashboard,
            project_root,
            selected_index: 0,
            watcher_rx: Some(rx),
            _debouncer: Some(debouncer),
            log_offset,
            log_path,
            should_quit: false,
            search_query: String::new(),
        })
    }

    /// Process an action and update state accordingly.
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Back => self.go_back(),
            Action::Up => self.move_up(),
            Action::Down => self.move_down(),
            Action::Select => self.select_item(),
            Action::GotoDashboard => {
                self.screen = Screen::Dashboard;
                self.selected_index = 0;
            }
            Action::GotoBacklog => {
                self.screen = Screen::Board {
                    epic_id: "_backlog".parse().unwrap_or_else(|_| {
                        // Sentinel: we use epic_id="" to mean backlog
                        // This is handled in board rendering
                        ItemId::new("BACK", 0)
                    }),
                };
                self.selected_index = 0;
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
            _ => {}
        }
    }

    fn go_back(&mut self) {
        self.screen = match &self.screen {
            Screen::Help => Screen::Dashboard,
            Screen::TaskDetail { .. } => Screen::Dashboard,
            Screen::Board { .. } => Screen::Dashboard,
            Screen::Activity => Screen::Dashboard,
            Screen::Dashboard => Screen::Dashboard,
        };
        self.selected_index = 0;
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
                // Select an epic to view its board
                let epics: Vec<_> = self.state.epics.keys().cloned().collect();
                if let Some(epic_id) = epics.get(self.selected_index) {
                    self.screen = Screen::Board {
                        epic_id: epic_id.clone(),
                    };
                    self.selected_index = 0;
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
            Screen::Dashboard => self.state.epics.len(),
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
            }
        }
    }
}
