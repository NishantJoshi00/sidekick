//! Socket path utilities for Neovim instance discovery.
//!
//! This module provides functions for computing and discovering Unix socket paths
//! for Neovim instances. Sockets are named using a deterministic hash of the
//! current working directory and the process ID.
//!
//! # Socket Naming Scheme
//!
//! - Pattern: `/tmp/<blake3(cwd)>-<pid>.sock`
//! - Example: `/tmp/a1b2c3d4e5f6...-12345.sock`
//!
//! This allows:
//! - Multiple Neovim instances per directory (different PIDs)
//! - Easy discovery of all instances for a directory (glob pattern)
//! - No socket conflicts between different directories
//!
//! # Example
//!
//! ```no_run
//! use sidekick::utils;
//!
//! // Compute socket path for current process
//! let pid = std::process::id();
//! let socket = utils::compute_socket_path_with_pid(pid).unwrap();
//! println!("Socket: {:?}", socket);
//!
//! // Find all Neovim instances in this directory
//! let sockets = utils::find_matching_sockets().unwrap();
//! println!("Found {} instances", sockets.len());
//! ```

use anyhow::Context;
use std::env;
use std::path::PathBuf;

/// Compute socket path based on current working directory hash and process ID
pub fn compute_socket_path_with_pid(pid: u32) -> anyhow::Result<PathBuf> {
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("Failed to canonicalize current directory")?;

    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    let hash_hex = hash.to_hex();

    Ok(PathBuf::from(format!("/tmp/{}-{}.sock", hash_hex, pid)))
}

/// Find all socket paths matching the current working directory hash
pub fn find_matching_sockets() -> anyhow::Result<Vec<PathBuf>> {
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("Failed to canonicalize current directory")?;

    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    let hash_hex = hash.to_hex();

    let pattern = format!("/tmp/{}-*.sock", hash_hex);

    Ok(glob::glob(&pattern)
        .context("Failed to glob socket pattern")?
        .filter_map(Result::ok)
        .filter(|path| path.exists())
        .collect())
}
