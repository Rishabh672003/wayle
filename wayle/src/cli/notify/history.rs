//! Notification history command.
//!
//! Reads the persistent history log written by the shell, so history survives
//! shell restarts and can be queried even when the shell is not running.

use chrono::{DateTime, Local};
use serde::Deserialize;
use wayle_core::paths::ConfigPaths;

use crate::cli::CliAction;

/// On-disk history log filename, under `ConfigPaths::state_dir()`.
const HISTORY_FILE: &str = "notification-history.jsonl";

/// One archived notification, as persisted by the shell (JSONL, oldest first).
#[derive(Deserialize)]
struct HistoryRecord {
    app_name: String,
    summary: String,
    body: String,
    timestamp: i64,
}

/// Executes the history command, showing the `count` most recent entries.
///
/// # Errors
/// Returns error if the state directory cannot be resolved.
pub async fn execute(count: usize) -> CliAction {
    let path = ConfigPaths::state_dir()
        .map_err(|e| format!("cannot resolve state directory: {e}"))?
        .join(HISTORY_FILE);

    let contents = std::fs::read_to_string(&path).unwrap_or_default();

    let records: Vec<HistoryRecord> = contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    if records.is_empty() {
        println!("No notification history");
        return Ok(());
    }

    println!("Notification history:");
    // File is oldest-first; show the most recent `count`, newest first.
    for record in records.iter().rev().take(count) {
        let time = DateTime::from_timestamp(record.timestamp, 0)
            .map(|dt| dt.with_timezone(&Local).format("%H:%M").to_string())
            .unwrap_or_default();

        println!("  {time}  {}: {}", record.app_name, record.summary);
        if !record.body.is_empty() {
            println!("        {}", record.body);
        }
    }

    Ok(())
}
