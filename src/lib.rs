//! Sidekick: Claude Code hook handler and editor integration (Neovim + VSCode).
//!
//! This crate provides two main functionalities:
//!
//! 1. **Claude Code Hook Handler**: Prevents file modifications when files are being
//!    edited with unsaved changes in Neovim or VSCode
//! 2. **Editor Integration**: Launches editors with deterministic socket paths for IPC
//!
//! # Supported Editors
//!
//! - **Neovim**: Via msgpack-rpc over Unix socket
//! - **VSCode**: Via JSON-RPC over Unix socket (requires vscode-sidekick extension)
//!
//! # Architecture
//!
//! - `handler`: Hook processing logic for Claude Code
//! - `hook`: Data structures for hook protocol
//! - `action`: Editor operations abstraction (buffer status, refresh, messages)
//!   - `action::neovim`: Neovim-specific implementation
//!   - `action::vscode`: VSCode-specific implementation
//! - `utils`: Socket path computation and discovery
//! - `constants`: Shared constants (timeouts, paths)
//!
//! # Example: Using as a Library
//!
//! ```no_run
//! use sidekick::action::{Action, neovim::NeovimAction, vscode::VSCodeAction};
//! use sidekick::utils;
//!
//! // Find editor instances in current directory
//! let nvim_sockets = utils::find_neovim_sockets().unwrap();
//! let vscode_sockets = utils::find_vscode_sockets().unwrap();
//!
//! // Create actions for each editor
//! if !nvim_sockets.is_empty() {
//!     let action = NeovimAction::new(nvim_sockets);
//!     let status = action.buffer_status("file.txt").unwrap();
//!     if !status.has_unsaved_changes {
//!         // Safe to modify file
//!     }
//! }
//!
//! if !vscode_sockets.is_empty() {
//!     let action = VSCodeAction::new(vscode_sockets);
//!     let status = action.buffer_status("file.txt").unwrap();
//!     if !status.has_unsaved_changes {
//!         // Safe to modify file
//!     }
//! }
//! ```

pub mod action;
pub mod constants;
pub mod handler;
pub mod hook;
pub mod utils;
