//! Editor action abstractions for interacting with text editors.
//!
//! This module defines the `Action` trait for performing operations on editor buffers,
//! such as checking buffer status, refreshing content, and sending messages.
//!
//! # Example
//!
//! ```no_run
//! use sidekick::action::{Action, neovim::NeovimAction};
//! use std::path::PathBuf;
//!
//! // Create action for Neovim instances
//! let sockets = vec![PathBuf::from("/tmp/socket.sock")];
//! let action = NeovimAction::new(sockets);
//!
//! // Check buffer status
//! let status = action.buffer_status("file.txt").unwrap();
//! if status.has_unsaved_changes && status.is_current {
//!     println!("File has unsaved changes!");
//! }
//!
//! // Refresh buffer after external modification
//! action.refresh_buffer("file.txt").unwrap();
//!
//! // Send message to editor
//! action.send_message("Hello from Sidekick!").unwrap();
//! ```

pub mod neovim;
pub mod vscode;

/// Buffer status information
#[derive(Debug, Clone)]
pub struct BufferStatus {
    pub is_current: bool,
    pub has_unsaved_changes: bool,
}

/// Editor context from visual selection
#[derive(Debug, Clone)]
pub struct EditorContext {
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

/// Trait for editor actions
pub trait Action {
    /// Get the status of a buffer
    fn buffer_status(&self, file_path: &str) -> anyhow::Result<BufferStatus>;

    /// Refresh the buffer (reload from disk)
    fn refresh_buffer(&self, file_path: &str) -> anyhow::Result<()>;

    /// Send a message to the editor
    fn send_message(&self, message: &str) -> anyhow::Result<()>;

    /// Get visual selections from all editor instances
    fn get_visual_selections(&self) -> anyhow::Result<Vec<EditorContext>>;
}
