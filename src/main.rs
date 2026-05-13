use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

mod action;
mod analytics;
mod constants;
mod demo;
mod doctor;
mod handler;
mod hook;
mod utils;

use analytics::event::{Event, NvimLaunch, StatsView};
use analytics::render::{Renderer, terminal::TerminalRenderer};
use analytics::{TimeRange, aggregate};

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
    /// Show your sidekick story — what Claude did, what got caught.
    Stats {
        /// Time window to summarize.
        #[arg(long, value_enum, default_value_t = StatsRange::Week)]
        range: StatsRange,
        /// Disable ANSI color codes (useful for piping).
        #[arg(long)]
        no_color: bool,
    },
    /// Verify sidekick is installed and wired up correctly.
    Doctor {
        /// Disable ANSI color codes (useful for piping).
        #[arg(long)]
        no_color: bool,
    },
    /// Play the bundled demo inside a media-player TUI.
    Demo,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum StatsRange {
    Week,
    Month,
    Year,
    All,
}

impl From<StatsRange> for TimeRange {
    fn from(r: StatsRange) -> Self {
        match r {
            StatsRange::Week => TimeRange::Week,
            StatsRange::Month => TimeRange::Month,
            StatsRange::Year => TimeRange::Year,
            StatsRange::All => TimeRange::All,
        }
    }
}

/// Handle the 'neovim' command
fn handle_neovim(args: Vec<String>) -> anyhow::Result<()> {
    let pid = std::process::id();
    let socket_path = utils::compute_socket_path_with_pid(pid)?;

    // Record the launch before we hand the process off to nvim via exec.
    // `write_all` on an O_APPEND file goes straight to the kernel — the bytes
    // survive the exec(2) replacement of our process image.
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    analytics::store::append(&Event::NvimLaunch(NvimLaunch {
        at: Utc::now(),
        pid,
        cwd,
        socket_path: socket_path.to_string_lossy().into_owned(),
        args: args.clone(),
    }));

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

fn handle_stats(range: StatsRange, no_color: bool) -> anyhow::Result<()> {
    // Log this view first; the rendered "Nth look today" counts include it.
    let range_label = match range {
        StatsRange::Week => "week",
        StatsRange::Month => "month",
        StatsRange::Year => "year",
        StatsRange::All => "all",
    };
    analytics::store::append(&Event::StatsView(StatsView {
        at: Utc::now(),
        range: range_label.to_string(),
    }));

    let events = analytics::store::read_all()?;
    let stats = aggregate(events, range.into());
    let renderer = TerminalRenderer { color: !no_color };
    let mut stdout = io::stdout().lock();
    renderer.render(&stats, &mut stdout)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hook => handler::handle_hook()?,
        Commands::Neovim { args } => handle_neovim(args)?,
        Commands::Stats { range, no_color } => handle_stats(range, no_color)?,
        Commands::Doctor { no_color } => doctor::run(no_color)?,
        Commands::Demo => demo::run()?,
    }

    Ok(())
}
