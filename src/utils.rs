use anyhow::Context;
use std::env;
use std::path::PathBuf;

/// Compute socket path based on current working directory hash
pub fn compute_socket_path() -> anyhow::Result<PathBuf> {
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("Failed to canonicalize current directory")?;

    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    let hash_hex = hash.to_hex();

    Ok(PathBuf::from(format!("/tmp/{}.sock", hash_hex)))
}
