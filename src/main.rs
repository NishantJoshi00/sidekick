use anyhow::Context;
use clap::{Parser, Subcommand};
use std::env;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

mod action;
mod handler;
mod hook;

#[derive(Parser)]
#[command(name = "sidekick")]
#[command(about = "Claude Code hook handler and Neovim integration", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Handle Claude Code hooks
    Hook,
    /// Launch Neovim with a socket based on current directory
    Neovim {
        /// Arguments to pass to Neovim
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}


/// Handle the 'neovim' command
fn handle_neovim(args: Vec<String>) -> anyhow::Result<()> {
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

    // Build neovim command with --listen flag
    let mut cmd = Command::new("nvim");
    cmd.arg("--listen").arg(&socket_path);

    // Add all trailing arguments
    cmd.args(&args);

    // Execute neovim, replacing current process
    let err = cmd.exec();

    // If exec returns, it failed
    Err(anyhow::anyhow!("Failed to execute nvim: {}", err))
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hook => handler::handle_hook()?,
        Commands::Neovim { args } => handle_neovim(args)?,
    }

    Ok(())
}
