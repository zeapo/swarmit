pub mod app;
pub mod components;
pub mod editor;
pub mod events;
pub mod theme;

#[cfg(test)]
mod test_harness;

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Frame, Terminal,
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
        tracing_subscriber::registry().with(chrome_layer).init();
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

    // Eagerly deserialize bat's bundled SyntaxSet (~989 KB bincode) so that
    // the first task detail open has no visible stall.
    components::detail_pane::warm_up_syntax();

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

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    let mut last_frame = Instant::now();

    loop {
        let _frame_guard = prof_guard!("frame");

        // Compute dt for smooth animation regardless of frame rate.
        let now = Instant::now();
        let dt = now.duration_since(last_frame).as_secs_f32();
        last_frame = now;

        // Handle deferred editor requests before drawing.
        if let Some(req) = app.pending_editor.take() {
            handle_editor_request(terminal, app, req)?;
        }

        // Refresh the highlight cache before borrowing `app` immutably for the
        // draw closure.  This runs bat highlighting at most once per content
        // change rather than once per frame (~10 FPS).
        {
            let _g = prof_guard!("refresh_highlight_cache");
            app.refresh_highlight_cache();
        }

        // Draw frame
        {
            let _g = prof_guard!("terminal_draw");
            terminal.draw(|f| render_frame(f, app))?;
        }

        // Poll for keyboard events (100ms timeout = ~10fps)
        let has_event = {
            let _g = prof_guard!("event_poll");
            event::poll(Duration::from_millis(100))?
        };

        if has_event {
            if let Event::Key(key) = event::read()? {
                if handle_key(app, key.code, key.modifiers) {
                    return Ok(());
                }
            }
        }

        // Advance and auto-expire the crab animation.
        app.update_crab_animation(dt);
        if app
            .crab_animation
            .as_ref()
            .map_or(false, |a| a.is_expired())
        {
            app.crab_animation = None;
        }

        // Poll file watcher for live refresh
        {
            let _g = prof_guard!("poll_log_changes");
            app.poll_log_changes();
        }
    }
}

/// Render one full frame of the TUI into the given `Frame`.
///
/// Extracted from `run_loop` so that tests can call it with a `TestBackend`.
pub(crate) fn render_frame(f: &mut Frame, app: &App) {
    let size = f.area();

    // Reserve bottom row for status bar
    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(size);

    let main_area = chunks[0];
    let status_area = chunks[1];

    // Render current screen
    match &app.screen {
        Screen::Main => {
            if app.detail_open {
                let left_pct = 100 - app.detail_width_percent;
                let split = Layout::horizontal([
                    Constraint::Percentage(left_pct),
                    Constraint::Percentage(app.detail_width_percent),
                ])
                .split(main_area);
                components::tree_list::render(f, app, split[0]);
                // Right side: 1-row breadcrumb + detail pane content
                let right = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
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
            Modal::QuitConfirm => components::quit_confirm::render(f, &app.theme, main_area),
            Modal::TaskCreate { .. } => components::task_create::render(f, app, main_area),
            Modal::FilterSelect { selected_index } => {
                components::filter_select::render(f, app, *selected_index, main_area)
            }
            Modal::SortSelect { selected_index } => {
                components::sort_select::render(f, app, *selected_index, main_area)
            }
            Modal::StatusSelect { selected_index } => {
                components::status_select::render(f, app, *selected_index, main_area)
            }
            Modal::EpicSelect { selected_index } => {
                components::epic_select::render(f, app, *selected_index, main_area)
            }
        }
    }

    // Crab parade overlay — rendered on top of all content.
    if let Some(ref anim) = app.crab_animation {
        if anim.active {
            components::crab_parade::render(f, anim, size, &app.theme);
        }
    }

    components::status_bar::render(f, app, status_area);
}

/// Process a single key event through the full input pipeline.
///
/// Returns `true` if the application should quit. Covers: Ctrl+C immediate
/// quit, crab animation dismissal, Konami sequence tracking, modal routing,
/// and normal action dispatch.
pub(crate) fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    // Ctrl+C always quits immediately (no dialog)
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        return true;
    }

    {
        let _g = prof_guard!("handle_key");
        if app.crab_animation.is_some() {
            // Any keypress dismisses the crab parade.
            app.crab_animation = None;
        } else {
            // Feed key to Konami tracker before normal dispatch.
            if app.konami_tracker.feed(code) {
                app.start_crab_animation();
            } else if app.modal.is_some() {
                app.handle_modal_key(code, modifiers);
            } else {
                let action = key_to_action(code, modifiers);
                app.handle_action(action);
            }
        }
    }

    app.should_quit
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
        KeyCode::Char('<') => Action::ResizeDetail(-5),
        KeyCode::Char('>') => Action::ResizeDetail(5),
        KeyCode::Tab => Action::SwitchDetailTab,
        KeyCode::Char('S') => Action::OpenStatusDialog,
        KeyCode::Char('E') => Action::OpenEpicDialog,
        KeyCode::Char('e') => Action::EditDescription,
        KeyCode::Char('a') => Action::AddComment,
        _ => Action::None,
    }
}

/// Process a deferred editor request: suspend the TUI, open the editor,
/// and write the resulting operation if the content changed.
fn handle_editor_request(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    req: app::EditorRequest,
) -> Result<()> {
    match req {
        app::EditorRequest::EditDescription {
            task_id,
            current_text,
        } => {
            if let Some(new_text) = editor::open_editor_with(terminal, &current_text, "md")? {
                if new_text != current_text {
                    if let Err(e) = app.submit_description_update(task_id, new_text) {
                        // Surface error briefly — next key press clears it
                        eprintln!("swarmit: {}", e);
                    }
                }
            }
        }
        app::EditorRequest::NewComment { task_id } => {
            if let Some(text) = editor::open_editor_with(terminal, "", "md")? {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    if let Err(e) = app.submit_add_comment(task_id, trimmed.to_string()) {
                        eprintln!("swarmit: {}", e);
                    }
                }
            }
        }
    }
    Ok(())
}
