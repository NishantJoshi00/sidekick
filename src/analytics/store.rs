//! Append-only JSONL event store.
//!
//! Concurrency model:
//! - Files are opened with `O_APPEND` so the kernel atomically performs
//!   seek-to-end + write as one operation. Two appenders cannot overwrite
//!   each other's bytes.
//! - Each event is pre-serialized into a single `Vec<u8>` (including the
//!   trailing newline) and then handed to a single `write_all` call. On
//!   Linux/macOS, regular-file writes under `PIPE_BUF` (4 KiB) do not
//!   interleave in practice, and our events are a few hundred bytes max.
//! - Writes are best-effort: failures never propagate to callers. The hook
//!   handler must not be broken by a disk error.
//! - Reads are tolerant: malformed lines are skipped, not fatal.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use crate::analytics::event::Event;

/// Resolve the events log path. Honors `SIDEKICK_EVENTS_PATH` for testability.
pub fn log_path() -> PathBuf {
    if let Ok(custom) = std::env::var("SIDEKICK_EVENTS_PATH") {
        return PathBuf::from(custom);
    }
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    base.join("sidekick").join("events.jsonl")
}

/// Append an event to the log. Never panics, never propagates errors.
///
/// If writing fails (no disk, permission denied, etc.), the event is silently
/// dropped. Analytics must not block the user's tool flow.
pub fn append(event: &Event) {
    let _ = try_append(event);
}

fn try_append(event: &Event) -> anyhow::Result<()> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&line)?;
    Ok(())
}

/// Read every event in the log. Malformed lines are silently skipped.
///
/// Returns an empty vec if the log doesn't exist yet (cold start case).
pub fn read_all() -> anyhow::Result<Vec<Event>> {
    let path = log_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let mut events = Vec::with_capacity(content.lines().count());
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<Event>(line) {
            events.push(event);
        }
    }
    Ok(events)
}
