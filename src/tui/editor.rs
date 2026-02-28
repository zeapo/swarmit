use std::io;
use std::process::Command;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

/// Resolve the editor command: $VISUAL → $EDITOR → "vi".
fn resolve_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
}

/// Open an external editor with the given content pre-filled.
///
/// Suspends the TUI (raw mode off, alternate screen off), spawns the editor,
/// waits for it to exit, then restores the TUI.
///
/// Returns `Some(new_text)` if the editor exited successfully (even if
/// unchanged — caller should compare). Returns `None` on non-zero exit
/// or spawn failure.
pub fn open_editor_with(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    content: &str,
    extension: &str,
) -> Result<Option<String>> {
    // Write content to a temp file.
    let tmp_dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    let filename = format!(
        "swarmit-edit-{}.{}",
        uuid::Uuid::now_v7(),
        extension
    );
    let tmp_path = std::path::PathBuf::from(&tmp_dir).join(&filename);
    std::fs::write(&tmp_path, content)?;

    // Suspend the TUI.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    // Spawn the editor.
    let editor = resolve_editor();
    let status = Command::new(&editor).arg(&tmp_path).status();

    // Restore the TUI regardless of editor outcome.
    // Each step runs independently so a failure in one doesn't prevent the rest.
    let r1 = enable_raw_mode();
    let r2 = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture);
    let r3 = terminal.clear();
    r1?;
    r2?;
    r3?;

    let result = match status {
        Ok(exit) if exit.success() => {
            let new_text = std::fs::read_to_string(&tmp_path)?;
            Some(new_text)
        }
        _ => None,
    };

    // Clean up temp file.
    let _ = std::fs::remove_file(&tmp_path);

    Ok(result)
}
