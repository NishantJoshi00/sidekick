//! `sidekick doctor --fix` — consent-gated repair of doctor findings.
//!
//! Every fix is shown as a diff before anything is written; nothing touches
//! disk until the user presses `y`. Once answered, the card collapses to a
//! single-line result and the next one opens. When stdout is not a terminal
//! the plan is printed and nothing is applied — consent can't be given
//! non-interactively.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use similar::{ChangeTag, TextDiff};

use crate::doctor::{self, Theme, display_path};
use crate::harness;

/// One file edit inside a repair.
pub(crate) struct FileFix {
    pub(crate) path: PathBuf,
    /// `None` when the file does not exist yet.
    pub(crate) before: Option<String>,
    pub(crate) after: String,
}

/// A repair, which may touch one file or a small group of related files.
///
/// Shared with `sidekick init` — both commands write config the same way,
/// they only present it differently.
pub(crate) struct Fix {
    pub(crate) title: String,
    pub(crate) files: Vec<FileFix>,
}

impl Fix {
    pub(crate) fn single(
        title: impl Into<String>,
        path: PathBuf,
        before: Option<String>,
        after: String,
    ) -> Self {
        Self {
            title: title.into(),
            files: vec![FileFix {
                path,
                before,
                after,
            }],
        }
    }

    pub(crate) fn verb(&self) -> &'static str {
        let mut has_create = false;
        let mut has_update = false;
        for file in &self.files {
            if file.before.is_some() {
                has_update = true;
            } else {
                has_create = true;
            }
        }
        match (has_create, has_update) {
            (true, false) => "create",
            (false, true) => "update",
            _ => "change",
        }
    }

    pub(crate) fn target_summary(&self) -> String {
        if self.files.len() == 1 {
            display_path(&self.files[0].path)
        } else {
            format!("{} files", self.files.len())
        }
    }

    pub(crate) fn apply(&self) -> Result<()> {
        for file in &self.files {
            if let Some(parent) = file.path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("couldn't create {}", parent.display()))?;
            }
            std::fs::write(&file.path, &file.after)
                .with_context(|| format!("couldn't write {}", file.path.display()))?;
        }
        Ok(())
    }
}

/// Build the fix list — one entry per repairable doctor finding, no
/// overlap. Iterates the harness registry; per-harness fix logic lives in
/// each `Harness::fix` impl, not here.
fn collect() -> Vec<Fix> {
    let mut fixes = Vec::new();
    for h in harness::registry::all() {
        fixes.extend(h.fix());
    }
    fixes.extend(alias_fix());
    fixes
}

pub(crate) fn alias_fix() -> Option<Fix> {
    const ALIAS: &str = "alias nvim='sidekick neovim'";
    // Use the doctor's runtime verdict so we never re-offer a live alias,
    // even when it lives in a file other than the one we'd append to.
    if doctor::nvim_alias_status() != doctor::AliasStatus::Missing {
        return None;
    }
    let path = shell_rc_path()?;
    let before = std::fs::read_to_string(&path).ok();
    if before.as_deref().is_some_and(|c| c.contains(ALIAS)) {
        return None;
    }
    let mut after = before.clone().unwrap_or_default();
    if !after.is_empty() && !after.ends_with('\n') {
        after.push('\n');
    }
    after.push_str("\n# sidekick\n");
    after.push_str(ALIAS);
    after.push('\n');
    Some(Fix::single(
        "Add the nvim → sidekick alias",
        path,
        before,
        after,
    ))
}

/// The rc file the user's login shell sources, mirroring `scripts/install.sh`.
fn shell_rc_path() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").ok()?;
    let name = Path::new(&shell).file_name()?.to_str()?;
    let home = dirs::home_dir()?;
    Some(match name {
        "zsh" => home.join(".zshrc"),
        "bash" => {
            let profile = home.join(".bash_profile");
            if cfg!(target_os = "macos") && profile.exists() {
                profile
            } else {
                home.join(".bashrc")
            }
        }
        "fish" => home.join(".config").join("fish").join("config.fish"),
        _ => home.join(".profile"),
    })
}

pub fn run(no_color: bool, any_failed: bool) -> Result<()> {
    let theme = Theme::new(!no_color);
    let fixes = collect();
    let mut out = io::stdout();

    if fixes.is_empty() {
        let msg = if any_failed {
            "Nothing here can be fixed automatically — see the report above."
        } else {
            "Nothing to fix — sidekick is fully wired up."
        };
        writeln!(out, "\n  {}\n", theme.dim(msg))?;
        return Ok(());
    }

    if !io::stdout().is_terminal() {
        return print_plan(&theme, &fixes);
    }

    writeln!(out, "\n  {}", theme.bold("sidekick · fix"))?;

    let total = fixes.len();
    let mut applied = 0usize;
    let mut skipped = 0usize;
    let mut reviewed = 0usize;

    for (i, fix) in fixes.iter().enumerate() {
        let card = card_lines(&theme, fix, i + 1, total);
        for line in &card {
            writeln!(out, "{line}")?;
        }
        write!(
            out,
            "  {}   {}   {} ",
            theme.bold("Apply?"),
            theme.dim("[y] yes    [n] skip    [q] quit"),
            theme.cyan("›"),
        )?;
        out.flush()?;

        let (answer, prompt_lines) = ask(&theme)?;
        collapse(&mut out, card.len() + prompt_lines)?;
        reviewed += 1;

        let resolved = match answer {
            Answer::Yes => match fix.apply() {
                Ok(()) => {
                    applied += 1;
                    format!("  {} {}", theme.green("✓"), fix.title)
                }
                Err(e) => {
                    reviewed -= 1;
                    format!(
                        "  {} {} {}",
                        theme.red("✗"),
                        fix.title,
                        theme.dim(&format!("— {e}")),
                    )
                }
            },
            Answer::No => {
                skipped += 1;
                format!("  {} {}", theme.dim("·"), theme.dim(&fix.title))
            }
            Answer::Quit => {
                reviewed -= 1;
                writeln!(out, "  {} {}", theme.dim("⊝"), theme.dim(&fix.title))?;
                break;
            }
        };
        writeln!(out, "{resolved}")?;
    }

    write!(out, "\n  ")?;
    let mut parts = Vec::new();
    if applied > 0 {
        parts.push(format!("{applied} applied"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if reviewed < total {
        parts.push(format!("{} left", total - reviewed));
    }
    writeln!(out, "{}", theme.dim(&parts.join(" · ")))?;
    if applied > 0 {
        writeln!(out, "  {}", theme.dim("Run `sidekick doctor` to confirm."))?;
    }
    writeln!(out)?;
    Ok(())
}

/// Non-interactive fallback: describe the fixes, apply nothing.
fn print_plan(theme: &Theme, fixes: &[Fix]) -> Result<()> {
    let mut out = io::stdout();
    writeln!(out, "\n  {}\n", theme.bold("sidekick · fix"))?;
    writeln!(
        out,
        "  {}",
        theme.dim("Run this in a terminal to review and apply:"),
    )?;
    for fix in fixes {
        writeln!(
            out,
            "    {} {}  {}",
            theme.dim("·"),
            fix.title,
            theme.dim(&fix.target_summary()),
        )?;
    }
    writeln!(out)?;
    Ok(())
}

enum Answer {
    Yes,
    No,
    Quit,
}

/// Read a y/n/q answer. Returns how many terminal lines the prompt occupied
/// (one per attempt) so the caller can erase the exact region on collapse.
fn ask(theme: &Theme) -> io::Result<(Answer, usize)> {
    let mut prompt_lines = 1usize;
    loop {
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok((Answer::Quit, prompt_lines));
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok((Answer::Yes, prompt_lines)),
            "" | "n" | "no" => return Ok((Answer::No, prompt_lines)),
            "q" | "quit" => return Ok((Answer::Quit, prompt_lines)),
            _ => {
                print!("  {} ", theme.dim("answer y, n, or q  ›"));
                io::stdout().flush()?;
                prompt_lines += 1;
            }
        }
    }
}

/// Move the cursor to the top of the just-drawn region and clear it, so the
/// caller can replace a whole consent card with a one-line result.
fn collapse(out: &mut impl Write, lines: usize) -> io::Result<()> {
    if lines > 0 {
        write!(out, "\x1b[{lines}A")?;
    }
    write!(out, "\r\x1b[J")
}

fn card_lines(theme: &Theme, fix: &Fix, idx: usize, total: usize) -> Vec<String> {
    let mut out = Vec::new();
    out.push(String::new());

    let head = format!("──  fix {idx} of {total}  ");
    let pad = 60usize.saturating_sub(head.chars().count());
    out.push(format!(
        "  {}",
        theme.dim(&format!("{head}{}", "─".repeat(pad)))
    ));
    out.push(String::new());

    out.push(format!("  {}", theme.bold(&fix.title)));
    if fix.files.len() == 1 {
        out.push(format!(
            "    {}   {}",
            theme.dim(fix.verb()),
            theme.dim(&display_path(&fix.files[0].path)),
        ));
    } else {
        out.push(format!(
            "    {}   {}",
            theme.dim(fix.verb()),
            theme.dim(&fix.target_summary()),
        ));
        for file in &fix.files {
            let verb = if file.before.is_some() {
                "update"
            } else {
                "create"
            };
            out.push(format!(
                "      {}   {}",
                theme.dim(verb),
                theme.dim(&display_path(&file.path)),
            ));
        }
    }
    out.push(String::new());

    out.extend(render_diff_lines(theme, fix));
    out.push(String::new());
    out
}

/// The diff body for a fix — context and add/remove rows, big diffs truncated.
/// Shared with `sidekick init`, which shows it on demand behind `[d]`.
pub(crate) fn render_diff_lines(theme: &Theme, fix: &Fix) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, file) in fix.files.iter().enumerate() {
        if fix.files.len() > 1 {
            if idx > 0 {
                out.push(format!("    {} {}", theme.dim("│"), theme.dim("⋮")));
            }
            out.push(format!(
                "    {} {}",
                theme.dim("│"),
                theme.dim(&display_path(&file.path))
            ));
        }
        out.extend(
            truncate_diff(diff_rows(file.before.as_deref().unwrap_or(""), &file.after))
                .iter()
                .map(|row| render_diff_row(theme, row)),
        );
    }
    out
}

enum DiffMark {
    Add,
    Del,
    Ctx,
    Gap,
}

struct DiffRow {
    mark: DiffMark,
    text: String,
}

/// Unified diff with three lines of context around each change.
fn diff_rows(before: &str, after: &str) -> Vec<DiffRow> {
    let diff = TextDiff::from_lines(before, after);
    let mut rows = Vec::new();
    for (group_idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if group_idx > 0 {
            rows.push(DiffRow {
                mark: DiffMark::Gap,
                text: String::new(),
            });
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let mark = match change.tag() {
                    ChangeTag::Insert => DiffMark::Add,
                    ChangeTag::Delete => DiffMark::Del,
                    ChangeTag::Equal => DiffMark::Ctx,
                };
                rows.push(DiffRow {
                    mark,
                    text: change.value().trim_end_matches(['\r', '\n']).to_string(),
                });
            }
        }
    }
    rows
}

/// Keep big diffs (a freshly created plugin file) readable: head, tail, and a
/// `⋮ N more lines` marker for everything in between.
fn truncate_diff(mut rows: Vec<DiffRow>) -> Vec<DiffRow> {
    const HEAD: usize = 16;
    const TAIL: usize = 3;
    if rows.len() <= HEAD + TAIL + 1 {
        return rows;
    }
    let hidden = rows.len() - HEAD - TAIL;
    let tail = rows.split_off(rows.len() - TAIL);
    rows.truncate(HEAD);
    rows.push(DiffRow {
        mark: DiffMark::Gap,
        text: format!("{hidden} more lines"),
    });
    rows.extend(tail);
    rows
}

fn render_diff_row(theme: &Theme, row: &DiffRow) -> String {
    let gutter = theme.dim("│");
    match row.mark {
        DiffMark::Add => format!(
            "    {gutter} {}",
            theme.green(&format!("+ {}", truncate_text(&row.text))),
        ),
        DiffMark::Del => format!(
            "    {gutter} {}",
            theme.red(&format!("- {}", truncate_text(&row.text))),
        ),
        DiffMark::Ctx => format!(
            "    {gutter} {}",
            theme.dim(&format!("  {}", truncate_text(&row.text))),
        ),
        DiffMark::Gap => {
            let body = if row.text.is_empty() {
                "⋮".to_string()
            } else {
                format!("⋮  {}", row.text)
            };
            format!("    {gutter} {}", theme.dim(&body))
        }
    }
}

fn truncate_text(s: &str) -> String {
    const MAX: usize = 84;
    if s.chars().count() > MAX {
        let kept: String = s.chars().take(MAX - 1).collect();
        format!("{kept}…")
    } else {
        s.to_string()
    }
}
