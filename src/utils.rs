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
    let mut sockets = Vec::new();

    // Use glob to find matching sockets
    for path in (glob::glob(&pattern).context("Failed to glob socket pattern")?).flatten() {
        if path.exists() {
            sockets.push(path);
        }
    }

    Ok(sockets)
}
