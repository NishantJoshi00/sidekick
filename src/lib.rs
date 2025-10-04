//! Sidekick: Claude Code hook handler and Neovim integration.
//!
//! This crate provides two main functionalities:
//!
//! 1. **Claude Code Hook Handler**: Prevents file modifications when files are being
//!    edited in Neovim with unsaved changes
//! 2. **Neovim Integration**: Launches Neovim instances with deterministic socket paths
//!
//! # Architecture
//!
//! - `handler`: Hook processing logic for Claude Code
//! - `hook`: Data structures for hook protocol
//! - `action`: Editor operations abstraction (buffer status, refresh, messages)
//! - `utils`: Socket path computation and discovery
//! - `constants`: Shared constants (timeouts, paths)
//!
//! # Example: Using as a Library
//!
//! ```no_run
//! use sidekick::action::{Action, neovim::NeovimAction};
//! use sidekick::utils;
//!
//! // Find Neovim instances in current directory
//! let sockets = utils::find_matching_sockets().unwrap();
//! let action = NeovimAction::new(sockets);
//!
//! // Check if file can be modified
//! let status = action.buffer_status("file.txt").unwrap();
//! if !status.has_unsaved_changes {
//!     // Safe to modify file
//! }
//! ```

pub mod action;
pub mod constants;
pub mod handler;
pub mod hook;
pub mod utils;
