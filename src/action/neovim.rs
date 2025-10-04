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

    /// Execute a closure for each successfully connected Neovim instance
    /// Returns whether any instance was successfully processed
    fn for_each_instance<F>(&self, mut f: F) -> bool
    where
        F: FnMut(&mut Neovim) -> Result<()>,
    {
        let mut any_success = false;
        for socket_path in &self.socket_paths {
            if let Ok(mut nvim) = Self::connect(socket_path)
                && f(&mut nvim).is_ok()
            {
                any_success = true;
            }
        }
        any_success
    }

    /// Fold over successfully connected Neovim instances with early exit support
    /// Returns None if no instances were processed, otherwise returns the accumulated value
    /// Closure updates accumulator in place and returns whether to continue
    fn try_fold_instances<T, F>(&self, init: T, mut f: F) -> Option<T>
    where
        F: FnMut(&mut T, &mut Neovim) -> Result<bool>,
    {
        let mut acc = init;
        let mut any_processed = false;

        for socket_path in &self.socket_paths {
            if let Ok(mut nvim) = Self::connect(socket_path) {
                match f(&mut acc, &mut nvim) {
                    Ok(should_continue) => {
                        any_processed = true;
                        if !should_continue {
                            return Some(acc);
                        }
                    }
                    Err(_) => continue,
                }
            }
        }

        any_processed.then_some(acc)
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
        let status = self
            .try_fold_instances((false, false), |(is_current_acc, unsaved_acc), nvim| {
                let buffer = Self::find_buffer(nvim, file_path)?;
                let current_buf = nvim.get_current_buf()?;
                let is_current = buffer == current_buf;

                let modified = buffer.get_option(nvim, "modified")?;
                let has_unsaved_changes = modified.as_bool().unwrap_or(false);

                *is_current_acc = *is_current_acc || is_current;
                *unsaved_acc = *unsaved_acc || has_unsaved_changes;

                // Early exit if we found unsaved changes
                Ok(!*unsaved_acc)
            })
            .unwrap_or((false, false));

        Ok(BufferStatus {
            is_current: status.0,
            has_unsaved_changes: status.1,
        })
    }

    fn refresh_buffer(&self, file_path: &str) -> Result<()> {
        let any_success = self.for_each_instance(|nvim| {
            let buffer = Self::find_buffer(nvim, file_path)?;
            let buf_number = buffer.get_number(nvim)?;

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

            nvim.execute_lua(&lua_code, vec![])
                .map(|_| ())
                .context("Failed to reload buffer")
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to refresh buffer in any Neovim instance")
        }
    }

    fn send_message(&self, message: &str) -> Result<()> {
        let cmd = format!("echo '{}'", message.replace('\'', "''"));
        let any_success = self.for_each_instance(|nvim| {
            nvim.command(&cmd)
                .context("Failed to send message to Neovim")
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to send message to any Neovim instance")
        }
    }
}
