//! Data structures for Claude Code hook protocol.
//!
//! This module defines the JSON structures for communicating with Claude Code's
//! hook system. It supports PreToolUse and PostToolUse hooks with discriminated
//! union types for different tools (Read, Write, Edit, MultiEdit, Bash).
//!
//! # Hook Protocol
//!
//! Claude Code sends a JSON payload via stdin with:
//! - Hook metadata (session_id, cwd, etc.)
//! - Hook event type (PreToolUse or PostToolUse)
//! - Tool information (name and input parameters)
//!
//! The hook handler responds with a JSON payload via stdout containing:
//! - Permission decision (Allow, Deny, Ask) for PreToolUse
//! - Additional context or system messages
//!
//! # Example
//!
//! ```no_run
//! use sidekick::hook::{parse_hook, HookOutput, PermissionDecision};
//!
//! let json = r#"{"session_id":"abc","transcript_path":"","cwd":".","hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"test.txt"}}"#;
//! let hook = parse_hook(json).unwrap();
//!
//! let output = HookOutput::new()
//!     .with_permission_decision(PermissionDecision::Deny, Some("File has unsaved changes".into()));
//!
//! println!("{}", output.to_json().unwrap());
//! ```

use anyhow::Context;

/// Hook event type
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
}

/// Hook input structure
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Hook {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    pub hook_event_name: HookEvent,
    #[serde(flatten)]
    pub tool: Tool,
}

/// Tool types discriminated by tool_name
#[non_exhaustive]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tool_name", content = "tool_input")]
pub enum Tool {
    Read(FileToolInput),
    Write(FileToolInput),
    Edit(FileToolInput),
    MultiEdit(FileToolInput),
    Bash(BashToolInput),
}

/// File operation tool input
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FileToolInput {
    pub file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_string: Option<String>,
}

/// Bash tool input
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BashToolInput {
    pub command: String,
    pub description: String,
}

pub fn parse_hook(input: &str) -> anyhow::Result<Hook> {
    serde_json::from_str(input).context("Invalid JSON for Hook")
}

/// Permission decision for PreToolUse hooks
#[non_exhaustive]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecision {
    Allow,
    Deny,
    Ask,
}

/// Hook-specific output
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    pub hook_event_name: String,
    // PreToolUse fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<PermissionDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    // PostToolUse fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

/// Hook output response
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub continue_execution: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    // PostToolUse fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

impl HookOutput {
    /// Create a new hook output with default values
    pub fn new() -> Self {
        Self {
            continue_execution: None,
            stop_reason: None,
            suppress_output: None,
            system_message: None,
            decision: None,
            reason: None,
            hook_specific_output: None,
        }
    }

    /// Set continue execution flag
    #[allow(dead_code)]
    pub fn with_continue(mut self, continue_execution: bool) -> Self {
        self.continue_execution = Some(continue_execution);
        self
    }

    /// Set stop reason
    #[allow(dead_code)]
    pub fn with_stop_reason(mut self, reason: impl Into<String>) -> Self {
        self.stop_reason = Some(reason.into());
        self
    }

    /// Set suppress output flag
    #[allow(dead_code)]
    pub fn with_suppress_output(mut self, suppress: bool) -> Self {
        self.suppress_output = Some(suppress);
        self
    }

    /// Set system message
    #[allow(dead_code)]
    pub fn with_system_message(mut self, message: impl Into<String>) -> Self {
        self.system_message = Some(message.into());
        self
    }

    /// Set PreToolUse permission decision
    pub fn with_permission_decision(
        mut self,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> Self {
        self.hook_specific_output = Some(HookSpecificOutput {
            hook_event_name: "PreToolUse".to_string(),
            permission_decision: Some(decision),
            permission_decision_reason: reason,
            additional_context: None,
        });
        self
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).context("Failed to serialize HookOutput")
    }

    /// Convert to pretty JSON string
    #[allow(dead_code)]
    pub fn to_json_pretty(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self).context("Failed to serialize HookOutput")
    }
}

impl Default for HookOutput {
    fn default() -> Self {
        Self::new()
    }
}
