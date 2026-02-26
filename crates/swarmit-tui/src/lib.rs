pub mod app;
pub mod components;
pub mod events;
pub mod theme;

use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Terminal,
};

use app::App;
use events::{Action, Modal, Screen};
use theme::Theme;

/// Creates a tracing span guard. Compiles to nothing without `profiling`.
macro_rules! prof_guard {
    ($name:expr) => {{
        #[cfg(feature = "profiling")]
        let _guard = ::tracing::info_span!($name).entered();
        #[cfg(not(feature = "profiling"))]
        let _guard = ();
        _guard
    }};
}
pub(crate) use prof_guard;

/// Entry point for the TUI.
pub fn run(project_root: &Path) -> Result<()> {
    // Detect theme BEFORE entering raw mode — terminal-colorsaurus sends OSC
    // escape sequences that require cooked mode for the terminal to respond.
    let theme = Theme::detect();

    // Initialize Chrome trace subscriber (no-op without `profiling` feature).
    #[cfg(feature = "profiling")]
    let trace_path = format!(
        "/tmp/swarmit-trace-{}.json",
        ::chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    #[cfg(feature = "profiling")]
    let _flush_guard = {
        use tracing_subscriber::prelude::*;
        let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file(&trace_path)
            .build();
        tracing_subscriber::registry()
            .with(chrome_layer)
            .init();
        guard
    };

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Application state
    let mut app = App::new(project_root.to_path_buf(), theme).map_err(|e| {
        // Restore terminal before propagating error
        let _ = restore_terminal();
        e
    })?;

    let result = run_loop(&mut terminal, &mut app);

    // Always restore terminal
    restore_terminal()?;

    // Flush trace file and report its path (no-op without `profiling` feature).
    #[cfg(feature = "profiling")]
    {
        drop(_flush_guard);
        eprintln!("Trace written to: {}", trace_path);
    }

    result
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        let _frame_guard = prof_guard!("frame");

        // Draw frame
        {
            let _g = prof_guard!("terminal_draw");
            terminal.draw(|f| {
                let size = f.area();

                // Reserve bottom row for status bar
                let chunks =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(size);

                let main_area = chunks[0];
                let status_area = chunks[1];

                // Render current screen
                match &app.screen {
                    Screen::Main => {
                        if app.detail_open {
                            let split = Layout::horizontal([
                                Constraint::Length(30),
                                Constraint::Min(40),
                            ])
                            .split(main_area);
                            components::tree_list::render(f, app, split[0]);
                            // Right side: 1-row breadcrumb + detail pane content
                            let right = Layout::vertical([
                                Constraint::Length(1),
                                Constraint::Min(0),
                            ])
                            .split(split[1]);
                            components::detail_pane::render_breadcrumb(f, app, right[0]);
                            components::detail_pane::render(f, app, right[1]);
                        } else {
                            components::tree_list::render(f, app, main_area);
                        }
                    }
                    Screen::Help => {
                        // Render tree underneath help overlay
                        components::tree_list::render(f, app, main_area);
                        components::help::render(f, &app.theme, main_area);
                    }
                }

                // Render modal overlay (if any) on top of the current screen
                if let Some(modal) = &app.modal {
                    match modal {
                        Modal::QuitConfirm => {
                            components::quit_confirm::render(f, &app.theme, main_area)
                        }
                        Modal::TaskCreate { .. } => {
                            components::task_create::render(f, app, main_area)
                        }
                        Modal::FilterSelect { selected_index } => {
                            components::filter_select::render(
                                f,
                                app,
                                *selected_index,
                                main_area,
                            )
                        }
                        Modal::SortSelect { selected_index } => {
                            components::sort_select::render(f, app, *selected_index, main_area)
                        }
                    }
                }

                components::status_bar::render(f, app, status_area);
            })?;
        }

        // Poll for keyboard events (100ms timeout = ~10fps)
        let has_event = {
            let _g = prof_guard!("event_poll");
            event::poll(Duration::from_millis(100))?
        };

        if has_event {
            if let Event::Key(key) = event::read()? {
                // Ctrl+C always quits immediately (no dialog)
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    return Ok(());
                }
                {
                    let _g = prof_guard!("handle_key");
                    // When a modal is open, route all input to the modal handler
                    if app.modal.is_some() {
                        app.handle_modal_key(key.code, key.modifiers);
                    } else {
                        let action = key_to_action(key.code, key.modifiers);
                        app.handle_action(action);
                    }
                }

                if app.should_quit {
                    return Ok(());
                }
            }
        }

        // Poll file watcher for live refresh
        {
            let _g = prof_guard!("poll_log_changes");
            app.poll_log_changes();
        }
    }
}

fn key_to_action(code: KeyCode, _modifiers: KeyModifiers) -> Action {
    match code {
        KeyCode::Char('q') => Action::QuitRequest,
        KeyCode::Char('n') => Action::NewTask,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Enter | KeyCode::Char('l') => Action::FocusRight,
        KeyCode::Char(' ') => Action::ToggleCollapse,
        KeyCode::Char('h') => Action::FocusLeft,
        KeyCode::Char('f') => Action::OpenFilterDialog,
        KeyCode::Char('s') => Action::OpenSortDialog,
        KeyCode::Esc => Action::Back,
        KeyCode::Char('?') => Action::Help,
        KeyCode::Char('/') => Action::Search,
        KeyCode::Char('r') => Action::Refresh,
        _ => Action::None,
    }
}
