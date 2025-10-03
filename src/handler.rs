use anyhow::Context;
use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use crate::action::{Action, neovim::NeovimAction};
use crate::hook::{self, HookOutput, PermissionDecision, Tool};

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;

    // Get absolute path of current working directory
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("Failed to canonicalize current directory")?;

    // Compute blake3 hash of the absolute path
    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    let hash_hex = hash.to_hex();

    // Create socket path
    let socket_path = PathBuf::from(format!("/tmp/{}.sock", hash_hex));

    // Try to initialize Neovim action manager if socket exists
    let nvim_action = if socket_path.exists() {
        Some(NeovimAction::new(socket_path))
    } else {
        None
    };

    // Check if this is a file modification tool (Edit, Write, MultiEdit)
    let output = match &hook.tool {
        Tool::Edit(file_tool) | Tool::Write(file_tool) | Tool::MultiEdit(file_tool) => {
            // If we have a neovim connection, check buffer status
            if let Some(action) = &nvim_action {
                match action.buffer_status(&file_tool.file_path) {
                    Ok(status) => {
                        // If buffer is modified AND is the current buffer, deny
                        if status.has_unsaved_changes && status.is_current {
                            // Send message to Neovim
                            let message = format!(
                                "Claude Code blocked: File {} has unsaved changes",
                                file_tool.file_path
                            );
                            if let Err(e) = action.send_message(&message) {
                                eprintln!("Warning: Failed to send message to Neovim: {}", e);
                            }

                            HookOutput::new().with_permission_decision(
                                PermissionDecision::Deny,
                                Some(format!(
                                    "File {} is currently being edited with unsaved changes",
                                    file_tool.file_path
                                )),
                            )
                        } else {
                            // Otherwise, try to refresh the buffer and allow
                            if let Err(e) = action.refresh_buffer(&file_tool.file_path) {
                                eprintln!("Warning: Failed to refresh buffer: {}", e);
                            }
                            HookOutput::new()
                        }
                    }
                    Err(_) => {
                        // Buffer doesn't exist in neovim or some error occurred, allow the action
                        HookOutput::new()
                    }
                }
            } else {
                // No neovim connection, allow the action
                HookOutput::new()
            }
        }
        // For all other tools, allow them
        _ => HookOutput::new(),
    };

    // Return hook output
    io::stdout().write_all(output.to_json()?.as_bytes())?;

    Ok(())
}
