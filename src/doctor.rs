//! `sidekick doctor` — diagnose a sidekick install.
//!
//! On a terminal, checks animate: every row prints up front with a spinner,
//! and they resolve one at a time. The first failure halts the cascade and
//! the remaining rows render as skipped. When stdout is not a terminal we
//! just run everything sequentially and print the final block.
//!
//! Per-harness rows are driven by [`crate::harness::registry`] — each
//! harness is a unit struct implementing [`crate::harness::Harness`].
//! Doctor doesn't branch on harness identity itself.

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::analytics::event::{Decision, Event, ToolKind};
use crate::analytics::store;
use crate::harness::{self, Harness, HarnessCapabilities, HarnessError, HarnessOk};
use crate::utils;

pub(crate) const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAMES_PER_CHECK: u32 = 3;
const FRAME_DELAY: Duration = Duration::from_millis(70);

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

enum RowKind {
    /// Flat row — the existing rendering (marker + label + detail).
    Normal {
        label: &'static str,
        run: fn() -> Check,
        /// Truecolor ANSI params for the label, applied only on Pass.
        accent: Option<&'static str>,
    },
    /// A section title with no marker and no spinner. Always renders as just
    /// its label; the `run` function still gates downstream rows by failing
    /// when no harness was detected.
    Subheader {
        label: &'static str,
        run: fn() -> Check,
    },
    /// A per-harness row that renders nested under the AI-harnesses subheader:
    /// harness name → dim support line → status row → paths.
    Harness(Box<dyn Harness>),
}

impl RowKind {
    fn label(&self) -> &'static str {
        match self {
            RowKind::Normal { label, .. } | RowKind::Subheader { label, .. } => label,
            RowKind::Harness(h) => h.name(),
        }
    }
}

enum RowResult {
    Standard(Check),
    Harness(Result<HarnessOk, HarnessError>),
}

struct Row {
    result: Option<RowResult>,
    skipped: bool,
    /// A prerequisite failure halts the cascade; other failures don't.
    prerequisite: bool,
    kind: RowKind,
}

impl Row {
    fn is_failed(&self) -> bool {
        matches!(
            self.result,
            Some(RowResult::Standard(Check {
                status: Status::Fail { .. },
                ..
            }))
        ) || matches!(self.result, Some(RowResult::Harness(Err(_))))
    }

    /// A failed prerequisite (e.g. missing `nvim`) makes the rest moot, so the
    /// remaining rows are skipped. A failed integration check does not.
    fn halts_cascade(&self) -> bool {
        self.prerequisite && self.is_failed()
    }

    fn run(&mut self) {
        let result = match &self.kind {
            RowKind::Normal { run, .. } | RowKind::Subheader { run, .. } => {
                RowResult::Standard(run())
            }
            RowKind::Harness(h) => RowResult::Harness(h.check()),
        };
        self.result = Some(result);
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

fn normal_row(
    label: &'static str,
    run: fn() -> Check,
    accent: Option<&'static str>,
    prerequisite: bool,
) -> Row {
    Row {
        result: None,
        skipped: false,
        prerequisite,
        kind: RowKind::Normal { label, run, accent },
    }
}

fn subheader_row(label: &'static str, run: fn() -> Check) -> Row {
    Row {
        result: None,
        skipped: false,
        prerequisite: false,
        kind: RowKind::Subheader { label, run },
    }
}

fn harness_row(harness: Box<dyn Harness>) -> Row {
    Row {
        result: None,
        skipped: false,
        prerequisite: false,
        kind: RowKind::Harness(harness),
    }
}

/// The discovery step runs first; per-harness rows are added only for
/// installed harnesses, so an absent harness gets no row.
fn build_rows() -> Vec<Row> {
    let mut rows = vec![
        normal_row("sidekick version", check_version, None, false),
        normal_row("nvim on PATH", check_nvim_on_path, None, true),
        subheader_row("AI harnesses", check_harnesses),
    ];

    for harness in harness::registry::all() {
        if harness.is_installed() {
            rows.push(harness_row(harness));
        }
    }

    rows.push(normal_row("nvim alias", check_shell_alias, None, false));
    rows.push(normal_row(
        "Neovim sockets opened here",
        check_sockets,
        None,
        false,
    ));
    rows.push(normal_row("last activity", check_last_hook, None, false));
    rows
}

fn animate(theme: &Theme, rows: &mut [Row]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut last_height = 0usize;
    let mut spin = 0usize;

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

        rows[i].run();

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
        row.run();
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

    let mut prev_was_harness = false;
    for row in rows {
        let is_harness = matches!(row.kind, RowKind::Harness(_));
        if prev_was_harness && !is_harness {
            writeln!(w)?;
            height += 1;
        }
        for line in render_row(theme, row, spin) {
            writeln!(w, "{line}")?;
            height += 1;
        }
        prev_was_harness = is_harness;
    }

    writeln!(w)?;
    height += 1;
    Ok(height)
}

fn render_row(theme: &Theme, row: &Row, spin: usize) -> Vec<String> {
    if row.skipped {
        let marker = theme.dim("⊝");
        let body = theme.dim(&format!("{} (skipped)", row.kind.label()));
        return vec![format!("  {marker} {body}")];
    }

    match &row.kind {
        RowKind::Subheader { label, .. } => render_subheader(theme, label, &row.result),
        RowKind::Harness(h) => render_harness_row(theme, h.as_ref(), &row.result, spin),
        RowKind::Normal { label, accent, .. } => {
            render_normal_row(theme, label, *accent, &row.result, spin)
        }
    }
}

fn render_normal_row(
    theme: &Theme,
    label: &'static str,
    accent: Option<&'static str>,
    result: &Option<RowResult>,
    spin: usize,
) -> Vec<String> {
    match result {
        None => {
            let marker = theme.cyan(SPINNER_FRAMES[spin % SPINNER_FRAMES.len()]);
            vec![format!("  {marker} {label}")]
        }
        Some(RowResult::Standard(check)) => {
            let marker = match &check.status {
                Status::Pass => theme.green("✓"),
                Status::Fail { .. } => theme.red("✗"),
                Status::Info => theme.dim("·"),
            };
            let label = match (&check.status, accent) {
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
        Some(RowResult::Harness(_)) => unreachable!("normal row produces Standard result"),
    }
}

/// Section title — no marker, no spinner, just the label, padded above. When
/// the discovery check returns Fail (no AI harnesses detected), we surface
/// that failure inline with a remedy in place of the empty section.
fn render_subheader(theme: &Theme, label: &'static str, result: &Option<RowResult>) -> Vec<String> {
    let mut out = vec![String::new()];
    match result {
        None => out.push(format!("  {label}")),
        Some(RowResult::Standard(check)) => match &check.status {
            Status::Fail { remedy } => {
                let marker = theme.red("✗");
                out.push(format!("  {marker} {}", check.label));
                for line in remedy {
                    out.push(format!("      {line}"));
                }
            }
            _ => out.push(format!("  {label}")),
        },
        Some(RowResult::Harness(_)) => unreachable!("subheader produces Standard result"),
    }
    out
}

fn render_harness_row(
    theme: &Theme,
    harness: &dyn Harness,
    result: &Option<RowResult>,
    spin: usize,
) -> Vec<String> {
    // Pending: one line per harness so the report doesn't paint the full
    // nested structure up front. The block expands to 4+ lines only once
    // this row's check resolves.
    let Some(RowResult::Harness(outcome)) = result else {
        let marker = theme.cyan(SPINNER_FRAMES[spin % SPINNER_FRAMES.len()]);
        return vec![format!("    {marker} {}", harness.name())];
    };

    let (bullet, name) = match outcome {
        Ok(_) => (
            theme.wrap(harness.accent(), "·"),
            theme.wrap(harness.accent(), harness.name()),
        ),
        Err(_) => (theme.dim("·"), harness.name().to_string()),
    };
    let mut out = vec![format!("    {bullet} {name}")];

    // Dim support line: `✓ full support` when complete, or `✗ <feature>`
    // for each missing capability when partial.
    out.push(format!(
        "      {}",
        theme.dim(&support_line(&harness.capabilities()))
    ));

    match outcome {
        Ok(ok) => {
            let marker = theme.green("✓");
            out.push(format!("      {marker} {}", ok.label));
            for path in &ok.paths {
                out.push(format!("          {}", theme.dim(&display_path(path))));
            }
        }
        Err(HarnessError::NotConfigured { label, remedy }) => {
            let marker = theme.red("✗");
            out.push(format!("      {marker} {label}"));
            for line in remedy {
                out.push(format!("          {line}"));
            }
        }
        Err(HarnessError::OutOfSync {
            label,
            paths,
            remedy,
        }) => {
            let marker = theme.red("✗");
            out.push(format!("      {marker} {label}"));
            for path in paths {
                out.push(format!("          {}", theme.dim(&display_path(path))));
            }
            for line in remedy {
                out.push(format!("          {line}"));
            }
        }
    }
    out
}

fn support_line(caps: &HarnessCapabilities) -> String {
    if caps.is_full() {
        return "✓ full support".to_string();
    }
    let mut gaps = Vec::new();
    if !caps.edit_deny {
        gaps.push("✗ edit deny");
    }
    if !caps.buffer_refresh {
        gaps.push("✗ buffer refresh");
    }
    if !caps.selection_inject {
        gaps.push("✗ selection injection");
    }
    gaps.join("  ")
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

/// Discover which AI harnesses are present. Drives both the subheader's
/// failure-state copy and (via the same trait method) which per-harness
/// rows get built at all.
fn check_harnesses() -> Check {
    let names: Vec<&'static str> = harness::registry::all()
        .iter()
        .filter(|h| h.is_installed())
        .map(|h| h.name())
        .collect();

    if names.is_empty() {
        Check {
            label: "no AI harness found".into(),
            detail: None,
            status: Status::Fail {
                remedy: vec![
                    "Install Claude Code, Codex, opencode, pi, or Crush — sidekick has nothing to guard without one"
                        .into(),
                ],
            },
        }
    } else {
        // The Info Check is only used for the subheader's success-state
        // dummy. render_subheader prints the section title from the static
        // pending_label, not from this check's label.
        Check {
            label: String::new(),
            detail: None,
            status: Status::Info,
        }
    }
}

#[derive(PartialEq)]
pub(crate) enum AliasStatus {
    Active,
    Missing,
    Unknown,
}

const NVIM_PROBE_SENTINEL: &str = "__sidekick_probe__";

fn probe_nvim_type(shell: &str) -> std::io::Result<String> {
    let out = Command::new(shell)
        .args([
            "-i",
            "-c",
            &format!("echo {NVIM_PROBE_SENTINEL}; type nvim"),
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let isolated = stdout.rsplit(NVIM_PROBE_SENTINEL).next().unwrap_or(&stdout);
    Ok(isolated.to_string())
}

pub(crate) fn nvim_alias_status() -> AliasStatus {
    let Ok(shell) = std::env::var("SHELL") else {
        return AliasStatus::Unknown;
    };
    match probe_nvim_type(&shell) {
        Ok(out) if out.contains("sidekick neovim") => AliasStatus::Active,
        Ok(_) => AliasStatus::Missing,
        Err(_) => AliasStatus::Unknown,
    }
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

    match probe_nvim_type(&shell) {
        Ok(stdout) => {
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
            detail: Some("Ask an AI agent to edit a file to see one.".into()),
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
