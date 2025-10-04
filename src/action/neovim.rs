//! Neovim integration for performing editor actions via RPC.
//!
//! This module provides the `NeovimAction` implementation that connects to a running
//! Neovim instance via Unix socket to check buffer status, refresh buffers, and send messages.

use crate::action::{Action, BufferStatus};
use anyhow::{Context, Result};
use neovim_lib::{Neovim, NeovimApi, Session, neovim_api::Buffer};
use std::path::PathBuf;
use std::time::Duration;

/// Neovim action implementation that supports multiple instances
pub struct NeovimAction {
    socket_paths: Vec<PathBuf>,
}

impl NeovimAction {
    pub fn new(socket_paths: Vec<PathBuf>) -> Self {
        Self { socket_paths }
    }

    /// Connect to Neovim via Unix socket and return Neovim client
    fn connect(socket_path: &PathBuf) -> Result<Neovim> {
        let mut session =
            Session::new_unix_socket(socket_path).context("Failed to connect to Neovim socket")?;
        session.set_timeout(Duration::from_secs(2));
        session.start_event_loop();
        Ok(Neovim::new(session))
    }

    /// Find buffer by file path
    fn find_buffer(nvim: &mut Neovim, file_path: &str) -> Result<Buffer> {
        let buffers = nvim.list_bufs().context("Failed to list buffers")?;

        let target_path = PathBuf::from(file_path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(file_path));

        for buffer in buffers {
            let buf_name = buffer.get_name(nvim).context("Failed to get buffer name")?;

            if buf_name.is_empty() {
                continue;
            }

            let buf_path = PathBuf::from(&buf_name)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&buf_name));

            if buf_path == target_path {
                return Ok(buffer);
            }
        }

        anyhow::bail!("Buffer not found for file: {}", file_path)
    }
}

impl Action for NeovimAction {
    fn buffer_status(&self, file_path: &str) -> Result<BufferStatus> {
        let mut any_is_current = false;
        let mut any_has_unsaved_changes = false;

        // Check all Neovim instances
        for socket_path in &self.socket_paths {
            let Ok(mut nvim) = Self::connect(socket_path) else {
                continue;
            };

            // Find the buffer
            let Ok(buffer) = Self::find_buffer(&mut nvim, file_path) else {
                continue;
            };

            // Get current buffer
            let Ok(current_buf) = nvim.get_current_buf() else {
                continue;
            };

            // Check if this is the current buffer
            let is_current = buffer == current_buf;

            // Check if buffer has unsaved changes
            let Ok(modified) = buffer.get_option(&mut nvim, "modified") else {
                continue;
            };

            let has_unsaved_changes = modified.as_bool().unwrap_or(false);

            // Aggregate status across all instances
            any_is_current = any_is_current || is_current;
            any_has_unsaved_changes = any_has_unsaved_changes || has_unsaved_changes;

            // Early exit if we found unsaved changes
            if any_has_unsaved_changes {
                break;
            }
        }

        Ok(BufferStatus {
            is_current: any_is_current,
            has_unsaved_changes: any_has_unsaved_changes,
        })
    }

    fn refresh_buffer(&self, file_path: &str) -> Result<()> {
        let mut any_success = false;

        // Refresh buffer in all Neovim instances that have it open
        for socket_path in &self.socket_paths {
            let Ok(mut nvim) = Self::connect(socket_path) else {
                continue;
            };

            // Find the buffer
            let Ok(buffer) = Self::find_buffer(&mut nvim, file_path) else {
                continue;
            };

            // Get buffer number for nvim_buf_call
            let Ok(buf_number) = buffer.get_number(&mut nvim) else {
                continue;
            };

            // Use Lua to refresh buffer while preserving cursor positions in all windows
            // This will reload the file from disk, updating the buffer content
            let lua_code = format!(
                r#"
                local buf = {}
                local cursor_positions = {{}}
                local is_current_buf = vim.api.nvim_get_current_buf() == buf

                -- Save cursor positions for all windows displaying this buffer
                for _, win in ipairs(vim.api.nvim_list_wins()) do
                    if vim.api.nvim_win_get_buf(win) == buf then
                        cursor_positions[win] = vim.api.nvim_win_get_cursor(win)
                    end
                end

                -- Refresh the buffer (checktime triggers file change detection)
                vim.api.nvim_buf_call(buf, function()
                    vim.cmd('checktime')
                    vim.cmd('edit')
                end)

                -- Restore cursor positions
                for win, pos in pairs(cursor_positions) do
                    if vim.api.nvim_win_is_valid(win) then
                        pcall(vim.api.nvim_win_set_cursor, win, pos)
                    end
                end

                -- Force redraw only if this is the current buffer
                if is_current_buf then
                    vim.cmd('redraw')
                end
                "#,
                buf_number
            );

            if nvim.execute_lua(&lua_code, vec![]).is_ok() {
                any_success = true;
            }
        }

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to refresh buffer in any Neovim instance")
        }
    }

    fn send_message(&self, message: &str) -> Result<()> {
        let mut any_success = false;

        // Send message to all Neovim instances
        for socket_path in &self.socket_paths {
            let Ok(mut nvim) = Self::connect(socket_path) else {
                continue;
            };

            // Use echo command to display message to the user
            let cmd = format!("echo '{}'", message.replace('\'', "''"));
            if nvim.command(&cmd).is_ok() {
                any_success = true;
            }
        }

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to send message to any Neovim instance")
        }
    }
}
