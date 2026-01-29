//! Socket path utilities for editor instance discovery.
//!
//! This module provides functions for computing and discovering Unix socket paths
//! for Neovim and VSCode instances. Sockets are named using a deterministic hash of the
//! current working directory and the process ID.
//!
//! # Socket Naming Schemes
//!
//! - Neovim: `/tmp/<blake3(cwd)>-<pid>.sock`
//! - VSCode: `/tmp/<blake3(cwd)>-vscode-<pid>.sock`
//!
//! This allows:
//! - Multiple editor instances per directory (different PIDs)
//! - Easy discovery of all instances for a directory (glob pattern)
//! - No socket conflicts between different directories
//! - Separate discovery of Neovim vs VSCode instances
//!
//! # Example
//!
//! ```no_run
//! use sidekick::utils;
//!
//! // Compute socket path for current process
//! let pid = std::process::id();
//! let nvim_socket = utils::compute_neovim_socket_path(pid).unwrap();
//! let vscode_socket = utils::compute_vscode_socket_path(pid).unwrap();
//!
//! // Find all editor instances in this directory
//! let nvim_sockets = utils::find_neovim_sockets().unwrap();
//! let vscode_sockets = utils::find_vscode_sockets().unwrap();
//! ```

use anyhow::Context;
use std::env;
use std::path::PathBuf;

/// Compute blake3 hash of the canonicalized current working directory
fn compute_cwd_hash() -> anyhow::Result<String> {
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("Failed to canonicalize current directory")?;

    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    Ok(hash.to_hex().to_string())
}

/// Compute Neovim socket path based on current working directory hash and process ID
///
/// Pattern: `/tmp/<blake3(cwd)>-<pid>.sock`
pub fn compute_neovim_socket_path(pid: u32) -> anyhow::Result<PathBuf> {
    let hash_hex = compute_cwd_hash()?;
    Ok(PathBuf::from(format!("/tmp/{}-{}.sock", hash_hex, pid)))
}

/// Compute VSCode socket path based on current working directory hash and process ID
///
/// Pattern: `/tmp/<blake3(cwd)>-vscode-<pid>.sock`
pub fn compute_vscode_socket_path(pid: u32) -> anyhow::Result<PathBuf> {
    let hash_hex = compute_cwd_hash()?;
    Ok(PathBuf::from(format!(
        "/tmp/{}-vscode-{}.sock",
        hash_hex, pid
    )))
}

/// Find all Neovim socket paths matching the current working directory hash
///
/// Discovers sockets with pattern: `/tmp/<blake3(cwd)>-*.sock`
/// Excludes VSCode sockets (those containing "-vscode-")
pub fn find_neovim_sockets() -> anyhow::Result<Vec<PathBuf>> {
    let hash_hex = compute_cwd_hash()?;
    let pattern = format!("/tmp/{}-*.sock", hash_hex);

    Ok(glob::glob(&pattern)
        .context("Failed to glob socket pattern")?
        .filter_map(Result::ok)
        .filter(|path| {
            path.exists()
                && !path
                    .file_name()
                    .map(|s| s.to_string_lossy().contains("-vscode-"))
                    .unwrap_or(false)
        })
        .collect())
}

/// Find all VSCode socket paths matching the current working directory hash
///
/// Discovers sockets with pattern: `/tmp/<blake3(cwd)>-vscode-*.sock`
pub fn find_vscode_sockets() -> anyhow::Result<Vec<PathBuf>> {
    let hash_hex = compute_cwd_hash()?;
    let pattern = format!("/tmp/{}-vscode-*.sock", hash_hex);

    Ok(glob::glob(&pattern)
        .context("Failed to glob socket pattern")?
        .filter_map(Result::ok)
        .filter(|path| path.exists())
        .collect())
}
