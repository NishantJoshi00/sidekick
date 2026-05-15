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

/// The opencode plugin, baked in so `--fix` needs no repo checkout or network.
/// Also the reference the doctor compares an installed plugin against.
pub(crate) const OPENCODE_PLUGIN_SRC: &str = include_str!("../plugins/opencode/sidekick.ts");

/// The pi extension, baked in so `--fix` needs no repo checkout or network.
/// Also the reference the doctor compares an installed extension against.
pub(crate) const PI_EXTENSION_SRC: &str = include_str!("../plugins/pi/sidekick.ts");

/// A single repair: the file at `path` goes from `before` to `after`.
///
/// Shared with `sidekick init` — both commands write config the same way,
/// they only present it differently.
pub(crate) struct Fix {
    pub(crate) title: String,
    pub(crate) path: PathBuf,
    /// `None` when the file does not exist yet.
    pub(crate) before: Option<String>,
    pub(crate) after: String,
}

impl Fix {
    pub(crate) fn verb(&self) -> &'static str {
        if self.before.is_some() {
            "update"
        } else {
            "create"
        }
    }

    pub(crate) fn apply(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("couldn't create {}", parent.display()))?;
        }
        std::fs::write(&self.path, &self.after)
            .with_context(|| format!("couldn't write {}", self.path.display()))
    }
}

/// Build the fix list — one entry per repairable doctor finding, no overlap.
fn collect() -> Vec<Fix> {
    [claude_fix(), opencode_fix(), pi_fix(), alias_fix()]
        .into_iter()
        .flatten()
        .collect()
}

pub(crate) fn opencode_fix() -> Option<Fix> {
    if !doctor::uses_opencode() {
        return None;
    }
    // Update a plugin that's already there (stale install), else create one
    // at the canonical path.
    let canonical = dirs::home_dir()?
        .join(".config")
        .join("opencode")
        .join("plugin")
        .join("sidekick.ts");
    let path = doctor::opencode_plugin_files()
        .into_iter()
        .next()
        .unwrap_or(canonical);
    let before = std::fs::read_to_string(&path).ok();
    if before.as_deref() == Some(OPENCODE_PLUGIN_SRC) {
        return None;
    }
    let title = if before.is_some() {
        "Update the opencode plugin"
    } else {
        "Install the opencode plugin"
    };
    Some(Fix {
        title: title.into(),
        path,
        before,
        after: OPENCODE_PLUGIN_SRC.to_string(),
    })
}

pub(crate) fn pi_fix() -> Option<Fix> {
    if !doctor::uses_pi() {
        return None;
    }
    // Update an extension that's already there (stale install), else create
    // one at the canonical path.
    let canonical = dirs::home_dir()?
        .join(".pi")
        .join("agent")
        .join("extensions")
        .join("sidekick.ts");
    let path = doctor::pi_extension_files()
        .into_iter()
        .next()
        .unwrap_or(canonical);
    let before = std::fs::read_to_string(&path).ok();
    if before.as_deref() == Some(PI_EXTENSION_SRC) {
        return None;
    }
    let title = if before.is_some() {
        "Update the pi extension"
    } else {
        "Install the pi extension"
    };
    Some(Fix {
        title: title.into(),
        path,
        before,
        after: PI_EXTENSION_SRC.to_string(),
    })
}

pub(crate) fn claude_fix() -> Option<Fix> {
    if !doctor::uses_claude_code() || !doctor::claude_hook_files().is_empty() {
        return None;
    }
    let path = dirs::home_dir()?.join(".claude").join("settings.json");
    let before = std::fs::read_to_string(&path).ok();
    let after = claude_settings_after(before.as_deref()).ok()?;
    Some(Fix {
        title: "Register the Claude Code hooks".into(),
        path,
        before,
        after,
    })
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
    Some(Fix {
        title: "Add the nvim → sidekick alias".into(),
        path,
        before,
        after,
    })
}

/// Merge sidekick's three hooks into a Claude Code `settings.json`, leaving
/// every other key — and the user's key order — untouched.
fn claude_settings_after(before: Option<&str>) -> Result<String> {
    let mut root: serde_json::Value = match before {
        Some(s) if !s.trim().is_empty() => {
            serde_json::from_str(s).context("~/.claude/settings.json isn't valid JSON")?
        }
        _ => serde_json::json!({}),
    };
    {
        let obj = root
            .as_object_mut()
            .context("~/.claude/settings.json isn't a JSON object")?;
        let hooks = obj
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("`hooks` in settings.json isn't an object")?;
        for (event, matcher) in [
            ("PreToolUse", "Edit|Write|MultiEdit"),
            ("PostToolUse", "Edit|Write|MultiEdit"),
            ("UserPromptSubmit", ""),
        ] {
            let arr = hooks
                .entry(event)
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .with_context(|| format!("`hooks.{event}` in settings.json isn't an array"))?;
            arr.push(serde_json::json!({
                "matcher": matcher,
                "hooks": [{ "type": "command", "command": "sidekick hook" }],
            }));
        }
    }
    let mut s = serde_json::to_string_pretty(&root)?;
    s.push('\n');
    Ok(s)
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
            theme.dim(&display_path(&fix.path)),
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
    out.push(format!(
        "    {}   {}",
        theme.dim(fix.verb()),
        theme.dim(&display_path(&fix.path)),
    ));
    out.push(String::new());

    let rows = truncate_diff(diff_rows(fix.before.as_deref().unwrap_or(""), &fix.after));
    for row in &rows {
        out.push(render_diff_row(theme, row));
    }
    out.push(String::new());
    out
}

/// The diff body for a fix — context and add/remove rows, big diffs truncated.
/// Shared with `sidekick init`, which shows it on demand behind `[d]`.
pub(crate) fn render_diff_lines(theme: &Theme, fix: &Fix) -> Vec<String> {
    truncate_diff(diff_rows(fix.before.as_deref().unwrap_or(""), &fix.after))
        .iter()
        .map(|row| render_diff_row(theme, row))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::claude_settings_after;

    #[test]
    fn merges_three_hooks_into_empty_settings() {
        let out = claude_settings_after(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let hooks = &v["hooks"];
        for event in ["PreToolUse", "PostToolUse", "UserPromptSubmit"] {
            let arr = hooks[event].as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0]["hooks"][0]["command"], "sidekick hook");
        }
        assert_eq!(hooks["PreToolUse"][0]["matcher"], "Edit|Write|MultiEdit");
        assert_eq!(hooks["UserPromptSubmit"][0]["matcher"], "");
    }

    #[test]
    fn keeps_existing_keys_order_and_hooks() {
        let before = r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[]}]}}"#;
        let out = claude_settings_after(Some(before)).unwrap();
        // preserve_order keeps `model` ahead of `hooks` rather than sorting.
        assert!(out.find("\"model\"").unwrap() < out.find("\"hooks\"").unwrap());

        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "opus");
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["matcher"], "Bash");
        assert_eq!(pre[1]["hooks"][0]["command"], "sidekick hook");
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(claude_settings_after(Some("{ not json")).is_err());
    }
}
