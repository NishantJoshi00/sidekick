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
use std::path::{Path, PathBuf};

/// Compute socket path based on current working directory hash and process ID
pub fn compute_socket_path_with_pid(pid: u32) -> anyhow::Result<PathBuf> {
    let cwd = env::current_dir().context("couldn't read current directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("couldn't resolve current directory")?;

    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    let hash_hex = hash.to_hex();

    Ok(PathBuf::from(format!("/tmp/{}-{}.sock", hash_hex, pid)))
}

/// Find all socket paths matching the current working directory hash
pub fn find_matching_sockets() -> anyhow::Result<Vec<PathBuf>> {
    let cwd = env::current_dir().context("couldn't read current directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("couldn't resolve current directory")?;
    sockets_for_dir(&cwd_absolute)
}

/// Walk up from `file_path` looking for a directory whose hash matches an
/// existing socket. Used as a fallback when cwd-based discovery returns
/// nothing — bridge harnesses whose plugin runtime sandboxes process.cwd()
/// (e.g. Amp running from `~/.config/amp/plugins`) lose the cwd signal
/// entirely, but the file path the agent is editing is absolute and
/// usually lives inside the directory where Neovim was launched.
pub fn find_sockets_for_path(file_path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let absolute = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        env::current_dir()
            .context("couldn't read current directory")?
            .join(file_path)
    };

    let mut current = absolute.parent();
    while let Some(dir) = current {
        // canonicalize the ancestor where possible — compute_socket_path_with_pid
        // canonicalizes too, so the hashes only match if we do the same.
        let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let sockets = sockets_for_dir(&canonical)?;
        if !sockets.is_empty() {
            return Ok(sockets);
        }
        current = dir.parent();
    }
    Ok(Vec::new())
}

fn sockets_for_dir(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let hash = blake3::hash(dir.to_string_lossy().as_bytes());
    let pattern = format!("/tmp/{}-*.sock", hash.to_hex());
    Ok(glob::glob(&pattern)
        .context("couldn't search for Neovim sockets")?
        .filter_map(Result::ok)
        .filter(|path| path.exists())
        .collect())
}
