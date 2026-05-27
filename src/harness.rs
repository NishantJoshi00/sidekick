//! The `Harness` trait — sidekick's single door to working with any AI
//! coding agent.
//!
//! Every supported harness (Claude Code, Codex, opencode, pi, Crush) is a
//! unit struct that implements [`Harness`]. The trait covers the harness's
//! installation lifecycle: identity, detection, doctor check, fix. Hook
//! wire parsing lives in `hook.rs` because it's a wire-level concern, not
//! per-harness — the parser sees only the JSON, not which host emitted it.
//!
//! Consumers iterate [`registry::all`] instead of branching on per-harness
//! functions. Adding a new harness is: implement [`Harness`], add the unit
//! struct to [`registry::all`], done.

use std::path::PathBuf;

use crate::fix::Fix;

pub mod claude_code;
pub mod codex;
pub mod crush;
pub mod opencode;
pub mod pi;
pub(crate) mod shared;

pub mod registry {
    //! The list of harnesses sidekick knows about. Iterate this from
    //! doctor, fix, init — there is no other source of truth.

    use super::Harness;

    pub fn all() -> Vec<Box<dyn Harness>> {
        vec![
            Box::new(super::claude_code::ClaudeCode),
            Box::new(super::codex::Codex),
            Box::new(super::opencode::Opencode),
            Box::new(super::pi::Pi),
            Box::new(super::crush::Crush),
        ]
    }
}

/// What sidekick can do with this harness. Drives the doctor's `support:`
/// line and the README capability matrix. `true` means sidekick fully
/// exercises that capability on this harness; `false` is an upstream gap.
#[derive(Clone, Copy)]
pub struct HarnessCapabilities {
    pub edit_deny: bool,
    pub buffer_refresh: bool,
    pub selection_inject: bool,
}

impl HarnessCapabilities {
    pub fn is_full(&self) -> bool {
        self.edit_deny && self.buffer_refresh && self.selection_inject
    }
}

/// A successful check — sidekick is wired into this harness correctly.
pub struct HarnessOk {
    /// Short label shown next to the green tick (e.g. `"hook registered"`,
    /// `"plugin installed"`, `"extension installed"`).
    pub label: String,
    /// Concrete paths where sidekick lives for this harness — typically
    /// the config files mentioning `sidekick hook` or the installed
    /// bridge file. Shown dim below the label.
    pub paths: Vec<PathBuf>,
}

/// A failed check — sidekick isn't wired in, or the wire-up is stale.
pub enum HarnessError {
    /// Harness is detected but not configured to use sidekick.
    NotConfigured { label: String, remedy: Vec<String> },
    /// Sidekick is installed but the installed file diverges from the
    /// embedded source (a bridge byte-mismatch, a stale codex plugin,
    /// etc.). Paths show what's stale; the remedy is the repair pitch.
    OutOfSync {
        label: String,
        paths: Vec<PathBuf>,
        remedy: Vec<String>,
    },
}

/// The one and only door to working with a harness. New harnesses
/// implement this trait and get added to [`registry::all`]; no other code
/// path branches on harness identity.
pub trait Harness {
    /// Display name shown in doctor and init (e.g. `"Claude Code"`,
    /// `"Crush"`).
    fn name(&self) -> &'static str;

    /// Truecolor ANSI parameters for the harness's brand accent (e.g.
    /// `"38;2;217;119;87"` for Claude orange).
    fn accent(&self) -> &'static str;

    fn capabilities(&self) -> HarnessCapabilities;

    /// One-line description shown in `sidekick init` describing what
    /// installing this harness lets sidekick do.
    fn init_description(&self) -> &'static str;

    /// Is this harness present on the user's machine? Drives whether
    /// doctor renders a row for it at all. Combines binary-on-PATH and
    /// home-directory checks, typically.
    fn is_installed(&self) -> bool;

    /// Runtime status — is sidekick correctly wired into this harness?
    /// Used by `sidekick doctor` to render the per-harness row.
    fn check(&self) -> Result<HarnessOk, HarnessError>;

    /// Repair actions, presented to the user under `--fix` or `init`.
    /// Multi-file harnesses (Codex) return several Fixes; single-file
    /// ones return one (or zero, if already configured).
    fn fix(&self) -> Vec<Fix>;
}
