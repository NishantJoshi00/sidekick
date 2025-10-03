use anyhow::Context;
use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use crate::action::{neovim::NeovimAction, Action};
use crate::hook;

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let _hook = hook::parse_hook(&input)?;

    // Get absolute path of current working directory
    let cwd = env::current_dir().context("Failed to get current working directory")?;
    let cwd_absolute = cwd
        .canonicalize()
        .context("Failed to canonicalize current directory")?;

    // Compute blake3 hash of the absolute path
    let hash = blake3::hash(cwd_absolute.to_string_lossy().as_bytes());
    let hash_hex = hash.to_hex();

    // Create socket path
    let socket_path = PathBuf::from(format!("/tmp/{}.sock", hash_hex));

    // Check if socket exists
    if socket_path.exists() {
        // Create Neovim client and send message
        let nvim_action = NeovimAction::new(socket_path);

        // Try to send message, but don't fail the hook if it fails
        if let Err(e) = nvim_action.send_message("Hi! from Claude") {
            eprintln!("Warning: Failed to send message to Neovim: {}", e);
        }
    }

    // Return normal hook output
    io::stdout().write_all(hook::HookOutput::new().to_json()?.as_bytes())?;

    Ok(())
}
