//! Hook processing logic.
//!
//! Reads a Claude-Code-shaped JSON envelope from stdin, decides allow/deny,
//! and writes the response JSON to stdout. The same protocol is also driven
//! by the opencode plugin under `plugins/opencode/`, which translates
//! opencode's `tool.execute.before`/`.after` events into this envelope.
//!
//! # Hook Flow
//!
//! 1. PreToolUse: Check if file has unsaved changes before the AI modifies it
//!    - If file is current buffer with unsaved changes → Deny
//!    - Otherwise → Allow
//!
//! 2. PostToolUse: Refresh buffer after the AI modifies it
//!    - Reload buffer from disk across all Neovim instances
//!    - Preserve cursor positions
//!
//! 3. UserPromptSubmit: Inject visual selection as additional context
//!    - If Neovim has a visual selection → inject as additionalContext
//!    - Otherwise → no-op
//!
//! # Example
//!
//! ```no_run
//! use sidekick::handler;
//!
//! // Called by Claude Code (or the opencode plugin) via stdin/stdout
//! handler::handle_hook().expect("Failed to process hook");
//! ```

use std::io::{self, Read, Write};
use std::time::Instant;

use chrono::Utc;

use crate::action::{Action, neovim::NeovimAction};
use crate::analytics::{
    self,
    event::{BufferRefresh, Decision, DecisionReason, Event, HookDecision, ToolKind},
};
use crate::hook::{self, Hook, HookEvent, HookOutput, PermissionDecision, Tool, ToolHook};
use crate::utils;

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;

    // Resolve nvim instances once so we know how many we probed.
    let socket_paths = utils::find_matching_sockets().unwrap_or_default();
    let instances_probed = socket_paths.len();
    let nvim_action = if socket_paths.is_empty() {
        None
    } else {
        Some(NeovimAction::new(socket_paths))
    };

    // Handle based on hook type
    let output = match hook {
        Hook::Tool(h) => match h.hook_event_name {
            HookEvent::PreToolUse => {
                handle_pre_tool_use(&h, nvim_action.as_ref(), instances_probed)
            }
            HookEvent::PostToolUse => handle_post_tool_use(&h, nvim_action.as_ref()),
        },
        Hook::UserPrompt => handle_user_prompt_submit(nvim_action.as_ref()),
    };

    // Return hook output
    io::stdout().write_all(output.to_json()?.as_bytes())?;

    Ok(())
}

/// Handle PreToolUse hook - check if file has unsaved changes
fn handle_pre_tool_use(
    h: &ToolHook,
    nvim_action: Option<&NeovimAction>,
    instances_probed: usize,
) -> HookOutput {
    let Some((tool_kind, file_path)) = tool_to_mutation(&h.tool) else {
        return HookOutput::new();
    };

    let started = Instant::now();
    let (output, reason) = check_buffer_modifications(nvim_action, file_path);
    let decision = match reason {
        DecisionReason::BufferDirtyAndCurrent => Decision::Deny,
        _ => Decision::Allow,
    };

    analytics::store::append(&Event::HookDecision(HookDecision {
        at: Utc::now(),
        session_id: h.session_id.clone(),
        cwd: h.cwd.clone(),
        tool: tool_kind,
        file: file_path.to_string(),
        decision,
        reason,
        instances_probed,
        latency_ms: started.elapsed().as_millis() as u64,
    }));

    output
}

/// Handle PostToolUse hook - refresh buffers after modifications
fn handle_post_tool_use(h: &ToolHook, nvim_action: Option<&NeovimAction>) -> HookOutput {
    let Some((tool_kind, file_path)) = tool_to_mutation(&h.tool) else {
        return HookOutput::new();
    };

    let output = refresh_buffer(nvim_action, file_path);

    // Only count refreshes when nvim was reachable — otherwise nothing happened
    // and recording the event would inflate the activity charts.
    if nvim_action.is_some() {
        analytics::store::append(&Event::BufferRefresh(BufferRefresh {
            at: Utc::now(),
            session_id: h.session_id.clone(),
            cwd: h.cwd.clone(),
            tool: tool_kind,
            file: file_path.to_string(),
        }));
    }

    output
}

/// Handle UserPromptSubmit hook - inject visual selections as context
fn handle_user_prompt_submit(nvim_action: Option<&NeovimAction>) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    let Ok(selections) = action.get_visual_selections() else {
        return HookOutput::new();
    };

    if selections.is_empty() {
        return HookOutput::new();
    }

    let context = selections
        .iter()
        .map(|ctx| {
            format!(
                "[Selected from {}:{}-{}]\n```\n{}\n```",
                ctx.file_path, ctx.start_line, ctx.end_line, ctx.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    HookOutput::new().with_additional_context(context)
}

/// Check if buffer has unsaved modifications and block if necessary.
/// Returns the hook response alongside a `DecisionReason` for analytics.
fn check_buffer_modifications(
    nvim_action: Option<&NeovimAction>,
    file_path: &str,
) -> (HookOutput, DecisionReason) {
    let Some(action) = nvim_action else {
        return (HookOutput::new(), DecisionReason::NoNvimRunning);
    };

    let Ok(status) = action.buffer_status(file_path) else {
        return (HookOutput::new(), DecisionReason::StatusCheckFailed);
    };

    if status.has_unsaved_changes && status.is_current {
        if let Err(e) = action.send_message("Edit blocked — file has unsaved changes") {
            eprintln!("Warning: {}", e);
        }

        let output = HookOutput::new().with_permission_decision(
            PermissionDecision::Deny,
            Some("The file is being edited by the user, try again later".to_string()),
        );
        (output, DecisionReason::BufferDirtyAndCurrent)
    } else {
        (HookOutput::new(), DecisionReason::BufferAvailable)
    }
}

/// Refresh buffer after file modification
fn refresh_buffer(nvim_action: Option<&NeovimAction>, file_path: &str) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    if let Err(e) = action.refresh_buffer(file_path) {
        eprintln!("Warning: {}", e);
    }

    HookOutput::new()
}

fn tool_to_mutation(tool: &Tool) -> Option<(ToolKind, &str)> {
    match tool {
        Tool::Edit(f) => Some((ToolKind::Edit, f.file_path.as_str())),
        Tool::Write(f) => Some((ToolKind::Write, f.file_path.as_str())),
        Tool::MultiEdit(f) => Some((ToolKind::MultiEdit, f.file_path.as_str())),
        _ => None,
    }
}
