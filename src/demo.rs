//! `sidekick demo` — bundled asciinema playback inside a media-player TUI.
//!
//! Drives an `avt::Vt` virtual terminal from a bundled `.cast` file and
//! copies its visible grid into a ratatui buffer each frame. The whole
//! thing renders inside a bordered "player" widget so it's visually
//! distinct from the user's real terminal.

use std::io;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, LineGauge, Paragraph};
use serde::Deserialize;

const DEMO_CAST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/demo.cast"));
const FRAME_POLL: Duration = Duration::from_millis(33); // ~30fps

#[derive(Deserialize)]
struct CastHeader {
    version: u8,
    // asciicast v2 puts terminal dimensions at the top level...
    width: Option<u16>,
    height: Option<u16>,
    // ...asciicast v3 nests them under `term`.
    term: Option<CastTerm>,
}

#[derive(Deserialize)]
struct CastTerm {
    cols: u16,
    rows: u16,
}

struct Cast {
    width: u16,
    height: u16,
    events: Vec<CastEvent>,
}

struct CastEvent {
    time: f64,
    data: String,
}

fn parse_cast(bytes: &[u8]) -> anyhow::Result<Cast> {
    let text = std::str::from_utf8(bytes).context("data is corrupted")?;
    let mut lines = text.lines();
    let header_line = lines.next().context("demo is empty")?;
    let header: CastHeader = serde_json::from_str(header_line).context("header is malformed")?;
    if header.version != 2 && header.version != 3 {
        return Err(anyhow!(
            "unsupported version {} (need v2 or v3)",
            header.version
        ));
    }
    let (width, height) = match (header.width, header.height, &header.term) {
        (Some(w), Some(h), _) => (w, h),
        (_, _, Some(term)) => (term.cols, term.rows),
        _ => return Err(anyhow!("header missing terminal dimensions")),
    };

    // asciicast v2 event times are absolute; v3 times are intervals since
    // the previous event. Accumulate v3 intervals into an absolute timeline.
    let v3_intervals = header.version == 3;
    let mut clock = 0.0f64;
    let mut events = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line).context("frame is malformed")?;
        let Some(arr) = value.as_array() else {
            continue;
        };
        if arr.len() < 3 {
            continue;
        }
        let Some(time) = arr[0].as_f64() else {
            continue;
        };
        // Accumulate before the "o" filter so skipped events (e.g. a
        // trailing "x" exit marker) don't desync the v3 clock.
        let abs_time = if v3_intervals {
            clock += time;
            clock
        } else {
            time
        };
        if arr[1].as_str() != Some("o") {
            continue;
        }
        let Some(data) = arr[2].as_str() else {
            continue;
        };
        events.push(CastEvent {
            time: abs_time,
            data: data.to_string(),
        });
    }
    Ok(Cast {
        width,
        height,
        events,
    })
}

pub fn run() -> anyhow::Result<()> {
    let cast = parse_cast(DEMO_CAST).context("couldn't load demo")?;

    let (term_cols, term_rows) = ratatui::crossterm::terminal::size()?;

    // Approximate aspect-ratio guard: refuse only when the terminal is
    // genuinely wrong-shaped for a widescreen cast (i.e. tall and narrow).
    // 50% threshold is permissive — same-shape shrinks pass, 4:3-ish
    // terminals pass, only the square / portrait shapes refuse.
    const MIN_RATIO_FRAC: f64 = 0.50;
    let cast_ratio = cast.width as f64 / cast.height as f64;
    let term_ratio = term_cols as f64 / term_rows as f64;
    let threshold = cast_ratio * MIN_RATIO_FRAC;
    if term_ratio < threshold {
        let min_cols = (threshold * term_rows as f64).ceil() as u16;
        let max_rows = (term_cols as f64 / threshold).floor() as u16;
        print_aspect_mismatch_error(
            term_cols,
            term_rows,
            cast.width,
            cast.height,
            min_cols,
            max_rows,
        );
        std::process::exit(2);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = playback_loop(&mut terminal, &cast);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match result? {
        ExitReason::UserQuit => Ok(()),
        ExitReason::AspectChanged {
            cols,
            rows,
            min_cols,
            max_rows,
        } => {
            print_aspect_mismatch_error(cols, rows, cast.width, cast.height, min_cols, max_rows);
            std::process::exit(2);
        }
    }
}

enum ExitReason {
    UserQuit,
    AspectChanged {
        cols: u16,
        rows: u16,
        min_cols: u16,
        max_rows: u16,
    },
}

fn playback_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    cast: &Cast,
) -> anyhow::Result<ExitReason> {
    let mut vt = avt::Vt::new(cast.width as usize, cast.height as usize);
    let mut next_event = 0usize;
    let total = cast.events.last().map(|e| e.time).unwrap_or(0.0);

    let cast_ratio = cast.width as f64 / cast.height as f64;
    const MIN_RATIO_FRAC: f64 = 0.50;
    let ratio_threshold = cast_ratio * MIN_RATIO_FRAC;

    let mut start = Instant::now();
    let mut paused = false;
    let mut pause_start: Option<Instant> = None;
    let mut paused_offset = Duration::ZERO;
    let mut last_play_at: Option<Instant> = Some(Instant::now());
    const PLAY_FLASH: Duration = Duration::from_millis(500);

    loop {
        // Bail if the user resized below the aspect-ratio threshold while
        // playback was running. Tear down cleanly via the caller.
        if let Ok((cols, rows)) = ratatui::crossterm::terminal::size() {
            let term_ratio = cols as f64 / rows as f64;
            if term_ratio < ratio_threshold {
                let min_cols = (ratio_threshold * rows as f64).ceil() as u16;
                let max_rows = (cols as f64 / ratio_threshold).floor() as u16;
                return Ok(ExitReason::AspectChanged {
                    cols,
                    rows,
                    min_cols,
                    max_rows,
                });
            }
        }
        let elapsed_dur = if let Some(ps) = pause_start {
            ps.saturating_duration_since(start)
                .saturating_sub(paused_offset)
        } else {
            Instant::now()
                .saturating_duration_since(start)
                .saturating_sub(paused_offset)
        };
        let elapsed = elapsed_dur.as_secs_f64();

        while next_event < cast.events.len() && cast.events[next_event].time <= elapsed {
            vt.feed_str(&cast.events[next_event].data);
            next_event += 1;
        }
        let done = next_event >= cast.events.len() && elapsed >= total;

        let show_play = !paused
            && last_play_at
                .map(|t| t.elapsed() < PLAY_FLASH)
                .unwrap_or(false);

        terminal.draw(|f| draw_player(f, &vt, elapsed, total, paused, done, show_play))?;

        if event::poll(FRAME_POLL)?
            && let Event::Key(k) = event::read()?
            && k.kind != KeyEventKind::Release
        {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(ExitReason::UserQuit),
                KeyCode::Char(' ') => {
                    if paused {
                        if let Some(ps) = pause_start.take() {
                            paused_offset += ps.elapsed();
                        }
                        paused = false;
                        last_play_at = Some(Instant::now());
                    } else {
                        pause_start = Some(Instant::now());
                        paused = true;
                    }
                }
                KeyCode::Char('r') => {
                    vt = avt::Vt::new(cast.width as usize, cast.height as usize);
                    next_event = 0;
                    start = Instant::now();
                    paused = false;
                    pause_start = None;
                    paused_offset = Duration::ZERO;
                    last_play_at = Some(Instant::now());
                }
                _ => {}
            }
        }
    }
}

fn draw_player(
    f: &mut Frame<'_>,
    vt: &avt::Vt,
    elapsed: f64,
    total: f64,
    paused: bool,
    done: bool,
    show_play: bool,
) {
    let area = f.area();

    let icon = if done {
        "■"
    } else if paused {
        "⏸"
    } else {
        "▶"
    };
    let title = format!(" {icon} sidekick demo ");
    let hints = " [space] pause · [r] restart · [q] quit ";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_bottom(Line::from(hints).right_aligned());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    render_vt(f.buffer_mut(), chunks[0], vt);
    if paused {
        render_pause_overlay(f.buffer_mut(), chunks[0]);
    } else if show_play {
        render_play_overlay(f.buffer_mut(), chunks[0]);
    }
    render_status(f, chunks[1], elapsed, total, paused);
}

fn render_play_overlay(buf: &mut Buffer, area: Rect) {
    // Right-pointing triangle: `◣` for the top slope, `◤` for the bottom
    // slope, `█` for the body. Even height means there's no single middle
    // row — instead the two innermost rows both end at the same column, and
    // their slope cells (`◣` above, `◤` below) share a corner point on the
    // cell boundary. That shared point IS the tip. No special tip glyph.
    const ICON_H: u16 = 10;
    let half = ICON_H / 2;
    let icon_w = half;
    if area.width < icon_w || area.height < ICON_H {
        return;
    }
    let x0 = area.x + (area.width - icon_w) / 2;
    let y0 = area.y + (area.height - ICON_H) / 2;
    let style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    for row in 0..ICON_H {
        let (dist, glyph) = if row < half {
            (half - 1 - row, "◣")
        } else {
            (row - half, "◤")
        };
        let body_w = half - 1 - dist;

        for col in 0..body_w {
            let pos = Position {
                x: x0 + col,
                y: y0 + row,
            };
            if let Some(c) = buf.cell_mut(pos) {
                c.set_symbol("█");
                c.set_style(style);
            }
        }
        let pos = Position {
            x: x0 + body_w,
            y: y0 + row,
        };
        if let Some(c) = buf.cell_mut(pos) {
            c.set_symbol(glyph);
            c.set_style(style);
        }
    }
}

fn render_pause_overlay(buf: &mut Buffer, area: Rect) {
    // Two solid blocks separated by a gap — a classic media-player pause glyph.
    const BAR_W: u16 = 5;
    const GAP_W: u16 = 4;
    const ICON_H: u16 = 8;
    let icon_w = BAR_W * 2 + GAP_W;
    if area.width < icon_w || area.height < ICON_H {
        return;
    }
    let x0 = area.x + (area.width - icon_w) / 2;
    let y0 = area.y + (area.height - ICON_H) / 2;
    let style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    for row in 0..ICON_H {
        for col in 0..icon_w {
            // Left bar: 0..BAR_W. Right bar: BAR_W+GAP_W..icon_w.
            if (BAR_W..BAR_W + GAP_W).contains(&col) {
                continue;
            }
            let pos = Position {
                x: x0 + col,
                y: y0 + row,
            };
            if let Some(c) = buf.cell_mut(pos) {
                c.set_symbol("█");
                c.set_style(style);
            }
        }
    }
}

fn render_vt(buf: &mut Buffer, area: Rect, vt: &avt::Vt) {
    for (y, line) in vt.view().enumerate() {
        let row = y as u16;
        if row >= area.height {
            break;
        }
        for (x, cell) in line.cells().iter().enumerate() {
            let col = x as u16;
            if col >= area.width {
                break;
            }
            if cell.width() == 0 {
                // WideTail: ratatui reserves the column itself when we set the
                // wide head; nothing to do here.
                continue;
            }
            let pos = Position {
                x: area.x + col,
                y: area.y + row,
            };
            if let Some(buf_cell) = buf.cell_mut(pos) {
                let mut s = [0u8; 4];
                let symbol = cell.char().encode_utf8(&mut s);
                buf_cell.set_symbol(symbol);
                buf_cell.set_style(pen_to_style(cell.pen()));
            }
        }
    }

    let cursor = vt.cursor();
    if cursor.visible {
        let col = cursor.col as u16;
        let row = cursor.row as u16;
        if col < area.width && row < area.height {
            let pos = Position {
                x: area.x + col,
                y: area.y + row,
            };
            if let Some(buf_cell) = buf.cell_mut(pos) {
                buf_cell.set_style(buf_cell.style().add_modifier(Modifier::REVERSED));
            }
        }
    }
}

fn pen_to_style(pen: &avt::Pen) -> Style {
    let mut style = Style::default();
    if let Some(fg) = pen.foreground() {
        style = style.fg(map_color(fg));
    }
    if let Some(bg) = pen.background() {
        style = style.bg(map_color(bg));
    }
    let mut mods = Modifier::empty();
    if pen.is_bold() {
        mods |= Modifier::BOLD;
    }
    if pen.is_italic() {
        mods |= Modifier::ITALIC;
    }
    if pen.is_underline() {
        mods |= Modifier::UNDERLINED;
    }
    if pen.is_inverse() {
        mods |= Modifier::REVERSED;
    }
    if pen.is_faint() {
        mods |= Modifier::DIM;
    }
    if pen.is_blink() {
        mods |= Modifier::SLOW_BLINK;
    }
    if pen.is_strikethrough() {
        mods |= Modifier::CROSSED_OUT;
    }
    style.add_modifier(mods)
}

fn map_color(c: avt::Color) -> Color {
    match c {
        avt::Color::Indexed(n) => Color::Indexed(n),
        avt::Color::RGB(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

fn render_status(f: &mut Frame<'_>, area: Rect, elapsed: f64, total: f64, paused: bool) {
    let clamped = elapsed.clamp(0.0, total.max(0.001));
    let time_text = format!("{} / {}", format_time(clamped), format_time(total));
    let prefix = if paused { " ⏸ " } else { " ▶ " };
    let time_w = time_text.len() as u16 + 2;
    let prefix_w = 4u16;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(prefix_w),
            Constraint::Min(1),
            Constraint::Length(time_w),
        ])
        .split(area);

    f.render_widget(
        Paragraph::new(prefix).style(Style::default().fg(Color::Cyan)),
        chunks[0],
    );

    let ratio = if total > 0.0 { clamped / total } else { 0.0 };
    let gauge = LineGauge::default()
        .filled_style(Style::default().fg(Color::Cyan))
        .unfilled_style(Style::default().fg(Color::DarkGray))
        .ratio(ratio.clamp(0.0, 1.0));
    f.render_widget(gauge, chunks[1]);

    f.render_widget(
        Paragraph::new(time_text)
            .style(Style::default().fg(Color::DarkGray))
            .right_aligned(),
        chunks[2],
    );
}

fn format_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn print_aspect_mismatch_error(
    have_w: u16,
    have_h: u16,
    cast_w: u16,
    cast_h: u16,
    min_cols: u16,
    max_rows: u16,
) {
    let red = "\x1b[31m";
    let cyan = "\x1b[36m";
    let dim = "\x1b[2m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    eprintln!();
    eprintln!("  {cyan}▌{reset} {bold}sidekick demo{reset}  {dim}⏹ can't play{reset}");
    eprintln!();
    eprintln!("    {red}⚠{reset}  {bold}Your terminal is the wrong shape for this demo.{reset}");
    eprintln!();
    eprintln!("        {dim}your terminal:{reset}   {bold}{have_w}×{have_h}{reset}");
    eprintln!("        {dim}demo:{reset}            {bold}{cast_w}×{cast_h}{reset}");
    eprintln!();
    eprintln!(
        "    {dim}Widen to at least{reset} {bold}{min_cols}{reset} {dim}cols{reset}{dim}, or shorten to{reset} {bold}{max_rows}{reset} {dim}rows.{reset}"
    );
    eprintln!();
}
