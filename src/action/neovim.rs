//! Neovim integration for performing editor actions via RPC.
//!
//! This module provides the `NeovimAction` implementation that connects to a running
//! Neovim instance via Unix socket to check buffer status, refresh buffers, and send messages.

use crate::action::{Action, BufferStatus};
use anyhow::{Context, Result};
use neovim_lib::{neovim_api::Buffer, Neovim, NeovimApi, Session};
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
            let buf_name = buffer
                .get_name(nvim)
                .context("Failed to get buffer name")?;

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

        // Get current buffer
        let current_buf = nvim
            .get_current_buf()
            .context("Failed to get current buffer")?;

        let is_current = buffer == current_buf;

        // Check if buffer has unsaved changes
        let modified = buffer
            .get_option(&mut nvim, "modified")
            .context("Failed to get modified option")?;

        let has_unsaved_changes = modified.as_bool().unwrap_or(false);

        // Don't refresh if buffer has unsaved changes AND is the current buffer
        if has_unsaved_changes && is_current {
            anyhow::bail!(
                "Cannot refresh buffer with unsaved changes while it is in view: {}",
                file_path
            );
        }

        // Get buffer number for nvim_buf_call
        let buf_number = buffer
            .get_number(&mut nvim)
            .context("Failed to get buffer number")?;

        // Use Lua to call edit! in the context of the buffer without switching windows
        let lua_code = format!(
            r#"
            vim.api.nvim_buf_call({}, function()
                vim.cmd('edit!')
            end)
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

    fn delete_buffer(&self, file_path: &str) -> Result<()> {
        let mut nvim = self.connect()?;

        // Find the buffer
        let buffer = self.find_buffer(&mut nvim, file_path)?;

        // Get buffer number
        let buf_number = buffer
            .get_number(&mut nvim)
            .context("Failed to get buffer number")?;

        // Delete the buffer using bdelete command
        let cmd = format!("bdelete {}", buf_number);
        nvim.command(&cmd)
            .context("Failed to delete buffer")?;

        Ok(())
    }
}
