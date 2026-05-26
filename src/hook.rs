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

/// Hook event type
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
}

/// Hook input for tool-related events (PreToolUse, PostToolUse)
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ToolHook {
    pub session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    pub cwd: String,
    // Crush sends `event` instead of `hook_event_name`.
    #[serde(alias = "event")]
    pub hook_event_name: HookEvent,
    #[serde(flatten)]
    pub tool: Tool,
}

/// Parsed hook - either tool-related or user prompt
#[derive(Debug)]
pub enum Hook {
    Tool(ToolHook),
    UserPrompt,
}

/// Tool types discriminated by tool_name
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Tool {
    Read(FileToolInput),
    Write(FileToolInput),
    Edit(FileToolInput),
    MultiEdit(FileToolInput),
    Bash(BashToolInput),
    Other {
        name: String,
        input: serde_json::Value,
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

impl<'de> serde::Deserialize<'de> for Tool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let name = value
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::missing_field("tool_name"))?
            .to_string();
        let input = value
            .get("tool_input")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        match name.as_str() {
            "Read" => serde_json::from_value(input)
                .map(Tool::Read)
                .map_err(serde::de::Error::custom),
            "Write" => serde_json::from_value(input)
                .map(Tool::Write)
                .map_err(serde::de::Error::custom),
            "Edit" => serde_json::from_value(input)
                .map(Tool::Edit)
                .map_err(serde::de::Error::custom),
            "MultiEdit" => serde_json::from_value(input)
                .map(Tool::MultiEdit)
                .map_err(serde::de::Error::custom),
            "Bash" => serde_json::from_value(input)
                .map(Tool::Bash)
                .map_err(serde::de::Error::custom),
            _ => Ok(Tool::Other { name, input }),
        }
    }
}

impl serde::Serialize for Tool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            tool_name: &'a str,
            tool_input: serde_json::Value,
        }

        let wire = match self {
            Tool::Read(input) => Wire {
                tool_name: "Read",
                tool_input: serde_json::to_value(input).map_err(serde::ser::Error::custom)?,
            },
            Tool::Write(input) => Wire {
                tool_name: "Write",
                tool_input: serde_json::to_value(input).map_err(serde::ser::Error::custom)?,
            },
            Tool::Edit(input) => Wire {
                tool_name: "Edit",
                tool_input: serde_json::to_value(input).map_err(serde::ser::Error::custom)?,
            },
            Tool::MultiEdit(input) => Wire {
                tool_name: "MultiEdit",
                tool_input: serde_json::to_value(input).map_err(serde::ser::Error::custom)?,
            },
            Tool::Bash(input) => Wire {
                tool_name: "Bash",
                tool_input: serde_json::to_value(input).map_err(serde::ser::Error::custom)?,
            },
            Tool::Other { name, input } => Wire {
                tool_name: name,
                tool_input: input.clone(),
            },
        };

        wire.serialize(serializer)
    }
}

pub fn parse_hook(input: &str) -> anyhow::Result<Hook> {
    // First, peek at the hook_event_name to determine which struct to parse
    let value: serde_json::Value =
        serde_json::from_str(input).context("couldn't parse hook input")?;
    let event_name = value
        .get("hook_event_name")
        .or_else(|| value.get("event"))
        .and_then(|v| v.as_str())
        .context("hook is missing event name")?;

    match event_name {
        "UserPromptSubmit" => Ok(Hook::UserPrompt),
        "PreToolUse" | "PostToolUse" => {
            let hook: ToolHook =
                serde_json::from_str(input).context("unrecognized tool in hook")?;
            Ok(Hook::Tool(hook))
        }
        _ => {
            anyhow::bail!("unrecognized hook event")
        }
    }
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
