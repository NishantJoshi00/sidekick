//! Terminal renderer — "The Mirror".
//!
//! The screen is a duet (or duel, depending on the week) between you and
//! the AI. Two sparklines straddle a row of save-dots. Where the lines spike
//! together, the dots cluster — those are the moments sidekick caught.
//!
//! Below the picture, two rotating prose lines and sometimes a third meta
//! observation. The picture is constant in shape; the language wears a
//! different sentence every visit (variable reward through framing).

use std::io::Write;

use crate::analytics::aggregate::Stats;
use crate::analytics::render::Renderer;

pub struct TerminalRenderer {
    pub color: bool,
}

impl Default for TerminalRenderer {
    fn default() -> Self {
        Self { color: true }
    }
}

impl Renderer for TerminalRenderer {
    fn render(&self, stats: &Stats, out: &mut dyn Write) -> anyhow::Result<()> {
        let p = Paint::new(self.color);

        if stats.you_buckets.iter().all(|&v| v == 0) && stats.ai_buckets.iter().all(|&v| v == 0)
        {
            return render_empty(out, &p);
        }

        writeln!(out)?;
        header(out, &p, stats)?;
        writeln!(out)?;
        mirror(out, &p, stats)?;
        writeln!(out)?;
        narrative(out, &p, stats)?;
        writeln!(out)?;
        Ok(())
    }
}

// ── header ────────────────────────────────────────────────────────────────────

fn header(out: &mut dyn Write, p: &Paint, stats: &Stats) -> anyhow::Result<()> {
    let title = format!(
        "{}  {}  {}",
        p.bold("sidekick"),
        p.dim("·"),
        p.dim(stats.range.label()),
    );
    let right = stats.generated_at.format("%b %-d, %Y").to_string();
    let pad = LINE_W.saturating_sub(visible_width(&title) + right.len());
    writeln!(out, "   {}{}{}", title, " ".repeat(pad), p.dim(&right))?;
    Ok(())
}

// ── the mirror ────────────────────────────────────────────────────────────────

fn mirror(out: &mut dyn Write, p: &Paint, stats: &Stats) -> anyhow::Result<()> {
    let width = stats.save_buckets.len();
    if width == 0 {
        return Ok(());
    }

    // Dense pipe maze. Generous 8-row corridor with many overlapping pipes,
    // a subtle background field of dots, and a 3-tier collision gradient.
    // Each top file is traced TWICE — once at a top-anchored row going down,
    // once at a bottom-anchored row going up — so its save pattern shows up
    // as two echoes weaving through the grid. With 8 files × 2 passes we get
    // up to 16 pipes interlocking inside the same corridor.
    const ROWS: usize = 8;
    const MAX_FILES: usize = 8;
    let mut grid: Vec<Vec<PipeCell>> = vec![vec![PipeCell::default(); width]; ROWS];

    let mut by_saves: Vec<&crate::analytics::aggregate::FileStats> = stats
        .top_files
        .iter()
        .filter(|f| f.saves > 0 && f.save_buckets.len() == width)
        .collect();
    by_saves.sort_by_key(|f| std::cmp::Reverse(f.saves));

    for (idx, file) in by_saves.iter().take(MAX_FILES).enumerate() {
        // Split this file's saves into two streams by save-index parity, so
        // the two pipes carry *different subsets* of the data and wind at
        // genuinely different buckets. No mirroring, no parallel echoes.
        let mut stream_a: Vec<u32> = vec![0; width];
        let mut stream_b: Vec<u32> = vec![0; width];
        for (i, &n) in file.save_buckets.iter().enumerate() {
            for save_idx in 0..n as usize {
                if (i + save_idx) % 2 == 0 {
                    stream_a[i] += 1;
                } else {
                    stream_b[i] += 1;
                }
            }
        }

        let pipe_a = (idx * 2) as u8;
        let pipe_b = (idx * 2 + 1) as u8;
        // Asymmetric start rows — offset by a prime so adjacent files don't
        // land at neighbouring rows.
        let row_a = (idx * 3) % ROWS;
        let row_b = (idx * 3 + 5) % ROWS;

        if stream_a.iter().any(|&n| n > 0) {
            trace_pipe(&mut grid, &stream_a, row_a, ROWS, pipe_a, 1);
        }
        if stream_b.iter().any(|&n| n > 0) {
            trace_pipe(&mut grid, &stream_b, row_b, ROWS, pipe_b, 1);
        }
    }

    const LABEL_W: usize = 9;
    let blank = " ".repeat(LABEL_W);

    for row in &grid {
        let mut rendered = String::new();
        for cell in row {
            let c = pipe_glyph(cell);
            let pipe_count = cell.pipes.count_ones();
            if c == ' ' {
                // Empty cell — render subtle background texture so the maze
                // sits in a field of dots, not in void.
                rendered.push_str(&p.dim("·"));
            } else if pipe_count >= 3 {
                // Dense collision — three or more pipes share this cell.
                rendered.push_str(&p.accent_strong(&c.to_string()));
            } else if pipe_count == 2 {
                // Standard collision.
                rendered.push_str(&p.accent(&c.to_string()));
            } else {
                rendered.push_str(&p.dim(&c.to_string()));
            }
        }
        writeln!(out, "   {}    {}", blank, rendered)?;
    }

    writeln!(out, "   {}    {}", blank, p.dim(&day_axis(stats, width)))?;
    Ok(())
}

#[derive(Default, Clone, Copy)]
struct PipeCell {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    /// Bitmask of pipe IDs that touched this cell. count_ones() gives the
    /// number of distinct pipes — 2+ means a collision. u32 supports up to
    /// 32 pipes.
    pipes: u32,
}

/// Trace one pipe left-to-right through the grid. Each cell touched gets the
/// pipe's bit set in `pipes`, so we can detect collisions at render time.
fn trace_pipe(
    grid: &mut [Vec<PipeCell>],
    saves: &[u32],
    start_row: usize,
    rows: usize,
    pipe_id: u8,
    initial_direction: i32,
) {
    let mask: u32 = 1u32 << pipe_id;
    let width = saves.len();
    let mut current_row = start_row;
    let mut direction: i32 = initial_direction;

    let mark = |grid: &mut [Vec<PipeCell>], r: usize, c: usize| {
        grid[r][c].pipes |= mask;
    };

    for col in 0..width {
        grid[current_row][col].left = true;
        mark(grid, current_row, col);

        let saves_here = saves[col];
        if saves_here == 0 {
            grid[current_row][col].right = true;
            continue;
        }

        // Cap turn-step at 2 rows regardless of save count. Smaller steps
        // mean pipes wind more frequently inside the corridor — more
        // crossings, more density.
        let steps = (saves_here as usize).clamp(1, 2);
        let mut target: i32 = current_row as i32 + direction * (steps as i32);
        if target < 0 || target >= rows as i32 {
            direction = -direction;
            target = current_row as i32 + direction * (steps as i32);
            target = target.clamp(0, (rows - 1) as i32);
        }
        let target = target as usize;

        if target > current_row {
            grid[current_row][col].down = true;
            for r in (current_row + 1)..target {
                grid[r][col].up = true;
                grid[r][col].down = true;
                mark(grid, r, col);
            }
            grid[target][col].up = true;
            grid[target][col].right = true;
            mark(grid, target, col);
        } else if target < current_row {
            grid[current_row][col].up = true;
            for r in (target + 1)..current_row {
                grid[r][col].up = true;
                grid[r][col].down = true;
                mark(grid, r, col);
            }
            grid[target][col].down = true;
            grid[target][col].right = true;
            mark(grid, target, col);
        } else {
            grid[current_row][col].right = true;
        }

        current_row = target;
    }
}

/// Pick the box-drawing glyph that matches the cell's connected edges.
fn pipe_glyph(c: &PipeCell) -> char {
    match (c.up, c.down, c.left, c.right) {
        (false, false, false, false) => ' ',
        (true, false, false, false) => '╵',
        (false, true, false, false) => '╷',
        (false, false, true, false) => '╴',
        (false, false, false, true) => '╶',
        (true, true, false, false) => '│',
        (false, false, true, true) => '─',
        (true, false, true, false) => '┘',
        (true, false, false, true) => '└',
        (false, true, true, false) => '┐',
        (false, true, false, true) => '┌',
        (true, true, true, false) => '┤',
        (true, true, false, true) => '├',
        (true, false, true, true) => '┴',
        (false, true, true, true) => '┬',
        (true, true, true, true) => '┼',
    }
}

fn day_axis(stats: &Stats, width: usize) -> String {
    use crate::analytics::TimeRange;
    use chrono::Datelike;

    let mut row = vec![' '; width];

    // Pick labels appropriate to the window. Weekday abbreviations only make
    // sense across ~7 days; longer ranges compress days into fractions of a
    // column, so we shift to coarser markers.
    let labels: Vec<(usize, String)> = match stats.range {
        TimeRange::Week => stats
            .day_markers
            .iter()
            .map(|(i, d)| (*i, d.format("%a").to_string().to_lowercase()))
            .collect(),
        TimeRange::Month => stats
            .day_markers
            .iter()
            .filter(|(_, d)| d.weekday().num_days_from_monday() == 0)
            .map(|(i, d)| (*i, d.format("%-d").to_string()))
            .collect(),
        TimeRange::Year | TimeRange::All => stats
            .day_markers
            .iter()
            .filter(|(_, d)| d.day() == 1)
            .map(|(i, d)| (*i, d.format("%b").to_string().to_lowercase()))
            .collect(),
    };

    // Place left-to-right; skip any label that would touch the previous one,
    // so adjacent month/week boundaries don't render as mush.
    let mut next_free = 0usize;
    for (idx, label) in &labels {
        let chars: Vec<char> = label.chars().collect();
        if *idx < next_free || idx + chars.len() > width {
            continue;
        }
        for (i, c) in chars.iter().enumerate() {
            row[idx + i] = *c;
        }
        next_free = idx + chars.len() + 1;
    }
    row.into_iter().collect()
}

// ── narrative: rotating frames ───────────────────────────────────────────────

fn narrative(out: &mut dyn Write, p: &Paint, stats: &Stats) -> anyhow::Result<()> {
    // Line 1: headline framing of the magnitude.
    if let Some(line) = headline(stats) {
        writeln!(out, "   {}", line)?;
    }

    // Line 2: a supporting detail — the most salient pattern from the data.
    if let Some(line) = detail(stats, p) {
        writeln!(out, "   {}", line)?;
    }

    // Line 3 (sometimes): the meta jab — drops in when the user has been
    // checking often, returning after a gap, or crossing a milestone.
    if let Some(line) = jab(stats, p) {
        writeln!(out)?;
        writeln!(out, "   {}  {}", p.accent("✦"), line)?;
    }

    Ok(())
}

fn headline(stats: &Stats) -> Option<String> {
    let saves = stats.saves;
    let attempts = stats.total_decisions;
    if attempts == 0 {
        return Some("quiet stretch. the AI barely came calling.".to_string());
    }
    let p = Paint::new(true);
    let s = p.accent_strong(&saves.to_string());
    let a = p.bold(&attempts.to_string());
    // Rotate by visit count + a data signature so language varies per visit
    // AND tracks data changes.
    let frame = (stats.views_total as usize + attempts as usize) % 5;
    Some(match frame {
        0 => format!(
            "{s} times this {span}, you both reached for the same file.",
            span = span_word(stats)
        ),
        1 => format!("{s} of the AI's {a} attempts landed on what you were typing."),
        2 => format!(
            "you held the line {s} times. the other {} edits, the AI moved freely.",
            attempts - saves
        ),
        3 => format!("the AI reached {a} times. you were there for {s} of them."),
        _ => format!("{a} attempts, {s} crossings. the rest, you weren't in the room."),
    })
}

fn detail(stats: &Stats, p: &Paint) -> Option<String> {
    // Pick the most salient single observation. Each insight has a small
    // pool of phrasings keyed off a stable seed so the words rotate.

    // Hottest day (only if one stands out)
    let hottest_day = stats.by_day.iter().max_by_key(|(_, d)| d.decisions);
    let (hot_day, hot_count) = match hottest_day {
        Some((d, day)) if day.decisions >= 5 => (Some(*d), day.decisions),
        _ => (None, 0),
    };

    // Dominant file
    let dom_file = stats.top_files.first().filter(|f| f.saves >= 3);

    // Peak hour
    let peak_hour = stats
        .by_hour
        .iter()
        .enumerate()
        .max_by_key(|&(_, &v)| v)
        .filter(|&(_, &v)| v >= 5)
        .map(|(h, &v)| (h, v));

    // Score and pick the winner.
    let mut best: Option<(u64, String)> = None;
    let mut consider = |score: u64, msg: String| {
        if score == 0 {
            return;
        }
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, msg));
        }
    };

    // All phrasing seeds fold in `views_total` so language rotates each visit,
    // even when the underlying data hasn't changed.
    let visit = stats.views_total as usize;

    if let (Some(day), n) = (hot_day, hot_count) {
        let weekday = day.format("%A").to_string().to_lowercase();
        let span = span_word(stats);
        let phrasings = [
            format!("{weekday} morning was the loudest stretch."),
            format!("{weekday} carried the {span} — {n} edits in a day."),
            format!("you ran {weekday} hard."),
        ];
        let msg = phrasings[(visit + n as usize) % phrasings.len()].clone();
        consider(n as u64 * 100, p.dim(&msg).to_string());
    }

    if let Some(f) = dom_file {
        let short = compact_path(&f.path, 36);
        let total = f.total;
        let phrasings = [
            format!(
                "{short} took most of the heat — {} of the catches.",
                f.saves
            ),
            format!("one file kept calling for help: {short}."),
            format!(
                "{} edits attempted on {short}. the AI couldn't stay away.",
                total
            ),
        ];
        let msg = phrasings[(visit + f.saves as usize) % phrasings.len()].clone();
        consider(f.saves as u64 * 200, p.dim(&msg).to_string());
    }

    if let Some((h, count)) = peak_hour {
        let when = format_hour(h);
        let phrasings = [
            format!("the storm hits around {when}."),
            format!("{when} is when sidekick works hardest."),
            format!("most of it lives around {when}."),
        ];
        let msg = phrasings[(visit + count as usize + h) % phrasings.len()].clone();
        consider(count as u64 * 40, p.dim(&msg).to_string());
    }

    // Quiet weekend callout if applicable
    let weekend_quiet = stats
        .by_day
        .iter()
        .filter(|(d, _)| {
            use chrono::Datelike;
            let wd = d.weekday().num_days_from_monday();
            wd == 5 || wd == 6 // sat, sun
        })
        .all(|(_, d)| d.decisions == 0);
    if weekend_quiet
        && stats.by_day.iter().any(|(d, _)| {
            use chrono::Datelike;
            let wd = d.weekday().num_days_from_monday();
            wd == 5 || wd == 6
        })
    {
        consider(50, p.dim("the weekend was the eye of it.").to_string());
    }

    best.map(|(_, msg)| msg)
}

fn jab(stats: &Stats, p: &Paint) -> Option<String> {
    // First time ever: a quiet welcome rather than a callout.
    if stats.views_total <= 1 {
        return None;
    }

    // Frequency gate. The jab is variable reward — showing it every visit
    // turns it into noise. Fire roughly two out of every five qualifying
    // visits, deterministic per-visit so it feels random but stable.
    let gate = (stats.views_total + stats.views_today) % 5;
    if gate >= 2 {
        return None;
    }

    // The visit just appended counts as one of `views_today`/`views_total`.
    let today = stats.views_today;

    // Heavy repeat-checking — gentle ribbing.
    if today >= 5 {
        let phrasings = [
            format!("{} checks today. everything okay?", today),
            format!("{} look today. you've been here.", ordinal(today)),
            format!("checking in {} times — sidekick noticed.", today),
        ];
        return Some(phrasings[(today as usize) % phrasings.len()].clone());
    }

    if today >= 3 {
        let phrasings = [
            format!("{} visit today. obsessed yet?", ordinal(today)),
            format!("{} looks today.", today),
            format!("your {} time on this screen today.", ordinal(today)),
        ];
        return Some(phrasings[(today as usize) % phrasings.len()].clone());
    }

    if today == 2 {
        let phrasings = [
            "second look today.".to_string(),
            "back again. couldn't help yourself.".to_string(),
            "twice today. fair.".to_string(),
        ];
        let _ = p;
        return Some(phrasings[(stats.views_total as usize) % phrasings.len()].clone());
    }

    // Returned after a gap.
    if let Some(hours) = stats.hours_since_last_view
        && hours >= 72
    {
        let days = hours / 24;
        return Some(format!("first peek in {} days. welcome back.", days));
    }

    // Milestones.
    if stats.views_total == 10
        || stats.views_total == 25
        || stats.views_total == 50
        || stats.views_total == 100
    {
        return Some(format!(
            "{} checks all-time. neat round number.",
            stats.views_total
        ));
    }

    None
}

fn ordinal(n: u32) -> String {
    match n {
        1 => "first".to_string(),
        2 => "second".to_string(),
        3 => "third".to_string(),
        4 => "fourth".to_string(),
        5 => "fifth".to_string(),
        6 => "sixth".to_string(),
        7 => "seventh".to_string(),
        _ => {
            let suffix = match n % 100 {
                11..=13 => "th",
                _ => match n % 10 {
                    1 => "st",
                    2 => "nd",
                    3 => "rd",
                    _ => "th",
                },
            };
            format!("{}{}", n, suffix)
        }
    }
}

fn span_word(stats: &Stats) -> &'static str {
    use crate::analytics::TimeRange;
    match stats.range {
        TimeRange::Week => "week",
        TimeRange::Month => "month",
        TimeRange::Year => "year",
        TimeRange::All => "stretch",
    }
}

fn format_hour(h: usize) -> String {
    match h {
        0 => "midnight".into(),
        12 => "noon".into(),
        1..=11 => format!("{}am", h),
        13..=23 => format!("{}pm", h - 12),
        _ => format!("{}:00", h),
    }
}

fn render_empty(out: &mut dyn Write, p: &Paint) -> anyhow::Result<()> {
    writeln!(out)?;
    writeln!(out, "   {}", p.bold("sidekick is quiet."))?;
    writeln!(
        out,
        "   {}",
        p.dim("no events yet — come back after some edits.")
    )?;
    writeln!(out)?;
    Ok(())
}

// ── visual primitives ─────────────────────────────────────────────────────────

const LINE_W: usize = 70;

fn visible_width(s: &str) -> usize {
    let mut count = 0usize;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c2 in chars.by_ref() {
                if c2 == 'm' {
                    break;
                }
            }
        } else {
            count += 1;
        }
    }
    count
}

fn compact_path(path: &str, max_w: usize) -> String {
    if path.chars().count() <= max_w {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    let candidate = format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]);
    if candidate.chars().count() <= max_w {
        candidate
    } else {
        let last = parts.last().copied().unwrap_or(path);
        format!("…/{last}")
    }
}

// ── color ────────────────────────────────────────────────────────────────────

struct Paint {
    enabled: bool,
}

impl Paint {
    fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn wrap(&self, prefix: &str, s: &str) -> String {
        if self.enabled {
            format!("{}{}\x1b[0m", prefix, s)
        } else {
            s.to_string()
        }
    }

    fn bold(&self, s: &str) -> String {
        self.wrap("\x1b[1m", s)
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("\x1b[2m", s)
    }
    fn accent(&self, s: &str) -> String {
        self.wrap("\x1b[38;2;217;119;87m", s)
    }
    fn accent_strong(&self, s: &str) -> String {
        self.wrap("\x1b[1;38;2;217;119;87m", s)
    }
}
