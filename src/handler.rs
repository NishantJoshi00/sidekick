use std::io::{self, Read, Write};

use crate::action::{Action, neovim::NeovimAction};
use crate::hook::{self, HookEvent, HookOutput, PermissionDecision, Tool};
use crate::utils;

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;

    // Get Neovim action if available
    let nvim_action = get_neovim_action()?;

    // Handle based on hook event type
    let output = match hook.hook_event_name {
        HookEvent::PreToolUse => {
            handle_pre_tool_use(&hook.tool, nvim_action.as_ref(), hook.hook_event_name)
        }
        HookEvent::PostToolUse => {
            handle_post_tool_use(&hook.tool, nvim_action.as_ref(), hook.hook_event_name)
        }
    };

    // Return hook output
    io::stdout().write_all(output.to_json()?.as_bytes())?;

    Ok(())
}

/// Get Neovim action if any sockets exist
fn get_neovim_action() -> anyhow::Result<Option<NeovimAction>> {
    let socket_paths = utils::find_matching_sockets()?;

    Ok(if socket_paths.is_empty() {
        None
    } else {
        Some(NeovimAction::new(socket_paths))
    })
}

/// Handle PreToolUse hook - only perform checks
fn handle_pre_tool_use(
    tool: &Tool,
    nvim_action: Option<&NeovimAction>,
    event: HookEvent,
) -> HookOutput {
    debug_assert_eq!(
        event,
        HookEvent::PreToolUse,
        "PreToolUse handler called with wrong event type"
    );

    match tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            check_buffer_modifications(nvim_action, &file_tool.file_path)
        }
        _ => HookOutput::new(),
    }
}

/// Handle PostToolUse hook - refresh buffers after modifications
fn handle_post_tool_use(
    tool: &Tool,
    nvim_action: Option<&NeovimAction>,
    event: HookEvent,
) -> HookOutput {
    debug_assert_eq!(
        event,
        HookEvent::PostToolUse,
        "PostToolUse handler called with wrong event type"
    );

    match tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            refresh_buffer(nvim_action, &file_tool.file_path)
        }
        _ => HookOutput::new(),
    }
}

/// Check if buffer has unsaved modifications and block if necessary
fn check_buffer_modifications(nvim_action: Option<&NeovimAction>, file_path: &str) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    let Ok(status) = action.buffer_status(file_path) else {
        return HookOutput::new();
    };

    if status.has_unsaved_changes && status.is_current {
        let message = format!("Claude tried to edit: {}", file_path);
        if let Err(e) = action.send_message(&message) {
            eprintln!("Warning: Failed to send message to Neovim: {}", e);
        }

        HookOutput::new().with_permission_decision(
            PermissionDecision::Deny,
            Some("Claude tried to edit this file".to_string()),
        )
    } else {
        HookOutput::new()
    }
}

/// Refresh buffer after file modification
fn refresh_buffer(nvim_action: Option<&NeovimAction>, file_path: &str) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    if let Err(e) = action.refresh_buffer(file_path) {
        eprintln!("Warning: Failed to refresh buffer: {}", e);
    }

    HookOutput::new()
}
