use clap::{Parser, Subcommand};
use std::os::unix::process::CommandExt;
use std::process::Command;

mod action;
mod constants;
mod handler;
mod hook;
mod utils;

#[derive(Parser)]
#[command(name = "sidekick")]
#[command(about = "Claude Code hook handler and editor integration (Neovim + VSCode)", long_about = None)]
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
    /// Show socket information for the current directory
    Info,
}

/// Handle the 'neovim' command
fn handle_neovim(args: Vec<String>) -> anyhow::Result<()> {
    let pid = std::process::id();
    let socket_path = utils::compute_neovim_socket_path(pid)?;

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

/// Handle the 'info' command - show socket information
fn handle_info() -> anyhow::Result<()> {
    let pid = std::process::id();

    // Show expected socket paths
    let nvim_socket = utils::compute_neovim_socket_path(pid)?;
    let vscode_socket = utils::compute_vscode_socket_path(pid)?;

    println!("Socket Information for Current Directory");
    println!("========================================");
    println!();
    println!("Expected Neovim socket:  {}", nvim_socket.display());
    println!("Expected VSCode socket:  {}", vscode_socket.display());
    println!();

    // Show discovered sockets
    let nvim_sockets = utils::find_neovim_sockets()?;
    let vscode_sockets = utils::find_vscode_sockets()?;

    println!("Discovered Neovim sockets: {}", nvim_sockets.len());
    for socket in &nvim_sockets {
        println!("  - {}", socket.display());
    }

    println!();
    println!("Discovered VSCode sockets: {}", vscode_sockets.len());
    for socket in &vscode_sockets {
        println!("  - {}", socket.display());
    }

    if nvim_sockets.is_empty() && vscode_sockets.is_empty() {
        println!();
        println!("No editor sockets found. Start an editor with Sidekick integration:");
        println!("  - Neovim: Run `sidekick neovim` or add `alias nvim='sidekick neovim'`");
        println!("  - VSCode: Install the vscode-sidekick extension from plugins/vscode-sidekick");
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hook => handler::handle_hook()?,
        Commands::Neovim { args } => handle_neovim(args)?,
        Commands::Info => handle_info()?,
    }

    Ok(())
}
