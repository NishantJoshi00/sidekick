//! Neovim integration for performing editor actions via RPC.
//!
//! This module provides the `NeovimAction` implementation that connects to a running
//! Neovim instance via Unix socket to check buffer status, refresh buffers, and send messages.

use crate::action::{Action, BufferStatus};
use anyhow::{Context, Result};
use neovim_lib::{Neovim, NeovimApi, Session, neovim_api::Buffer};
use std::path::PathBuf;
use std::time::Duration;

/// Neovim action implementation
pub struct NeovimAction {
    socket_path: PathBuf,
}

impl NeovimAction {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// Connect to Neovim via Unix socket and return Neovim client
    fn connect(&self) -> Result<Neovim> {
        let mut session = Session::new_unix_socket(&self.socket_path)
            .context("Failed to connect to Neovim socket")?;
        session.set_timeout(Duration::from_secs(2));
        session.start_event_loop();
        Ok(Neovim::new(session))
    }

    /// Find buffer by file path
    fn find_buffer(&self, nvim: &mut Neovim, file_path: &str) -> Result<Buffer> {
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
        let mut nvim = self.connect()?;

        // Find the buffer
        let buffer = self.find_buffer(&mut nvim, file_path)?;

        // Get current buffer
        let current_buf = nvim
            .get_current_buf()
            .context("Failed to get current buffer")?;

        // Check if this is the current buffer
        let is_current = buffer == current_buf;

        // Check if buffer has unsaved changes
        let modified = buffer
            .get_option(&mut nvim, "modified")
            .context("Failed to get modified option")?;

        let has_unsaved_changes = modified.as_bool().unwrap_or(false);

        Ok(BufferStatus {
            is_current,
            has_unsaved_changes,
        })
    }

    fn refresh_buffer(&self, file_path: &str) -> Result<()> {
        let mut nvim = self.connect()?;

        // Find the buffer
        let buffer = self.find_buffer(&mut nvim, file_path)?;

        // Get buffer number for nvim_buf_call
        let buf_number = buffer
            .get_number(&mut nvim)
            .context("Failed to get buffer number")?;

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

        nvim.execute_lua(&lua_code, vec![])
            .context("Failed to reload buffer")?;

        Ok(())
    }

    fn send_message(&self, message: &str) -> Result<()> {
        let mut nvim = self.connect()?;

        // Use echo command to display message to the user
        let cmd = format!("echo '{}'", message.replace('\'', "''"));
        nvim.command(&cmd)
            .context("Failed to send message to Neovim")?;

        Ok(())
    }
}
