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
//!    - Reload buffer from disk across all editor instances (Neovim and VSCode)
//!    - Preserve cursor positions
//!
//! 3. UserPromptSubmit: Inject visual selection as additional context
//!    - If any editor has a visual selection → inject as additionalContext
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

use crate::action::neovim::NeovimAction;
use crate::action::vscode::VSCodeAction;
use crate::action::{Action, BufferStatus, EditorContext};
use crate::hook::{self, Hook, HookEvent, HookOutput, PermissionDecision, Tool};
use crate::utils;

/// Collection of all available editor actions
struct EditorActions {
    neovim: Option<NeovimAction>,
    vscode: Option<VSCodeAction>,
}

impl EditorActions {
    /// Check if any editor is available
    fn has_any(&self) -> bool {
        self.neovim.is_some() || self.vscode.is_some()
    }

    /// Get combined buffer status from all editors
    fn buffer_status(&self, file_path: &str) -> BufferStatus {
        let mut combined = BufferStatus {
            is_current: false,
            has_unsaved_changes: false,
        };

        if let Some(ref nvim) = self.neovim
            && let Ok(status) = nvim.buffer_status(file_path)
        {
            combined.is_current = combined.is_current || status.is_current;
            combined.has_unsaved_changes =
                combined.has_unsaved_changes || status.has_unsaved_changes;
        }

        if let Some(ref vscode) = self.vscode
            && let Ok(status) = vscode.buffer_status(file_path)
        {
            combined.is_current = combined.is_current || status.is_current;
            combined.has_unsaved_changes =
                combined.has_unsaved_changes || status.has_unsaved_changes;
        }

        combined
    }

    /// Refresh buffer in all editors
    fn refresh_buffer(&self, file_path: &str) {
        if let Some(ref nvim) = self.neovim
            && let Err(e) = nvim.refresh_buffer(file_path)
        {
            eprintln!("Warning: Failed to refresh buffer in Neovim: {}", e);
        }

        if let Some(ref vscode) = self.vscode
            && let Err(e) = vscode.refresh_buffer(file_path)
        {
            eprintln!("Warning: Failed to refresh buffer in VSCode: {}", e);
        }
    }

    /// Send message to all editors
    fn send_message(&self, message: &str) {
        if let Some(ref nvim) = self.neovim
            && let Err(e) = nvim.send_message(message)
        {
            eprintln!("Warning: Failed to send message to Neovim: {}", e);
        }

        if let Some(ref vscode) = self.vscode
            && let Err(e) = vscode.send_message(message)
        {
            eprintln!("Warning: Failed to send message to VSCode: {}", e);
        }
    }

    /// Get visual selections from all editors
    fn get_visual_selections(&self) -> Vec<EditorContext> {
        let mut selections = Vec::new();

        if let Some(ref nvim) = self.neovim
            && let Ok(mut nvim_selections) = nvim.get_visual_selections()
        {
            selections.append(&mut nvim_selections);
        }

        if let Some(ref vscode) = self.vscode
            && let Ok(mut vscode_selections) = vscode.get_visual_selections()
        {
            selections.append(&mut vscode_selections);
        }

        selections
    }
}

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;

    // Get actions for all available editors
    let actions = get_editor_actions()?;

    // Handle based on hook type
    let output = match hook {
        Hook::Tool(h) => match h.hook_event_name {
            HookEvent::PreToolUse => handle_pre_tool_use(&h.tool, &actions),
            HookEvent::PostToolUse => handle_post_tool_use(&h.tool, &actions),
        },
        Hook::UserPrompt => handle_user_prompt_submit(&actions),
    };

    // Return hook output
    io::stdout().write_all(output.to_json()?.as_bytes())?;

    Ok(())
}

/// Get actions for all available editors
fn get_editor_actions() -> anyhow::Result<EditorActions> {
    let nvim_sockets = utils::find_neovim_sockets()?;
    let vscode_sockets = utils::find_vscode_sockets()?;

    Ok(EditorActions {
        neovim: if nvim_sockets.is_empty() {
            None
        } else {
            Some(NeovimAction::new(nvim_sockets))
        },
        vscode: if vscode_sockets.is_empty() {
            None
        } else {
            Some(VSCodeAction::new(vscode_sockets))
        },
    })
}

/// Handle PreToolUse hook - check if file has unsaved changes
fn handle_pre_tool_use(tool: &Tool, actions: &EditorActions) -> HookOutput {
    match tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            check_buffer_modifications(actions, &file_tool.file_path)
        }
        _ => HookOutput::new(),
    }
}

/// Handle PostToolUse hook - refresh buffers after modifications
fn handle_post_tool_use(tool: &Tool, actions: &EditorActions) -> HookOutput {
    match tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            refresh_buffer(actions, &file_tool.file_path)
        }
        _ => HookOutput::new(),
    }
}

/// Handle UserPromptSubmit hook - inject visual selections as context
fn handle_user_prompt_submit(actions: &EditorActions) -> HookOutput {
    if !actions.has_any() {
        return HookOutput::new();
    }

    let selections = actions.get_visual_selections();

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

/// Check if buffer has unsaved modifications and block if necessary
fn check_buffer_modifications(actions: &EditorActions, file_path: &str) -> HookOutput {
    if !actions.has_any() {
        return HookOutput::new();
    }

    let status = actions.buffer_status(file_path);

    if status.has_unsaved_changes && status.is_current {
        actions.send_message("Claude tried to edit this file");

        HookOutput::new().with_permission_decision(
            PermissionDecision::Deny,
            Some("The file is being edited by the user, try again later".to_string()),
        )
    } else {
        HookOutput::new()
    }
}

/// Refresh buffer after file modification
fn refresh_buffer(actions: &EditorActions, file_path: &str) -> HookOutput {
    if !actions.has_any() {
        return HookOutput::new();
    }

    actions.refresh_buffer(file_path);

    HookOutput::new()
}
