//! VSCode connection management and multi-instance operations.

use super::rpc::RPCClient;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Connect to VSCode via Unix socket and return RPC client
pub fn connect(socket_path: &Path) -> Result<RPCClient> {
    RPCClient::connect(socket_path)
}

/// Execute a closure for each successfully connected VSCode instance
/// Returns whether any instance was successfully processed
pub fn for_each_instance<F>(socket_paths: &[PathBuf], mut f: F) -> bool
where
    F: FnMut(&mut RPCClient) -> Result<()>,
{
    socket_paths
        .iter()
        .filter_map(|path| connect(path).ok())
        .any(|mut client| f(&mut client).is_ok())
}

/// Fold over successfully connected VSCode instances with early exit support
/// Returns None if no instances were processed, otherwise returns the accumulated value
/// Closure updates accumulator in place and returns whether to continue
pub fn try_fold_instances<T, F>(socket_paths: &[PathBuf], init: T, mut f: F) -> Option<T>
where
    F: FnMut(&mut T, &mut RPCClient) -> Result<bool>,
{
    let mut any_processed = false;

    let result = socket_paths
        .iter()
        .filter_map(|path| connect(path).ok())
        .try_fold(init, |mut acc, mut client| match f(&mut acc, &mut client) {
            Ok(should_continue) => {
                any_processed = true;
                if should_continue { Ok(acc) } else { Err(acc) }
            }
            Err(_) => Ok(acc),
        });

    any_processed.then(|| result.unwrap_or_else(|acc| acc))
}

/// Collect all non-None results from all VSCode instances
pub fn collect_all<T, F>(socket_paths: &[PathBuf], mut f: F) -> Vec<T>
where
    F: FnMut(&mut RPCClient) -> Result<Option<T>>,
{
    socket_paths
        .iter()
        .filter_map(|path| connect(path).ok())
        .filter_map(|mut client| f(&mut client).ok().flatten())
        .collect()
}
