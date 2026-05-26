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
pub(crate) const CODEX_ACCENT: &str = "38;2;16;185;129"; // #10B981
pub(crate) const OPENCODE_ACCENT: &str = "38;2;91;155;213"; // #5B9BD5
pub(crate) const PI_ACCENT: &str = "38;2;157;124;216"; // #9D7CD8
pub(crate) const CRUSH_ACCENT: &str = "38;2;236;72;153"; // #EC4899

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

/// What sidekick can do with this harness. Drives the doctor's `support:`
/// line and the README capability matrix. A `true` cell means sidekick fully
/// exercises that capability on this harness; `false` is an upstream gap.
#[derive(Clone, Copy)]
pub(crate) struct HarnessCapabilities {
    pub(crate) edit_deny: bool,
    pub(crate) buffer_refresh: bool,
    pub(crate) selection_inject: bool,
}

impl HarnessCapabilities {
    fn is_full(&self) -> bool {
        self.edit_deny && self.buffer_refresh && self.selection_inject
    }
}

pub(crate) const CLAUDE_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    edit_deny: true,
    buffer_refresh: true,
    selection_inject: true,
};
pub(crate) const CODEX_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    edit_deny: true,
    buffer_refresh: true,
    selection_inject: true,
};
pub(crate) const OPENCODE_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    edit_deny: true,
    buffer_refresh: true,
    selection_inject: true,
};
pub(crate) const PI_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    edit_deny: true,
    buffer_refresh: true,
    selection_inject: true,
};
pub(crate) const CRUSH_CAPABILITIES: HarnessCapabilities = HarnessCapabilities {
    edit_deny: true,
    buffer_refresh: false,
    selection_inject: false,
};

struct HarnessMeta {
    name: &'static str,
    accent: &'static str,
    capabilities: HarnessCapabilities,
}

enum RowKind {
    /// Flat row — the existing rendering (marker + label + detail).
    Normal,
    /// A section title with no marker and no spinner. Always renders as just
    /// its label; the `run` function still gates downstream rows by failing
    /// when no harness was detected.
    Subheader,
    /// A per-harness row that renders nested under the AI-harnesses subheader:
    /// harness name → dim support line → status row → paths.
    Harness(HarnessMeta),
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
    kind: RowKind,
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
        kind: RowKind::Normal,
    }
}

fn subheader_row(label: &'static str, run: fn() -> Check) -> Row {
    Row {
        pending_label: label,
        run,
        result: None,
        skipped: false,
        accent: None,
        prerequisite: false,
        kind: RowKind::Subheader,
    }
}

fn harness_row(
    pending_label: &'static str,
    run: fn() -> Check,
    name: &'static str,
    accent: &'static str,
    capabilities: HarnessCapabilities,
) -> Row {
    Row {
        pending_label,
        run,
        result: None,
        skipped: false,
        accent: Some(accent),
        prerequisite: false,
        kind: RowKind::Harness(HarnessMeta {
            name,
            accent,
            capabilities,
        }),
    }
}

/// The discovery step runs first; the per-harness rows below it are built only
/// for the AI harnesses actually present, so an absent harness gets no row.
fn build_rows() -> Vec<Row> {
    let mut rows = vec![
        row("sidekick version", check_version, None, false),
        row("nvim on PATH", check_nvim_on_path, None, true),
        subheader_row("AI harnesses", check_harnesses),
    ];

    if uses_claude_code() {
        rows.push(harness_row(
            "hook registered",
            check_claude_hook,
            "Claude Code",
            CLAUDE_ACCENT,
            CLAUDE_CAPABILITIES,
        ));
    }
    if uses_codex() {
        rows.push(harness_row(
            "plugin installed",
            check_codex_plugin,
            "Codex",
            CODEX_ACCENT,
            CODEX_CAPABILITIES,
        ));
    }
    if uses_opencode() {
        rows.push(harness_row(
            "plugin installed",
            check_opencode_plugin,
            "opencode",
            OPENCODE_ACCENT,
            OPENCODE_CAPABILITIES,
        ));
    }
    if uses_pi() {
        rows.push(harness_row(
            "extension installed",
            check_pi_extension,
            "pi",
            PI_ACCENT,
            PI_CAPABILITIES,
        ));
    }
    if uses_crush() {
        rows.push(harness_row(
            "hook registered",
            check_crush_hook,
            "Crush",
            CRUSH_ACCENT,
            CRUSH_CAPABILITIES,
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

    let mut prev_was_harness = false;
    for row in rows {
        let is_harness = matches!(row.kind, RowKind::Harness(_));
        // Exiting the harness section — one blank to separate it from the
        // post-harness rows (nvim alias, sockets, last activity).
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
        let body = theme.dim(&format!("{} (skipped)", row.pending_label));
        return vec![format!("  {marker} {body}")];
    }

    match &row.kind {
        RowKind::Subheader => render_subheader(theme, row),
        RowKind::Harness(meta) => render_harness_row(theme, row, meta, spin),
        RowKind::Normal => render_normal_row(theme, row, spin),
    }
}

fn render_normal_row(theme: &Theme, row: &Row, spin: usize) -> Vec<String> {
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

/// Section title — no marker, no spinner, just the label, padded above and
/// below so the nested harness blocks read as their own section. When the
/// discovery check returns Fail (no AI harnesses detected), we surface that
/// failure inline with a remedy in place of the empty section.
fn render_subheader(theme: &Theme, row: &Row) -> Vec<String> {
    let mut out = vec![String::new()];
    match &row.result {
        None => out.push(format!("  {}", row.pending_label)),
        Some(check) => match &check.status {
            Status::Fail { remedy } => {
                let marker = theme.red("✗");
                out.push(format!("  {marker} {}", check.label));
                for line in remedy {
                    out.push(format!("      {line}"));
                }
            }
            _ => out.push(format!("  {}", row.pending_label)),
        },
    }
    out
}

fn render_harness_row(theme: &Theme, row: &Row, meta: &HarnessMeta, spin: usize) -> Vec<String> {
    // Pending: one line per harness so the report doesn't paint the full
    // nested structure up front. The block expands to 4+ lines only once
    // this row's check resolves.
    let Some(check) = &row.result else {
        let marker = theme.cyan(SPINNER_FRAMES[spin % SPINNER_FRAMES.len()]);
        return vec![format!("    {marker} {}", meta.name)];
    };

    let (bullet, name) = match check.status {
        Status::Pass => (
            theme.wrap(meta.accent, "·"),
            theme.wrap(meta.accent, meta.name),
        ),
        _ => (theme.dim("·"), meta.name.to_string()),
    };
    let mut out = vec![format!("    {bullet} {name}")];

    // Dim support line: `✓ full support` when complete, or `✗ <feature>`
    // for each missing capability when partial.
    out.push(format!(
        "      {}",
        theme.dim(&support_line(&meta.capabilities))
    ));

    let marker = match &check.status {
        Status::Pass => theme.green("✓"),
        Status::Fail { .. } => theme.red("✗"),
        Status::Info => theme.dim("·"),
    };
    out.push(format!("      {marker} {}", check.label));
    if let Some(detail) = &check.detail {
        for line in detail.lines() {
            out.push(format!("          {}", theme.dim(line)));
        }
    }
    if let Status::Fail { remedy } = &check.status {
        for line in remedy {
            out.push(format!("          {line}"));
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

/// Discover which AI harnesses are present. Drives both this row's summary and
/// (via the same `uses_*` checks) which per-harness rows get built at all.
fn check_harnesses() -> Check {
    let mut found: Vec<&str> = Vec::new();
    if uses_claude_code() {
        found.push("Claude Code");
    }
    if uses_codex() {
        found.push("Codex");
    }
    if uses_opencode() {
        found.push("opencode");
    }
    if uses_pi() {
        found.push("pi");
    }
    if uses_crush() {
        found.push("Crush");
    }

    if found.is_empty() {
        Check {
            label: "no AI harness found".into(),
            detail: None,
            status: Status::Fail {
                remedy: vec![
                    "Install Claude Code, Codex, opencode, or pi — sidekick has nothing to guard without one"
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
            label: "hook registered".into(),
            detail: Some(detail),
            status: Status::Pass,
        };
    }

    Check {
        label: "hook not registered".into(),
        detail: None,
        status: Status::Fail {
            remedy: vec![
                "Install the plugin:  /plugin install sidekick@nishant-plugins".into(),
                "Or add `sidekick hook` to ~/.claude/settings.json".into(),
            ],
        },
    }
}

pub(crate) fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn codex_config_file() -> PathBuf {
    codex_home().join("config.toml")
}

pub(crate) fn codex_hook_files() -> Vec<PathBuf> {
    let mut matched = Vec::new();

    for path in [
        codex_config_file(),
        codex_home().join("hooks.json"),
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".codex")
            .join("hooks.json"),
    ] {
        if file_mentions_sidekick_hook(&path) {
            matched.push(path);
        }
    }

    matched.sort();
    matched.dedup();
    matched
}

pub(crate) fn codex_hook_is_configured() -> bool {
    !codex_hook_files().is_empty() || codex_plugin_is_configured()
}

fn codex_plugin_detail_files() -> Vec<PathBuf> {
    let mut matched = Vec::new();

    if let Some(home) = dirs::home_dir() {
        for path in [
            home.join("plugins")
                .join("sidekick")
                .join(".codex-plugin")
                .join("plugin.json"),
            home.join("plugins").join("sidekick").join("hooks.json"),
            home.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
        ] {
            if path.is_file() {
                matched.push(path);
            }
        }
    }

    let config = codex_config_file();
    if config.is_file() {
        matched.push(config);
    }

    matched
}

fn codex_plugin_is_configured() -> bool {
    codex_plugin_enabled()
        && codex_plugin_hooks_enabled()
        && codex_plugin_files_current()
        && codex_plugin_hooks_trusted()
}

/// True only when every hook our plugin registers has a matching
/// `trusted_hash` under `[hooks.state.*]`. Without these, Codex silently
/// drops the handlers at discovery time.
fn codex_plugin_hooks_trusted() -> bool {
    let Ok(config) = std::fs::read_to_string(codex_config_file()) else {
        return false;
    };
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    let Some(state) = doc.get("hooks").and_then(|hooks| hooks.get("state")) else {
        return false;
    };
    crate::codex_trust::expected_trust_entries()
        .iter()
        .all(|entry| {
            state
                .get(&entry.key)
                .and_then(|item| item.get("trusted_hash"))
                .and_then(|v| v.as_str())
                == Some(entry.trusted_hash.as_str())
        })
}

fn codex_plugin_enabled() -> bool {
    let Ok(config) = std::fs::read_to_string(codex_config_file()) else {
        return false;
    };
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("plugins")
        .and_then(|p| p.get("sidekick@personal"))
        .and_then(|p| p.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn codex_plugin_hooks_enabled() -> bool {
    let Ok(config) = std::fs::read_to_string(codex_config_file()) else {
        return false;
    };
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("features")
        .and_then(|p| p.get("plugin_hooks"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn codex_plugin_files_current() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let source_root = home.join("plugins").join("sidekick");
    let cache_root = codex_home()
        .join("plugins")
        .join("cache")
        .join("personal")
        .join("sidekick")
        .join(env!("CARGO_PKG_VERSION"));

    codex_plugin_root_current(&source_root) && codex_plugin_root_current(&cache_root)
}

fn codex_plugin_root_current(root: &Path) -> bool {
    std::fs::read_to_string(root.join(".codex-plugin").join("plugin.json"))
        .ok()
        .as_deref()
        == Some(crate::fix::CODEX_PLUGIN_SRC)
        && std::fs::read_to_string(root.join("hooks.json"))
            .ok()
            .as_deref()
            == Some(crate::fix::CODEX_HOOKS_SRC)
}

fn check_codex_plugin() -> Check {
    let direct_hooks = codex_hook_files();
    if !direct_hooks.is_empty() {
        return Check {
            label: "hook registered".into(),
            detail: Some(
                direct_hooks
                    .iter()
                    .map(|p| display_path(p))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            status: Status::Pass,
        };
    }

    if codex_plugin_is_configured() {
        return Check {
            label: "plugin installed".into(),
            detail: Some(
                codex_plugin_detail_files()
                    .iter()
                    .map(|p| display_path(p))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            status: Status::Pass,
        };
    }

    let mut remedy = Vec::new();
    if !codex_plugin_hooks_enabled() {
        remedy.push("Enable Codex plugin hooks in ~/.codex/config.toml".into());
    }
    if !codex_plugin_enabled() {
        remedy.push("Enable the Sidekick Codex plugin as `sidekick@personal`".into());
    }
    if !codex_plugin_files_current() {
        remedy.push("Install the Sidekick Codex plugin into your personal marketplace".into());
    }

    Check {
        label: "plugin not installed".into(),
        detail: None,
        status: Status::Fail { remedy },
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
            label: "plugin not installed".into(),
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
            label: "plugin installed".into(),
            detail: Some(detail),
            status: Status::Pass,
        }
    } else {
        Check {
            label: "plugin out of sync".into(),
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
            label: "extension not installed".into(),
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
            label: "extension installed".into(),
            detail: Some(detail),
            status: Status::Pass,
        }
    } else {
        Check {
            label: "extension out of sync".into(),
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

/// Whether this machine looks like a Codex user.
pub(crate) fn uses_codex() -> bool {
    binary_on_path("codex") || codex_home().is_dir()
}

#[derive(PartialEq)]
pub(crate) enum AliasStatus {
    Active,
    Missing,
    Unknown,
}

/// Echoed immediately before `type nvim` so we can discard whatever the
/// interactive rc files (.zshrc banners, MOTDs) wrote to stdout on startup —
/// otherwise that noise gets mistaken for the probe's result.
const NVIM_PROBE_SENTINEL: &str = "__sidekick_probe__";

/// Runs `type nvim` in `shell` as an interactive shell (`-i`, so rc-file
/// aliases resolve) and returns just that command's output, with any startup
/// banner printed before [`NVIM_PROBE_SENTINEL`] stripped off.
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

/// Runtime check of whether `nvim` resolves to `sidekick neovim` in the login
/// shell — the same probe `check_shell_alias` renders, shared so `--fix` never
/// offers an alias that's already live.
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

/// Whether this machine looks like a Crush user.
pub(crate) fn uses_crush() -> bool {
    binary_on_path("crush")
        || dirs::home_dir()
            .map(|h| h.join(".config").join("crush").is_dir())
            .unwrap_or(false)
}

/// Candidate Crush config files in precedence order: project local first,
/// then global (`$CRUSH_GLOBAL_CONFIG` overrides the default global path).
pub(crate) fn crush_config_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".crush.json"));
        candidates.push(cwd.join("crush.json"));
    }

    if let Some(path) = std::env::var_os("CRUSH_GLOBAL_CONFIG") {
        candidates.push(PathBuf::from(path));
    } else if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("crush").join("crush.json"));
    }

    candidates
}

/// Config files that already wire up `sidekick hook` for Crush.
pub(crate) fn crush_hook_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = crush_config_candidates()
        .into_iter()
        .filter(|p| file_mentions_sidekick_hook(p))
        .collect();
    matched.sort();
    matched.dedup();
    matched
}

fn check_crush_hook() -> Check {
    let matched = crush_hook_files();

    if !matched.is_empty() {
        let detail = matched
            .iter()
            .map(|p| display_path(p))
            .collect::<Vec<_>>()
            .join("\n");
        return Check {
            label: "hook registered".into(),
            detail: Some(detail),
            status: Status::Pass,
        };
    }

    Check {
        label: "hook not registered".into(),
        detail: None,
        status: Status::Fail {
            remedy: vec![
                "Run `sidekick doctor --fix` to merge the hook into ~/.config/crush/crush.json"
                    .into(),
            ],
        },
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

    // `probe_nvim_type` sources the user's rc files (so aliases resolve) and
    // strips any startup banner the rc files print, leaving just `type nvim`.
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
