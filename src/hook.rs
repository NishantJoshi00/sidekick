//! Data structures for Claude/Codex-compatible hook protocol.
//!
//! This module defines the JSON structures for communicating with Claude Code,
//! Codex, and bridge plugins. It supports PreToolUse and PostToolUse hooks with
//! known tool types and a generic fallback for tool shapes sidekick does not
//! need to understand.
//!
//! # Hook Protocol
//!
//! The host sends a JSON payload via stdin with:
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

/// Top-level hook envelope, discriminated by wire shape.
///
/// `Claude` covers the wire-identical envelopes from Claude Code, Codex,
/// and the opencode/pi bridges — all carry a `hook_event_name` field.
/// `Crush` is Crush's distinct envelope, which uses `event` instead and
/// omits `transcript_path`. `Antigravity` has no top-level event tag at
/// all (the event is implicit in the `hooks.json` registration), uses
/// camelCase fields, and nests the tool call. `Grok` uses camelCase field
/// names (`hookEventName`, `sessionId`, `toolName`, `toolInput`) with
/// snake_case event-tag values (`pre_tool_use` / `post_tool_use` /
/// `user_prompt_submit`). Untagged matching tries the more-specific
/// variants (those with a distinguishing tag/field set) first; Grok is
/// last and only matches its own camelCase-tagged envelope.
#[non_exhaustive]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Hook {
    Claude(ClaudeShape),
    Crush(CrushShape),
    Antigravity(AntigravityShape),
    Grok(GrokShape),
}

/// Claude-shape envelopes — internally tagged on `hook_event_name`.
#[non_exhaustive]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "hook_event_name")]
pub enum ClaudeShape {
    PreToolUse(ClaudeToolHook),
    PostToolUse(ClaudeToolHook),
    UserPromptSubmit(ClaudeUserPromptHook),
}

/// Body of a Claude-shape PreToolUse / PostToolUse envelope.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ClaudeToolHook {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub cwd: String,
    #[serde(flatten)]
    pub tool: Tool,
}

/// Body of a Claude-shape UserPromptSubmit envelope. No tool payload —
/// this fires before a tool is chosen, on the user's prompt itself.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ClaudeUserPromptHook {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Crush-shape envelopes — internally tagged on `event`. Crush emits
/// only `PreToolUse` today; the type encodes that upstream constraint.
#[non_exhaustive]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event")]
pub enum CrushShape {
    PreToolUse(CrushToolHook),
}

/// Body of a Crush PreToolUse envelope.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CrushToolHook {
    pub session_id: String,
    pub cwd: String,
    #[serde(flatten)]
    pub tool: Tool,
}

/// Antigravity's hook envelope. The wire has no top-level event-name
/// field; the discriminator is structural — PreToolUse payloads carry a
/// `toolCall: { name, args }` object, PostToolUse payloads do not. Field
/// names are camelCase, tool argument keys are PascalCase.
#[non_exhaustive]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AntigravityShape {
    PreToolUse(AntigravityToolHook),
    PostToolUse(AntigravityPostHook),
}

/// Antigravity PreToolUse body — has the nested `toolCall`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityToolHook {
    pub conversation_id: String,
    pub workspace_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_directory_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_idx: Option<i64>,
    pub tool_call: AntigravityToolCall,
}

/// Antigravity PostToolUse body — no `toolCall`, carries `stepIdx` and an
/// optional `error` for the completed step.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityPostHook {
    pub conversation_id: String,
    pub workspace_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_directory_path: Option<String>,
    pub step_idx: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Nested tool call inside an Antigravity PreToolUse envelope. `name` is
/// the snake_case tool name (`write_to_file`, `replace_file_content`,
/// `multi_replace_file_content`); `args` keys are PascalCase
/// (`TargetFile`).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AntigravityToolCall {
    pub name: String,
    pub args: serde_json::Value,
}

/// Grok's hook envelopes — internally tagged on `hookEventName` with
/// snake_case values. Grok emits Pre/Post tool-use and prompt-submit
/// events, so the capability triple is full (like Claude Code).
#[non_exhaustive]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "hookEventName")]
pub enum GrokShape {
    #[serde(rename = "pre_tool_use")]
    PreToolUse(GrokToolHook),
    #[serde(rename = "post_tool_use")]
    PostToolUse(GrokToolHook),
    #[serde(rename = "user_prompt_submit")]
    UserPromptSubmit(GrokUserPromptHook),
}

/// Body of a Grok pre/post tool-use envelope. `toolName` is Grok's
/// snake_case tool name (`search_replace` for edits, `write_file` for
/// writes); `toolInput` carries the arguments (path key is `file_path`).
/// The many other fields Grok sends — `workspaceRoot`, `timestamp`,
/// `toolUseId`, `effectiveToolName`, `toolResult`, `toolInputTruncated`,
/// `isBackgrounded` — are ignored.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokToolHook {
    pub session_id: String,
    pub cwd: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

/// Body of a Grok user_prompt_submit envelope. No tool payload — this
/// fires on the user's prompt itself, before a tool is chosen.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokUserPromptHook {
    pub session_id: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Tool types recognised by name. Adjacent tagging matches the wire
/// shape exactly: `tool_name` selects the variant, `tool_input` carries
/// the payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "tool_name", content = "tool_input")]
pub enum KnownTool {
    Read(FileToolInput),
    Write(FileToolInput),
    Edit(FileToolInput),
    MultiEdit(FileToolInput),
    Bash(BashToolInput),
}

/// Tool envelope flattened into each hook body. `Known` covers named
/// tools; `Other` is the catch-all that carries any other
/// `tool_name`/`tool_input` pair (Codex's `apply_patch`, Crush's
/// lowercase `edit`/`write`, MCP-prefixed tools) so the handler can do
/// its own normalisation.
#[non_exhaustive]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Tool {
    Known(KnownTool),
    Other {
        tool_name: String,
        tool_input: serde_json::Value,
    },
}

/// File operation tool input
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BashToolInput {
    pub command: String,
    pub description: String,
}

pub fn parse_hook(input: &str) -> anyhow::Result<Hook> {
    serde_json::from_str(input).context("couldn't parse hook input")
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

    /// Set Antigravity-shape decision: a top-level `decision` string
    /// (`"allow"` / `"deny"` / `"ask"` / `"force_ask"`) plus an optional
    /// top-level `reason`. Antigravity reads the gating decision from the
    /// top of the JSON, not from a nested `hookSpecificOutput`, so emitting
    /// only the Claude-shape form means the host silently treats the call
    /// as allowed.
    pub fn with_top_level_decision(
        mut self,
        decision: impl Into<String>,
        reason: Option<String>,
    ) -> Self {
        self.decision = Some(decision.into());
        self.reason = reason;
        self
    }

    /// Set additional context for UserPromptSubmit
    pub fn with_additional_context(mut self, context: impl Into<String>) -> Self {
        self.hook_specific_output = Some(HookSpecificOutput {
            hook_event_name: "UserPromptSubmit".to_string(),
            permission_decision: None,
            permission_decision_reason: None,
            additional_context: Some(context.into()),
        });
        self
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).context("couldn't serialize hook output")
    }

    /// Convert to pretty JSON string
    #[allow(dead_code)]
    pub fn to_json_pretty(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self).context("couldn't serialize hook output")
    }
}

impl Default for HookOutput {
    fn default() -> Self {
        Self::new()
    }
}
