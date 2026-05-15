//! `sidekick init` — guided first-run setup.
//!
//! Where `doctor --fix` is a terse repair path, `init` is an onboarding one: a
//! calm checklist that walks every integration top-to-bottom, ticks off what's
//! already wired up, and pauses on what isn't. Skipping a step is a fine,
//! expected outcome — never an error. The writes themselves are `fix.rs`'s, so
//! `init` and `--fix` can't drift apart.
//!
//! On a terminal the checklist animates and prompts inline. When stdout is not
//! a terminal we print the plan and apply nothing — consent can't be given
//! non-interactively.

use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use crate::doctor::{
    self, AliasStatus, CLAUDE_ACCENT, OPENCODE_ACCENT, PI_ACCENT, SPINNER_FRAMES, Theme,
    display_path,
};
use crate::fix::{self, Fix};

/// Spinner frames shown per step before it resolves.
const SPIN_FRAMES: u32 = 3;
const FRAME_DELAY: Duration = Duration::from_millis(70);

/// Whether a step is already satisfied or still needs a write.
enum StepState {
    Done,
    Pending(Fix),
}

/// How a step ended, once resolved.
enum Outcome {
    AlreadyDone,
    SetUp,
    Skipped,
    Failed(String),
    /// Left untouched because the user quit before reaching it.
    Untouched,
}

struct Step {
    label: &'static str,
    description: &'static str,
    /// Truecolor ANSI params for the label, applied once the step is green.
    accent: Option<&'static str>,
    state: StepState,
    outcome: Option<Outcome>,
}

fn make_step(
    label: &'static str,
    description: &'static str,
    accent: Option<&'static str>,
    fix: Option<Fix>,
) -> Step {
    Step {
        label,
        description,
        accent,
        state: match fix {
            Some(f) => StepState::Pending(f),
            None => StepState::Done,
        },
        outcome: None,
    }
}

/// One step per detected harness, plus the `nvim` alias. An absent harness
/// gets no row; past the `uses_*` gate, a `None` fix means already configured.
fn build_steps() -> Vec<Step> {
    let mut steps = Vec::new();

    if doctor::uses_claude_code() {
        steps.push(make_step(
            "Claude Code hook",
            "Lets Claude Code check your unsaved Neovim buffers before it edits.",
            Some(CLAUDE_ACCENT),
            fix::claude_fix(),
        ));
    }
    if doctor::uses_opencode() {
        steps.push(make_step(
            "opencode plugin",
            "Lets opencode respect your unsaved Neovim buffers before it writes.",
            Some(OPENCODE_ACCENT),
            fix::opencode_fix(),
        ));
    }
    if doctor::uses_pi() {
        steps.push(make_step(
            "pi extension",
            "Lets the pi agent respect your unsaved Neovim buffers before it writes.",
            Some(PI_ACCENT),
            fix::pi_fix(),
        ));
    }

    const ALIAS_DESC: &str = "Routes `nvim` through sidekick so every session is guarded.";
    match doctor::nvim_alias_status() {
        AliasStatus::Active => steps.push(make_step("nvim alias", ALIAS_DESC, None, None)),
        AliasStatus::Missing => {
            steps.push(make_step("nvim alias", ALIAS_DESC, None, fix::alias_fix()));
        }
        // Couldn't probe the shell — say nothing rather than guess.
        AliasStatus::Unknown => {}
    }

    steps
}

pub fn run(no_color: bool) -> anyhow::Result<()> {
    let theme = Theme::new(!no_color);
    let mut steps = build_steps();

    if steps.is_empty() {
        let mut out = io::stdout();
        writeln!(
            out,
            "\n  {}\n",
            theme.dim("No AI harness found — install Claude Code, opencode, or pi first."),
        )?;
        return Ok(());
    }

    if !io::stdout().is_terminal() {
        print_plan(&theme, &steps)?;
        return Ok(());
    }

    let mut out = io::stdout();
    // Hide the cursor while we redraw; restore it on every path out.
    write!(out, "\x1b[?25l")?;
    let result = drive(&mut out, &theme, &mut steps);
    write!(out, "\x1b[?25h")?;
    out.flush()?;
    result?;

    print_summary(&theme, &steps)?;
    Ok(())
}

/// Walk the checklist top-to-bottom, resolving each step in place.
fn drive(out: &mut impl Write, theme: &Theme, steps: &mut [Step]) -> io::Result<()> {
    let mut height = 0usize;
    let mut spin = 0usize;
    let mut quit = false;

    draw(out, theme, steps, None, false, false, spin, &mut height)?;

    for i in 0..steps.len() {
        if quit {
            steps[i].outcome = Some(Outcome::Untouched);
            continue;
        }

        for _ in 0..SPIN_FRAMES {
            spin += 1;
            thread::sleep(FRAME_DELAY);
            draw(out, theme, steps, Some(i), false, false, spin, &mut height)?;
        }

        if matches!(steps[i].state, StepState::Done) {
            steps[i].outcome = Some(Outcome::AlreadyDone);
        } else {
            let mut show_diff = false;
            draw(out, theme, steps, Some(i), true, show_diff, spin, &mut height)?;
            loop {
                let (answer, prompt_lines) = ask(theme)?;
                // The typed answer + Enter echo extra lines; fold them into the
                // tracked height so the next redraw clears them too.
                height += prompt_lines - 1;
                match answer {
                    Answer::Diff => {
                        show_diff = true;
                        draw(out, theme, steps, Some(i), true, show_diff, spin, &mut height)?;
                    }
                    Answer::Yes => {
                        let outcome = match &steps[i].state {
                            StepState::Pending(fix) => match fix.apply() {
                                Ok(()) => Outcome::SetUp,
                                Err(e) => Outcome::Failed(e.to_string()),
                            },
                            StepState::Done => unreachable!("Done steps never prompt"),
                        };
                        steps[i].outcome = Some(outcome);
                        break;
                    }
                    Answer::No => {
                        steps[i].outcome = Some(Outcome::Skipped);
                        break;
                    }
                    Answer::Quit => {
                        steps[i].outcome = Some(Outcome::Untouched);
                        quit = true;
                        break;
                    }
                }
            }
        }

        draw(out, theme, steps, None, false, false, spin, &mut height)?;
    }
    Ok(())
}

/// Redraw the whole checklist: jump to its top, clear, and reprint. When
/// `expanded`, the active step shows its detail and the trailing prompt line
/// is left without a newline so input lands right after the `›`.
#[allow(clippy::too_many_arguments)]
fn draw(
    out: &mut impl Write,
    theme: &Theme,
    steps: &[Step],
    active: Option<usize>,
    expanded: bool,
    show_diff: bool,
    spin: usize,
    last_height: &mut usize,
) -> io::Result<()> {
    if *last_height > 0 {
        write!(out, "\x1b[{}A\r", *last_height)?;
    }
    write!(out, "\x1b[J")?;

    let body = render_block(theme, steps, active, expanded, show_diff, spin);
    let mut height = 0;
    for line in &body {
        writeln!(out, "{line}")?;
        height += 1;
    }
    if expanded {
        write!(out, "{}", prompt_line(theme))?;
        height += 1;
    }
    *last_height = height;
    out.flush()
}

fn render_block(
    theme: &Theme,
    steps: &[Step],
    active: Option<usize>,
    expanded: bool,
    show_diff: bool,
    spin: usize,
) -> Vec<String> {
    let mut out = vec![
        String::new(),
        format!("  {}", theme.bold("sidekick · init")),
        String::new(),
    ];

    for (idx, step) in steps.iter().enumerate() {
        if let Some(outcome) = &step.outcome {
            out.push(render_resolved(theme, step, outcome));
        } else if active == Some(idx) {
            let spinner = theme.cyan(SPINNER_FRAMES[spin % SPINNER_FRAMES.len()]);
            out.push(format!("  {spinner} {}", step.label));
            if expanded {
                out.push(format!("      {}", theme.dim(step.description)));
                if let StepState::Pending(fix) = &step.state {
                    out.push(format!(
                        "      {}   {}",
                        theme.dim(fix.verb()),
                        theme.dim(&display_path(&fix.path)),
                    ));
                    if show_diff {
                        out.extend(fix::render_diff_lines(theme, fix));
                    }
                }
            }
        } else {
            out.push(format!("  {} {}", theme.dim("○"), theme.dim(step.label)));
        }
    }

    out.push(String::new());
    out
}

fn render_resolved(theme: &Theme, step: &Step, outcome: &Outcome) -> String {
    let label = match step.accent {
        Some(code) => theme.wrap(code, step.label),
        None => step.label.to_string(),
    };
    match outcome {
        Outcome::AlreadyDone => format!(
            "  {} {}   {}",
            theme.green("✓"),
            label,
            theme.dim("already set"),
        ),
        Outcome::SetUp => format!("  {} {}   {}", theme.green("✓"), label, theme.dim("set up")),
        Outcome::Skipped => format!(
            "  {} {}   {}",
            theme.dim("·"),
            theme.dim(step.label),
            theme.dim("skipped"),
        ),
        Outcome::Failed(e) => format!(
            "  {} {}   {}",
            theme.red("✗"),
            step.label,
            theme.dim(&format!("— {e}")),
        ),
        Outcome::Untouched => format!("  {} {}", theme.dim("○"), theme.dim(step.label)),
    }
}

fn prompt_line(theme: &Theme) -> String {
    format!(
        "  {}   {}   {} ",
        theme.bold("Set up?"),
        theme.dim("[y] yes   [n] skip   [d] diff   [q] quit"),
        theme.cyan("›"),
    )
}

enum Answer {
    Yes,
    No,
    Diff,
    Quit,
}

/// Read a y/n/d/q answer. Returns how many terminal lines the prompt occupied
/// (one per attempt) so the caller can clear the echoed input on redraw.
fn ask(theme: &Theme) -> io::Result<(Answer, usize)> {
    let mut prompt_lines = 1usize;
    loop {
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok((Answer::Quit, prompt_lines));
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok((Answer::Yes, prompt_lines)),
            "" | "n" | "no" | "skip" => return Ok((Answer::No, prompt_lines)),
            "d" | "diff" => return Ok((Answer::Diff, prompt_lines)),
            "q" | "quit" => return Ok((Answer::Quit, prompt_lines)),
            _ => {
                print!("  {} ", theme.dim("answer y, n, d, or q  ›"));
                io::stdout().flush()?;
                prompt_lines += 1;
            }
        }
    }
}

/// Tally the run and point at `doctor` when anything changed. Skips are a plain
/// fact here, not a problem to chase.
fn print_summary(theme: &Theme, steps: &[Step]) -> io::Result<()> {
    let mut out = io::stdout();
    let (mut set_up, mut already, mut skipped, mut failed) = (0, 0, 0, 0);
    for step in steps {
        match &step.outcome {
            Some(Outcome::SetUp) => set_up += 1,
            Some(Outcome::AlreadyDone) => already += 1,
            Some(Outcome::Skipped) => skipped += 1,
            Some(Outcome::Failed(_)) => failed += 1,
            _ => {}
        }
    }

    let mut parts = Vec::new();
    if set_up > 0 {
        parts.push(format!("{set_up} set up"));
    }
    if already > 0 {
        parts.push(format!("{already} already configured"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if parts.is_empty() {
        parts.push("nothing to do".to_string());
    }

    writeln!(out, "  {}", theme.dim(&parts.join(" · ")))?;
    if set_up > 0 || failed > 0 {
        writeln!(out, "  {}", theme.dim("Run `sidekick doctor` to confirm."))?;
    }
    writeln!(out)?;
    Ok(())
}

/// Non-interactive fallback: list the pending steps, apply nothing.
fn print_plan(theme: &Theme, steps: &[Step]) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "\n  {}\n", theme.bold("sidekick · init"))?;

    let pending: Vec<&Step> = steps
        .iter()
        .filter(|s| matches!(s.state, StepState::Pending(_)))
        .collect();

    if pending.is_empty() {
        writeln!(out, "  {}\n", theme.dim("Everything's already wired up."))?;
        return Ok(());
    }

    writeln!(out, "  {}", theme.dim("Run this in a terminal to set up:"))?;
    for step in pending {
        if let StepState::Pending(fix) = &step.state {
            writeln!(
                out,
                "    {} {}  {}",
                theme.dim("·"),
                step.label,
                theme.dim(&display_path(&fix.path)),
            )?;
        }
    }
    writeln!(out)?;
    Ok(())
}
