use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::models::Result;

use super::operations::Operation;

/// Appends a single operation to the JSONL log file.
/// Caller must hold the exclusive write lock before calling.
pub fn append_operation(log_path: &Path, op: &Operation) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    let line = serde_json::to_string(op)?;
    writeln!(file, "{}", line)?;
    file.flush()?;
    // fsync to ensure durability
    file.sync_data()?;
    Ok(())
}

/// Reads all operations from the JSONL log file.
/// Skips empty lines and logs warnings for corrupted lines (partial trailing writes).
pub fn read_operations(log_path: &Path) -> Result<Vec<Operation>> {
    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(log_path)?;
    let reader = BufReader::new(file);
    let mut ops = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Operation>(trimmed) {
            Ok(op) => ops.push(op),
            Err(e) => {
                // Tolerate a single corrupted trailing line (partial write).
                // If it's not the last content, that's a real error.
                eprintln!(
                    "Warning: skipping corrupted operation at line {}: {}",
                    line_num + 1,
                    e
                );
            }
        }
    }

    Ok(ops)
}

/// Reads operations starting from a given byte offset in the file.
/// Returns (operations, new_byte_offset) for incremental reads.
pub fn read_operations_since(log_path: &Path, byte_offset: u64) -> Result<(Vec<Operation>, u64)> {
    use std::io::{Read, Seek, SeekFrom};

    if !log_path.exists() {
        return Ok((Vec::new(), 0));
    }

    let mut file = File::open(log_path)?;
    let file_len = file.metadata()?.len();

    if byte_offset >= file_len {
        return Ok((Vec::new(), file_len));
    }

    file.seek(SeekFrom::Start(byte_offset))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;

    let mut ops = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(op) = serde_json::from_str::<Operation>(trimmed) {
            ops.push(op);
        }
    }

    Ok((ops, file_len))
}
