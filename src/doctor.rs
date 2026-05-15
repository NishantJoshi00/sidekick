//! `sidekick doctor` — diagnose a sidekick install.
//!
//! On a terminal, checks animate: every row prints up front with a spinner,
//! and they resolve one at a time. The first failure halts the cascade and
//! the remaining rows render as skipped. When stdout is not a terminal we
//! just run everything sequentially and print the final block.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::analytics::event::{Decision, Event, ToolKind};
use crate::analytics::store;
use crate::utils;

pub(crate) const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAMES_PER_CHECK: u32 = 3;
const FRAME_DELAY: Duration = Duration::from_millis(70);

// Brand accents (truecolor ANSI params) for the AI-integration rows, applied
// to the label only when that integration is configured correctly.
pub(crate) const CLAUDE_ACCENT: &str = "38;2;217;119;87"; // #D97757
pub(crate) const OPENCODE_ACCENT: &str = "38;2;91;155;213"; // #5B9BD5
pub(crate) const PI_ACCENT: &str = "38;2;157;124;216"; // #9D7CD8

enum Status {
    Pass,
    Fail { remedy: Vec<String> },
    Info,
}

struct Check {
    label: String,
    detail: Option<String>,
    status: Status,
}

struct Row {
    pending_label: &'static str,
    run: fn() -> Check,
    result: Option<Check>,
    skipped: bool,
    /// Truecolor ANSI params for the label, applied only when the check passes.
    accent: Option<&'static str>,
    /// A prerequisite failure halts the cascade; other failures don't.
    prerequisite: bool,
}

impl Row {
    fn is_failed(&self) -> bool {
        matches!(
            self.result,
            Some(Check {
                status: Status::Fail { .. },
                ..
            })
        )
    }

    /// A failed prerequisite (e.g. missing `nvim`) makes the rest moot, so the
    /// remaining rows are skipped. A failed integration check does not.
    fn halts_cascade(&self) -> bool {
        self.prerequisite && self.is_failed()
    }
}

/// Runs every check and renders the report. Returns whether anything failed —
/// the caller owns the exit code, so `--fix` can run before the process ends.
pub fn run(no_color: bool, fix: bool) -> anyhow::Result<bool> {
    let theme = Theme::new(!no_color);
    let mut rows = build_rows();

    if io::stdout().is_terminal() {
        animate(&theme, &mut rows)?;
    } else {
        run_static(&mut rows);
        let mut stdout = io::stdout().lock();
        render_block_to(&mut stdout, &theme, &rows, 0)?;
    }

    let any_failed = rows.iter().any(Row::is_failed);

    // When something's wrong, point at --fix — unless --fix is already running.
    if any_failed && !fix {
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "  {}sidekick doctor --fix{}",
            theme.dim("Run "),
            theme.dim(" to fix the issues above.")
        )?;
        writeln!(stdout)?;
    }

    Ok(any_failed)
}

fn row(
    pending_label: &'static str,
    run: fn() -> Check,
    accent: Option<&'static str>,
    prerequisite: bool,
) -> Row {
    Row {
        pending_label,
        run,
        result: None,
        skipped: false,
        accent,
        prerequisite,
    }
}

/// The discovery step runs first; the per-harness rows below it are built only
/// for the AI harnesses actually present, so an absent harness gets no row.
fn build_rows() -> Vec<Row> {
    let mut rows = vec![
        row("sidekick version", check_version, None, false),
        row("nvim on PATH", check_nvim_on_path, None, true),
        row("AI harnesses", check_harnesses, None, false),
    ];

    if uses_claude_code() {
        rows.push(row(
            "Claude Code hook registered",
            check_claude_hook,
            Some(CLAUDE_ACCENT),
            false,
        ));
    }
    if uses_opencode() {
        rows.push(row(
            "opencode plugin",
            check_opencode_plugin,
            Some(OPENCODE_ACCENT),
            false,
        ));
    }
    if uses_pi() {
        rows.push(row(
            "pi extension",
            check_pi_extension,
            Some(PI_ACCENT),
            false,
        ));
    }

    rows.push(row("nvim alias", check_shell_alias, None, false));
    rows.push(row(
        "Neovim sockets opened here",
        check_sockets,
        None,
        false,
    ));
    rows.push(row("last activity", check_last_hook, None, false));
    rows
}

fn animate(theme: &Theme, rows: &mut [Row]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut last_height = 0usize;
    let mut spin = 0usize;

    // Hide cursor while we redraw; restore on the way out (incl. failure path).
    write!(stdout, "\x1b[?25l")?;
    let result = animate_inner(&mut stdout, theme, rows, &mut last_height, &mut spin);
    write!(stdout, "\x1b[?25h")?;
    stdout.flush()?;
    result
}

fn animate_inner(
    stdout: &mut impl Write,
    theme: &Theme,
    rows: &mut [Row],
    last_height: &mut usize,
    spin: &mut usize,
) -> io::Result<()> {
    redraw(stdout, theme, rows, *spin, last_height)?;

    for i in 0..rows.len() {
        for _ in 0..FRAMES_PER_CHECK {
            *spin = spin.wrapping_add(1);
            thread::sleep(FRAME_DELAY);
            redraw(stdout, theme, rows, *spin, last_height)?;
        }

        rows[i].result = Some((rows[i].run)());

        if rows[i].halts_cascade() {
            for row in rows.iter_mut().skip(i + 1) {
                row.skipped = true;
            }
            redraw(stdout, theme, rows, *spin, last_height)?;
            return Ok(());
        }

        redraw(stdout, theme, rows, *spin, last_height)?;
    }
    Ok(())
}

fn redraw(
    w: &mut impl Write,
    theme: &Theme,
    rows: &[Row],
    spin: usize,
    last_height: &mut usize,
) -> io::Result<()> {
    if *last_height > 0 {
        write!(w, "\x1b[{}A\r", *last_height)?;
    }
    write!(w, "\x1b[J")?;
    *last_height = render_block_to(w, theme, rows, spin)?;
    w.flush()
}

fn run_static(rows: &mut [Row]) {
    let mut failed = false;
    for row in rows.iter_mut() {
        if failed {
            row.skipped = true;
            continue;
        }
        row.result = Some((row.run)());
        if row.halts_cascade() {
            failed = true;
        }
    }
}

fn render_block_to(
    w: &mut impl Write,
    theme: &Theme,
    rows: &[Row],
    spin: usize,
) -> io::Result<usize> {
    let mut height = 0;
    writeln!(w)?;
    height += 1;
    writeln!(w, "  {}", theme.bold("sidekick doctor"))?;
    height += 1;
    writeln!(w)?;
    height += 1;

    for row in rows {
        for line in render_row(theme, row, spin) {
            writeln!(w, "{line}")?;
            height += 1;
        }
    }

    writeln!(w)?;
    height += 1;
    Ok(height)
}

fn render_row(theme: &Theme, row: &Row, spin: usize) -> Vec<String> {
    if row.skipped {
        let marker = theme.dim("⊝");
        let body = theme.dim(&format!("{} (skipped)", row.pending_label));
        return vec![format!("  {marker} {body}")];
    }

    match &row.result {
        None => {
            let marker = theme.cyan(SPINNER_FRAMES[spin % SPINNER_FRAMES.len()]);
            vec![format!("  {marker} {}", row.pending_label)]
        }
        Some(check) => {
            let marker = match &check.status {
                Status::Pass => theme.green("✓"),
                Status::Fail { .. } => theme.red("✗"),
                Status::Info => theme.dim("·"),
            };
            let label = match (&check.status, row.accent) {
                (Status::Pass, Some(code)) => theme.wrap(code, &check.label),
                _ => check.label.clone(),
            };
            let mut out = vec![format!("  {marker} {label}")];
            if let Some(detail) = &check.detail {
                for line in detail.lines() {
                    out.push(format!("      {}", theme.dim(line)));
                }
            }
            if let Status::Fail { remedy } = &check.status {
                for line in remedy {
                    out.push(format!("      {line}"));
                }
            }
            out
        }
    }
}

fn check_version() -> Check {
    let version = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()
        .ok()
        .map(|p| display_path(&p))
        .unwrap_or_else(|| "(unknown path)".to_string());
    Check {
        label: format!("sidekick v{version} on PATH"),
        detail: Some(exe),
        status: Status::Pass,
    }
}

fn check_nvim_on_path() -> Check {
    match Command::new("nvim").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let first_line = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("nvim")
                .trim()
                .to_string();
            let label = if first_line.is_empty() {
                "nvim on PATH".to_string()
            } else {
                format!("{first_line} on PATH")
            };
            Check {
                label,
                detail: None,
                status: Status::Pass,
            }
        }
        _ => Check {
            label: "Neovim (`nvim`) not on PATH".into(),
            detail: None,
            status: Status::Fail {
                remedy: vec!["Install Neovim: https://neovim.io/".into()],
            },
        },
    }
}

/// Discover which AI harnesses are present. Drives both this row's summary and
/// (via the same `uses_*` checks) which per-harness rows get built at all.
fn check_harnesses() -> Check {
    let mut found: Vec<&str> = Vec::new();
    if uses_claude_code() {
        found.push("Claude Code");
    }
    if uses_opencode() {
        found.push("opencode");
    }
    if uses_pi() {
        found.push("pi");
    }

    if found.is_empty() {
        Check {
            label: "no AI harness found".into(),
            detail: None,
            status: Status::Fail {
                remedy: vec![
                    "Install Claude Code, opencode, or pi — sidekick has nothing to guard without one"
                        .into(),
                ],
            },
        }
    } else {
        Check {
            label: format!("AI harnesses: {}", found.join(", ")),
            detail: None,
            status: Status::Info,
        }
    }
}

/// Config files that already wire up `sidekick hook` for Claude Code.
pub(crate) fn claude_hook_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".claude").join("settings.json"));
        candidates.push(home.join(".claude").join("settings.local.json"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".claude").join("settings.json"));
    }

    for path in &candidates {
        if file_mentions_sidekick_hook(path) {
            matched.push(path.clone());
        }
    }

    if let Some(home) = dirs::home_dir() {
        walk_for_json_mentioning_hook(&home.join(".claude").join("plugins"), &mut matched, 4);
    }

    matched.sort();
    matched.dedup();
    matched
}

fn check_claude_hook() -> Check {
    let matched = claude_hook_files();

    if !matched.is_empty() {
        let detail = matched
            .iter()
            .map(|p| display_path(p))
            .collect::<Vec<_>>()
            .join("\n");
        return Check {
            label: "Claude Code hook registered".into(),
            detail: Some(detail),
            status: Status::Pass,
        };
    }

    Check {
        label: "Claude Code hook not registered".into(),
        detail: None,
        status: Status::Fail {
            remedy: vec![
                "Install the plugin:  /plugin install sidekick@nishant-plugins".into(),
                "Or add `sidekick hook` to ~/.claude/settings.json".into(),
            ],
        },
    }
}

/// Plugin files that already wire up sidekick for opencode.
pub(crate) fn opencode_plugin_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    // opencode globs `{plugin,plugins}/*.{ts,js}` under its global config dir
    // (~/.config/opencode) and per-project (.opencode).
    let mut plugin_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        plugin_dirs.push(home.join(".config").join("opencode"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        plugin_dirs.push(cwd.join(".opencode"));
    }
    for base in &plugin_dirs {
        for dir in ["plugin", "plugins"] {
            for ext in ["ts", "js"] {
                let candidate = base.join(dir).join(format!("sidekick.{ext}"));
                if candidate.is_file() {
                    matched.push(candidate);
                }
            }
        }
    }

    matched.sort();
    matched.dedup();
    matched
}

fn check_opencode_plugin() -> Check {
    let matched = opencode_plugin_files();

    if matched.is_empty() {
        return Check {
            label: "opencode plugin not installed".into(),
            detail: None,
            status: Status::Fail {
                remedy: vec![
                    "Drop plugins/opencode/sidekick.ts into ~/.config/opencode/plugin/".into(),
                ],
            },
        };
    }

    let detail = matched
        .iter()
        .map(|p| display_path(p))
        .collect::<Vec<_>>()
        .join("\n");

    // Presence isn't enough — a stale or hand-edited plugin is missing fixes.
    // The binary embeds the canonical plugin, so compare byte-for-byte.
    let all_current = matched.iter().all(|p| {
        std::fs::read_to_string(p).ok().as_deref() == Some(crate::fix::OPENCODE_PLUGIN_SRC)
    });

    if all_current {
        Check {
            label: "opencode plugin installed".into(),
            detail: Some(detail),
            status: Status::Pass,
        }
    } else {
        Check {
            label: "opencode plugin out of sync".into(),
            detail: Some(detail),
            status: Status::Fail {
                remedy: vec!["The installed plugin differs from this sidekick build.".into()],
            },
        }
    }
}

/// Extension files that already wire up sidekick for the pi coding agent.
pub(crate) fn pi_extension_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    // pi loads `extensions/*.{ts,js}` from its global agent config dir
    // (~/.pi/agent) and per-project (.pi).
    let mut extension_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        extension_dirs.push(home.join(".pi").join("agent").join("extensions"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        extension_dirs.push(cwd.join(".pi").join("extensions"));
    }
    for dir in &extension_dirs {
        for ext in ["ts", "js"] {
            let candidate = dir.join(format!("sidekick.{ext}"));
            if candidate.is_file() {
                matched.push(candidate);
            }
        }
    }

    matched.sort();
    matched.dedup();
    matched
}

fn check_pi_extension() -> Check {
    let matched = pi_extension_files();

    if matched.is_empty() {
        return Check {
            label: "pi extension not installed".into(),
            detail: None,
            status: Status::Fail {
                remedy: vec!["Drop plugins/pi/sidekick.ts into ~/.pi/agent/extensions/".into()],
            },
        };
    }

    let detail = matched
        .iter()
        .map(|p| display_path(p))
        .collect::<Vec<_>>()
        .join("\n");

    // Presence isn't enough — a stale or hand-edited extension is missing
    // fixes. The binary embeds the canonical extension, so compare byte-for-byte.
    let all_current = matched
        .iter()
        .all(|p| std::fs::read_to_string(p).ok().as_deref() == Some(crate::fix::PI_EXTENSION_SRC));

    if all_current {
        Check {
            label: "pi extension installed".into(),
            detail: Some(detail),
            status: Status::Pass,
        }
    } else {
        Check {
            label: "pi extension out of sync".into(),
            detail: Some(detail),
            status: Status::Fail {
                remedy: vec!["The installed extension differs from this sidekick build.".into()],
            },
        }
    }
}

/// Whether an executable of this name is on `$PATH`.
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// Whether this machine looks like a Claude Code user.
pub(crate) fn uses_claude_code() -> bool {
    binary_on_path("claude")
        || dirs::home_dir()
            .map(|h| h.join(".claude").is_dir())
            .unwrap_or(false)
}

#[derive(PartialEq)]
pub(crate) enum AliasStatus {
    Active,
    Missing,
    Unknown,
}

/// Runtime check of whether `nvim` resolves to `sidekick neovim` in the login
/// shell — the same probe `check_shell_alias` renders, shared so `--fix` never
/// offers an alias that's already live.
pub(crate) fn nvim_alias_status() -> AliasStatus {
    let Ok(shell) = std::env::var("SHELL") else {
        return AliasStatus::Unknown;
    };
    match Command::new(&shell)
        .args(["-i", "-c", "type nvim"])
        .output()
    {
        Ok(out) => {
            if String::from_utf8_lossy(&out.stdout).contains("sidekick neovim") {
                AliasStatus::Active
            } else {
                AliasStatus::Missing
            }
        }
        Err(_) => AliasStatus::Unknown,
    }
}

/// Whether this machine looks like an opencode user.
pub(crate) fn uses_opencode() -> bool {
    binary_on_path("opencode")
        || dirs::home_dir()
            .map(|h| h.join(".config").join("opencode").is_dir())
            .unwrap_or(false)
}

/// Whether this machine looks like a pi coding agent user. The `~/.pi/agent`
/// directory is pi-specific; `pi` alone is a short name that could collide.
pub(crate) fn uses_pi() -> bool {
    binary_on_path("pi")
        || dirs::home_dir()
            .map(|h| h.join(".pi").join("agent").is_dir())
            .unwrap_or(false)
}

fn check_shell_alias() -> Check {
    let Ok(shell) = std::env::var("SHELL") else {
        return Check {
            label: "nvim alias: $SHELL is not set".into(),
            detail: None,
            status: Status::Info,
        };
    };

    let shell_name = Path::new(&shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("shell")
        .to_string();

    // `-i` makes the shell source the user's rc files (.zshrc, .bashrc, …)
    // so aliases defined there resolve. `type nvim` works in bash/zsh/fish.
    match Command::new(&shell)
        .args(["-i", "-c", "type nvim"])
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("sidekick neovim") {
                Check {
                    label: format!("nvim alias: nvim → sidekick neovim ({shell_name})"),
                    detail: None,
                    status: Status::Pass,
                }
            } else {
                let current =
                    first_meaningful_line(&stdout).unwrap_or_else(|| "(no output)".to_string());
                Check {
                    label: format!("nvim alias not set ({shell_name})"),
                    detail: Some(format!("`type nvim` → {current}")),
                    status: Status::Fail {
                        remedy: vec![format!(
                            "Add to your {shell_name} config:  alias nvim='sidekick neovim'"
                        )],
                    },
                }
            }
        }
        Err(e) => Check {
            label: format!("nvim alias: couldn't run {shell_name}"),
            detail: Some(e.to_string()),
            status: Status::Info,
        },
    }
}

fn first_meaningful_line(s: &str) -> Option<String> {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

fn check_sockets() -> Check {
    match utils::find_matching_sockets() {
        Ok(sockets) if !sockets.is_empty() => {
            let count = sockets.len();
            let detail = sockets
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            Check {
                label: format!(
                    "{count} Neovim socket{} opened here",
                    if count == 1 { "" } else { "s" }
                ),
                detail: Some(detail),
                status: Status::Info,
            }
        }
        _ => Check {
            label: "no Neovim opened here".into(),
            detail: None,
            status: Status::Info,
        },
    }
}

fn check_last_hook() -> Check {
    let last = store::read_all()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| match e {
            Event::HookDecision(d) => Some(d),
            _ => None,
        })
        .max_by_key(|d| d.at);

    match last {
        Some(d) => {
            let when = relative_time(d.at);
            let tool = match d.tool {
                ToolKind::Edit => "Edit",
                ToolKind::Write => "Write",
                ToolKind::MultiEdit => "MultiEdit",
            };
            let decision = match d.decision {
                Decision::Allow => "allowed",
                Decision::Deny => "blocked",
            };
            let file = Path::new(&d.file)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| d.file.clone());
            Check {
                label: format!("last activity: {when}"),
                detail: Some(format!("{decision} · {tool} · {file}")),
                status: Status::Info,
            }
        }
        None => Check {
            label: "last activity: never".into(),
            detail: Some("Ask Claude to edit a file to see one.".into()),
            status: Status::Info,
        },
    }
}

fn relative_time(at: DateTime<Utc>) -> String {
    let secs = Utc::now().signed_duration_since(at).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

fn file_mentions_sidekick_hook(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|c| c.contains("sidekick hook"))
        .unwrap_or(false)
}

fn walk_for_json_mentioning_hook(dir: &Path, matched: &mut Vec<PathBuf>, depth: usize) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_for_json_mentioning_hook(&path, matched, depth - 1);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json")
            && file_mentions_sidekick_hook(&path)
        {
            matched.push(path);
        }
    }
}

pub(crate) fn display_path(p: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = p.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    p.display().to_string()
}

pub(crate) struct Theme {
    color: bool,
}

impl Theme {
    pub(crate) fn new(color: bool) -> Self {
        Self { color }
    }
    pub(crate) fn wrap(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    pub(crate) fn green(&self, s: &str) -> String {
        self.wrap("32", s)
    }
    pub(crate) fn red(&self, s: &str) -> String {
        self.wrap("31", s)
    }
    pub(crate) fn cyan(&self, s: &str) -> String {
        self.wrap("36", s)
    }
    pub(crate) fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    pub(crate) fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
}
