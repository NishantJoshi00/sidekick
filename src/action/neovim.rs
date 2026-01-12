//! Neovim integration for performing editor actions via RPC.
//!
//! This module provides the `NeovimAction` implementation that connects to a running
//! Neovim instance via Unix socket to check buffer status, refresh buffers, and send messages.

mod buffer;
mod connection;
mod lua;

use crate::action::{Action, BufferStatus, EditorContext};
use anyhow::Result;
use neovim_lib::NeovimApi;
use std::path::PathBuf;

/// Neovim action implementation that supports multiple instances
pub struct NeovimAction {
    socket_paths: Vec<PathBuf>,
}

impl NeovimAction {
    pub fn new(socket_paths: Vec<PathBuf>) -> Self {
        Self { socket_paths }
    }
}

impl Action for NeovimAction {
    fn buffer_status(&self, file_path: &str) -> Result<BufferStatus> {
        let status = connection::try_fold_instances(
            &self.socket_paths,
            (false, false),
            |(is_current_acc, unsaved_acc), nvim| {
                let status = buffer::get_buffer_status(nvim, file_path)?;

                *is_current_acc = *is_current_acc || status.is_current;
                *unsaved_acc = *unsaved_acc || status.has_unsaved_changes;

                // Early exit if we found unsaved changes
                Ok(!*unsaved_acc)
            },
        )
        .unwrap_or((false, false));

        Ok(BufferStatus {
            is_current: status.0,
            has_unsaved_changes: status.1,
        })
    }

    fn refresh_buffer(&self, file_path: &str) -> Result<()> {
        let any_success = connection::for_each_instance(&self.socket_paths, |nvim| {
            buffer::refresh_buffer(nvim, file_path)
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to refresh buffer in any Neovim instance")
        }
    }

    fn send_message(&self, message: &str) -> Result<()> {
        let lua_code = lua::send_notification_lua(message);
        let any_success = connection::for_each_instance(&self.socket_paths, |nvim| {
            nvim.execute_lua(&lua_code, vec![])
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("Failed to send message to Neovim: {}", e))
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to send message to any Neovim instance")
        }
    }

    fn get_visual_selection(&self) -> Result<Option<EditorContext>> {
        Ok(connection::find_first(&self.socket_paths, |nvim| {
            buffer::get_visual_selection(nvim)
        }))
    }
}
