use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{Epic, ItemId, Result, Task};

/// Writes an epic and its tasks to the materialized markdown directory.
pub fn materialize_epic(state_dir: &Path, epic: &Epic, tasks: &[&Task]) -> Result<()> {
    let epic_dir = epic_dir_path(state_dir, epic);
    fs::create_dir_all(&epic_dir)?;
    write_epic_file(&epic_dir, epic)?;
    for task in tasks {
        write_task_file(&epic_dir, task)?;
    }
    Ok(())
}

/// Writes a backlog task (no epic) to the backlog directory.
pub fn materialize_backlog_task(state_dir: &Path, task: &Task) -> Result<()> {
    let backlog_dir = state_dir.join("backlog");
    fs::create_dir_all(&backlog_dir)?;
    write_task_file(&backlog_dir, task)?;
    Ok(())
}

/// Writes a done/cancelled backlog task to the archive directory.
pub fn materialize_archived_task(state_dir: &Path, task: &Task) -> Result<()> {
    let archive_dir = state_dir.join("archive");
    fs::create_dir_all(&archive_dir)?;
    write_task_file(&archive_dir, task)?;
    Ok(())
}

/// Removes an epic's directory and all markdown files within it.
pub fn remove_epic(state_dir: &Path, epic: &Epic) -> Result<()> {
    let dir = epic_dir_path(state_dir, epic);
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Removes a single task's markdown file from its location (epic dir, backlog, or archive).
pub fn remove_task_file(state_dir: &Path, task_id: &ItemId, epic: Option<&Epic>) -> Result<()> {
    match epic {
        Some(e) => {
            let path = epic_dir_path(state_dir, e).join(format!("{}.md", task_id));
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        None => {
            // Could be in backlog/ or archive/ — clean up both
            for dir_name in ["backlog", "archive"] {
                let path = state_dir.join(dir_name).join(format!("{}.md", task_id));
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}

fn epic_dir_path(state_dir: &Path, epic: &Epic) -> PathBuf {
    let slug = slugify(&epic.title);
    state_dir
        .join("epics")
        .join(format!("{}-{}", epic.id, slug))
}

fn write_epic_file(dir: &Path, epic: &Epic) -> Result<()> {
    let path = dir.join("epic.md");
    let content = format_epic_markdown(epic);
    fs::write(path, content)?;
    Ok(())
}

fn write_task_file(dir: &Path, task: &Task) -> Result<()> {
    let path = dir.join(format!("{}.md", task.id));
    let content = format_task_markdown(task);
    fs::write(path, content)?;
    Ok(())
}

fn format_epic_markdown(epic: &Epic) -> String {
    let mut buf = String::new();
    buf.push_str("---\n");
    buf.push_str(&format!("id: {}\n", epic.id));
    buf.push_str(&format!("title: {}\n", epic.title));
    buf.push_str(&format!("status: {}\n", epic.status));
    buf.push_str(&format!("priority: {}\n", epic.priority));
    buf.push_str(&format!("created_by: {}\n", epic.created_by));
    buf.push_str(&format!(
        "created_at: {}\n",
        epic.created_at.to_rfc3339()
    ));
    buf.push_str(&format!(
        "updated_at: {}\n",
        epic.updated_at.to_rfc3339()
    ));
    if let Some(a) = &epic.assignee {
        buf.push_str(&format!("assignee: {}\n", a));
    }
    buf.push_str("---\n\n");
    buf.push_str(&format!("# {}\n\n", epic.title));
    if let Some(desc) = &epic.description {
        buf.push_str(desc);
        buf.push('\n');
    }
    buf
}

fn format_task_markdown(task: &Task) -> String {
    let mut buf = String::new();
    buf.push_str("---\n");
    buf.push_str(&format!("id: {}\n", task.id));
    buf.push_str(&format!("title: {}\n", task.title));
    buf.push_str(&format!("status: {}\n", task.status));
    buf.push_str(&format!("priority: {}\n", task.priority));
    buf.push_str(&format!("created_by: {}\n", task.created_by));
    buf.push_str(&format!(
        "created_at: {}\n",
        task.created_at.to_rfc3339()
    ));
    buf.push_str(&format!(
        "updated_at: {}\n",
        task.updated_at.to_rfc3339()
    ));
    if let Some(eid) = &task.epic_id {
        buf.push_str(&format!("epic_id: {}\n", eid));
    }
    if let Some(a) = &task.assignee {
        buf.push_str(&format!("assignee: {}\n", a));
    }
    if let Some(t) = task.claimed_at {
        buf.push_str(&format!("claimed_at: {}\n", t.to_rfc3339()));
    }
    if let Some(t) = task.completed_at {
        buf.push_str(&format!("completed_at: {}\n", t.to_rfc3339()));
    }
    buf.push_str("---\n\n");
    buf.push_str(&format!("# {}\n\n", task.title));
    if let Some(desc) = &task.description {
        buf.push_str(desc);
        buf.push('\n');
    }
    buf
}

/// Very simple slugifier: lowercase, replace spaces/special chars with dashes, collapse dashes.
fn slugify(s: &str) -> String {
    let slug: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive dashes and trim leading/trailing
    let mut result = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash && !result.is_empty() {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }
    result.trim_end_matches('-').to_string()
}
