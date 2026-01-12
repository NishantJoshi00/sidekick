//! Hook processing logic for Claude Code integration.
//!
//! This module handles Claude Code hooks by reading JSON from stdin, processing
//! the hook event, and writing the response JSON to stdout. It supports:
//!
//! # Hook Flow
//!
//! 1. PreToolUse: Check if file has unsaved changes before Claude Code modifies it
//!    - If file is current buffer with unsaved changes → Deny
//!    - Otherwise → Allow
//!
//! 2. PostToolUse: Refresh buffer after Claude Code modifies it
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
//! // Called by Claude Code via stdin/stdout
//! handler::handle_hook().expect("Failed to process hook");
//! ```

use std::io::{self, Read, Write};

use crate::action::{Action, neovim::NeovimAction};
use crate::hook::{self, Hook, HookEvent, HookOutput, PermissionDecision, Tool};
use crate::utils;

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;

    // Get Neovim action if available
    let nvim_action = get_neovim_action()?;

    // Handle based on hook type
    let output = match hook {
        Hook::Tool(h) => match h.hook_event_name {
            HookEvent::PreToolUse => handle_pre_tool_use(&h.tool, nvim_action.as_ref()),
            HookEvent::PostToolUse => handle_post_tool_use(&h.tool, nvim_action.as_ref()),
            HookEvent::UserPromptSubmit => HookOutput::new(), // shouldn't happen
        },
        Hook::UserPrompt => handle_user_prompt_submit(nvim_action.as_ref()),
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

/// Handle PreToolUse hook - check if file has unsaved changes
fn handle_pre_tool_use(tool: &Tool, nvim_action: Option<&NeovimAction>) -> HookOutput {
    match tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            check_buffer_modifications(nvim_action, &file_tool.file_path)
        }
        _ => HookOutput::new(),
    }
}

/// Handle PostToolUse hook - refresh buffers after modifications
fn handle_post_tool_use(tool: &Tool, nvim_action: Option<&NeovimAction>) -> HookOutput {
    match tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            refresh_buffer(nvim_action, &file_tool.file_path)
        }
        _ => HookOutput::new(),
    }
}

/// Handle UserPromptSubmit hook - inject visual selection as context
fn handle_user_prompt_submit(nvim_action: Option<&NeovimAction>) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    let Ok(Some(ctx)) = action.get_visual_selection() else {
        return HookOutput::new();
    };

    let context = format!(
        "[Selected from {}:{}-{}]\n```\n{}\n```",
        ctx.file_path, ctx.start_line, ctx.end_line, ctx.content
    );

    HookOutput::new().with_additional_context(context)
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
        if let Err(e) = action.send_message("Claude tried to edit this file") {
            eprintln!("Warning: Failed to send message to Neovim: {}", e);
        }

        HookOutput::new().with_permission_decision(
            PermissionDecision::Deny,
            Some("The file is being edited by the user, try again later".to_string()),
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
