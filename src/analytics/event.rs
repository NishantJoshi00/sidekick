//! Event schema for sidekick analytics.
//!
//! Events are append-only records of things that happened — every mutation hook
//! decision, every buffer refresh, every nvim launch. The schema is designed to
//! be stable: once written, a line should remain readable across future versions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    HookDecision(HookDecision),
    BufferRefresh(BufferRefresh),
    NvimLaunch(NvimLaunch),
    /// Logged every time the user opens `sidekick stats`. Powers the
    /// meta-observations in the renderer (e.g. "fourth look today").
    StatsView(StatsView),
}

impl Event {
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Event::HookDecision(e) => e.at,
            Event::BufferRefresh(e) => e.at,
            Event::NvimLaunch(e) => e.at,
            Event::StatsView(e) => e.at,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Edit,
    Write,
    MultiEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    /// No nvim sockets matched the cwd hash. Hook degrades to allow.
    NoNvimRunning,
    /// RPC to nvim failed. Hook degrades to allow rather than block the user.
    StatusCheckFailed,
    /// File is open as the current buffer and has unsaved changes. The save.
    BufferDirtyAndCurrent,
    /// File was checked against nvim but was not dirty-and-current. Allowed.
    BufferAvailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDecision {
    pub at: DateTime<Utc>,
    pub session_id: String,
    pub cwd: String,
    pub tool: ToolKind,
    pub file: String,
    pub decision: Decision,
    pub reason: DecisionReason,
    pub instances_probed: usize,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferRefresh {
    pub at: DateTime<Utc>,
    pub session_id: String,
    pub cwd: String,
    pub tool: ToolKind,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvimLaunch {
    pub at: DateTime<Utc>,
    pub pid: u32,
    pub cwd: String,
    pub socket_path: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsView {
    pub at: DateTime<Utc>,
    pub range: String,
}
