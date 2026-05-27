//! opencode harness — bridge plugin model. The .ts bridge translates
//! opencode's in-process events into the sidekick wire protocol.

use std::path::PathBuf;

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub(crate) const PLUGIN_SRC: &str = include_str!("../../plugins/opencode/sidekick.ts");

pub struct Opencode;

impl Harness for Opencode {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn accent(&self) -> &'static str {
        "38;2;91;155;213" // #5B9BD5
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: true,
            selection_inject: true,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets opencode respect your unsaved Neovim buffers before it writes."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("opencode")
            || dirs::home_dir()
                .map(|h| h.join(".config").join("opencode").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let matched = plugin_files();
        if matched.is_empty() {
            return Err(HarnessError::NotConfigured {
                label: "plugin not installed".into(),
                remedy: vec![
                    "Drop plugins/opencode/sidekick.ts into ~/.config/opencode/plugin/".into(),
                ],
            });
        }
        // Presence isn't enough — a stale or hand-edited plugin is missing
        // fixes. The binary embeds the canonical plugin, so compare bytes.
        let all_current = matched
            .iter()
            .all(|p| std::fs::read_to_string(p).ok().as_deref() == Some(PLUGIN_SRC));
        if all_current {
            Ok(HarnessOk {
                label: "plugin installed".into(),
                paths: matched,
            })
        } else {
            Err(HarnessError::OutOfSync {
                label: "plugin out of sync".into(),
                paths: matched,
                remedy: vec!["The installed plugin differs from this sidekick build.".into()],
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
        let canonical = home
            .join(".config")
            .join("opencode")
            .join("plugin")
            .join("sidekick.ts");
        let path = plugin_files().into_iter().next().unwrap_or(canonical);
        let before = std::fs::read_to_string(&path).ok();
        if before.as_deref() == Some(PLUGIN_SRC) {
            return Vec::new();
        }
        let title = if before.is_some() {
            "Update the opencode plugin"
        } else {
            "Install the opencode plugin"
        };
        vec![Fix::single(title, path, before, PLUGIN_SRC.to_string())]
    }
}

/// Plugin files that already wire up sidekick for opencode.
pub(crate) fn plugin_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    // opencode globs `{plugin,plugins}/*.{ts,js}` under its global config
    // dir (~/.config/opencode) and per-project (.opencode).
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
