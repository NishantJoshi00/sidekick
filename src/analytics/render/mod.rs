//! Render layer. Isolated behind the `Renderer` trait so we can iterate
//! creatively on the presentation without disturbing the data pipeline.
//!
//! A renderer takes an aggregated `Stats` and writes its output somewhere.
//! Today the only implementation is `terminal::TerminalRenderer` (ANSI text
//! to a writer). Future implementations (PNG, SVG, kitty graphics, scripted
//! reveals) can plug in behind the same trait — or, if they don't fit the
//! "write bytes" model, sit alongside it without forcing breaking changes.

pub mod terminal;

use crate::analytics::aggregate::Stats;

pub trait Renderer {
    fn render(&self, stats: &Stats, out: &mut dyn std::io::Write) -> anyhow::Result<()>;
}
