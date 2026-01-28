//! VSCode integration for performing editor actions via IPC.
//!
//! This module provides the `VSCodeAction` implementation that connects to a running
//! VSCode instance via Unix socket to check buffer status, refresh buffers, and send messages.

mod connection;
mod rpc;

use crate::action::{Action, BufferStatus, EditorContext};
use anyhow::Result;
use std::path::PathBuf;

/// VSCode action implementation that supports multiple instances
pub struct VSCodeAction {
    socket_paths: Vec<PathBuf>,
}

impl VSCodeAction {
    pub fn new(socket_paths: Vec<PathBuf>) -> Self {
        Self { socket_paths }
    }
}

impl Action for VSCodeAction {
    fn buffer_status(&self, file_path: &str) -> Result<BufferStatus> {
        let status = connection::try_fold_instances(
            &self.socket_paths,
            (false, false),
            |(is_current_acc, unsaved_acc), client| {
                let status = client.buffer_status(file_path)?;

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
        let any_success = connection::for_each_instance(&self.socket_paths, |client| {
            client.refresh_buffer(file_path)?;
            Ok(())
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to refresh buffer in any VSCode instance")
        }
    }

    fn send_message(&self, message: &str) -> Result<()> {
        let any_success = connection::for_each_instance(&self.socket_paths, |client| {
            client.send_message(message)?;
            Ok(())
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to send message to any VSCode instance")
        }
    }

    fn get_visual_selections(&self) -> Result<Vec<EditorContext>> {
        Ok(connection::collect_all(&self.socket_paths, |client| {
            let result = client.get_visual_selection()?;
            Ok(result.map(|sel| EditorContext {
                file_path: sel.file_path,
                start_line: sel.start_line,
                end_line: sel.end_line,
                content: sel.content,
            }))
        }))
    }
}
