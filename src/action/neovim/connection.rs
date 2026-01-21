//! Neovim connection management and multi-instance operations.

use crate::constants::NEOVIM_RPC_TIMEOUT;
use anyhow::{Context, Result};
use neovim_lib::{Neovim, Session};
use std::path::PathBuf;

/// Connect to Neovim via Unix socket and return Neovim client
pub fn connect(socket_path: &PathBuf) -> Result<Neovim> {
    let mut session =
        Session::new_unix_socket(socket_path).context("Failed to connect to Neovim socket")?;
    session.set_timeout(NEOVIM_RPC_TIMEOUT);
    session.start_event_loop();
    Ok(Neovim::new(session))
}

/// Execute a closure for each successfully connected Neovim instance
/// Returns whether any instance was successfully processed
pub fn for_each_instance<F>(socket_paths: &[PathBuf], mut f: F) -> bool
where
    F: FnMut(&mut Neovim) -> Result<()>,
{
    socket_paths
        .iter()
        .filter_map(|path| connect(path).ok())
        .any(|mut nvim| f(&mut nvim).is_ok())
}

/// Fold over successfully connected Neovim instances with early exit support
/// Returns None if no instances were processed, otherwise returns the accumulated value
/// Closure updates accumulator in place and returns whether to continue
pub fn try_fold_instances<T, F>(socket_paths: &[PathBuf], init: T, mut f: F) -> Option<T>
where
    F: FnMut(&mut T, &mut Neovim) -> Result<bool>,
{
    let mut any_processed = false;

    let result = socket_paths
        .iter()
        .filter_map(|path| connect(path).ok())
        .try_fold(init, |mut acc, mut nvim| match f(&mut acc, &mut nvim) {
            Ok(should_continue) => {
                any_processed = true;
                if should_continue { Ok(acc) } else { Err(acc) }
            }
            Err(_) => Ok(acc),
        });

    any_processed.then(|| result.unwrap_or_else(|acc| acc))
}

/// Collect all non-None results from all Neovim instances
pub fn collect_all<T, F>(socket_paths: &[PathBuf], mut f: F) -> Vec<T>
where
    F: FnMut(&mut Neovim) -> Result<Option<T>>,
{
    socket_paths
        .iter()
        .filter_map(|path| connect(path).ok())
        .filter_map(|mut nvim| f(&mut nvim).ok().flatten())
        .collect()
}
