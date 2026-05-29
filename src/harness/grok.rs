//! Grok harness — native, with its own wire shape. Despite Grok's docs
//! claiming Claude Code compatibility, the hook envelope diverges: field
//! names are camelCase, event-tag values are snake_case, and tool names are
//! Grok-specific (see `hook::GrokShape`). The deny output is also distinct
//! — Grok reads a top-level `{"decision":"deny"}`, not the nested Claude
//! `permissionDecision`. Selection injection is unsupported: Grok's
//! UserPromptSubmit is a passive event whose hook stdout is ignored, so
//! the capability triple is `true / true / false`, like Amp.
//!
//! Grok merges every `*.json` file in its hooks directory at startup, so
//! sidekick owns `sidekick.json` outright rather than merging into a shared
//! settings file. That means a byte-compare against the embedded source
//! detects stale installs (the pi/opencode mechanics) instead of a
//! key-preserving JSON merge (the Claude Code mechanics).

use std::path::PathBuf;

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub(crate) const HOOK_SRC: &str = include_str!("../../plugins/grok/sidekick.json");

pub struct Grok;

impl Harness for Grok {
    fn name(&self) -> &'static str {
        "Grok"
    }

    fn accent(&self) -> &'static str {
        "38;2;6;182;212" // #06B6D4
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: true,
            // Grok's UserPromptSubmit is passive — hook stdout is ignored,
            // so there's no channel to inject the Neovim visual selection.
            selection_inject: false,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets Grok check your unsaved Neovim buffers before it edits."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("grok")
            || dirs::home_dir()
                .map(|h| h.join(".grok").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let matched = hook_files();
        if matched.is_empty() {
            return Err(HarnessError::NotConfigured {
                label: "hook not registered".into(),
                remedy: vec![
                    "Run `sidekick doctor --fix` to install the hook config into ~/.grok/hooks/sidekick.json"
                        .into(),
                ],
            });
        }
        let all_current = matched
            .iter()
            .all(|p| std::fs::read_to_string(p).ok().as_deref() == Some(HOOK_SRC));
        if all_current {
            Ok(HarnessOk {
                label: "hook registered".into(),
                paths: matched,
            })
        } else {
            Err(HarnessError::OutOfSync {
                label: "hook out of sync".into(),
                paths: matched,
                remedy: vec!["The installed hook config differs from this sidekick build.".into()],
            })
        }
    }

    fn fix(&self) -> Vec<Fix> {
        if !self.is_installed() {
            return Vec::new();
        }
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        let canonical = home.join(".grok").join("hooks").join("sidekick.json");
        let path = hook_files().into_iter().next().unwrap_or(canonical);
        let before = std::fs::read_to_string(&path).ok();
        if before.as_deref() == Some(HOOK_SRC) {
            return Vec::new();
        }
        let title = if before.is_some() {
            "Update the Grok hook config"
        } else {
            "Install the Grok hook config"
        };
        vec![Fix::single(title, path, before, HOOK_SRC.to_string())]
    }
}

/// `sidekick.json` files installed in Grok's hooks directories — global
/// (`~/.grok/hooks`) and per-project (`.grok/hooks`).
pub(crate) fn hook_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    let mut hook_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        hook_dirs.push(home.join(".grok").join("hooks"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        hook_dirs.push(cwd.join(".grok").join("hooks"));
    }
    for dir in &hook_dirs {
        let candidate = dir.join("sidekick.json");
        if candidate.is_file() {
            matched.push(candidate);
        }
    }

    matched.sort();
    matched.dedup();
    matched
}
