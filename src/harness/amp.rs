//! Amp harness — bridge plugin model. The .ts plugin translates Amp's
//! in-process plugin events into the sidekick wire protocol.

use std::path::PathBuf;

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub(crate) const PLUGIN_SRC: &str = include_str!("../../plugins/amp/sidekick.ts");

pub struct Amp;

impl Harness for Amp {
    fn name(&self) -> &'static str {
        "Amp"
    }

    fn accent(&self) -> &'static str {
        "38;2;239;68;68" // #EF4444
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: true,
            // Amp emits no UserPromptSubmit-equivalent event; agent.start
            // fires on prompt submission but exposes no documented hook
            // into visual-selection context.
            selection_inject: false,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets the Amp coding agent respect your unsaved Neovim buffers before it writes."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("amp")
            || dirs::home_dir()
                .map(|h| h.join(".config").join("amp").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let matched = plugin_files();
        if matched.is_empty() {
            return Err(HarnessError::NotConfigured {
                label: "plugin not installed".into(),
                remedy: vec![
                    "Run `sidekick doctor --fix` to install the plugin into ~/.config/amp/plugins/sidekick.ts"
                        .into(),
                ],
            });
        }
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
            .join("amp")
            .join("plugins")
            .join("sidekick.ts");
        let path = plugin_files().into_iter().next().unwrap_or(canonical);
        let before = std::fs::read_to_string(&path).ok();
        if before.as_deref() == Some(PLUGIN_SRC) {
            return Vec::new();
        }
        let title = if before.is_some() {
            "Update the Amp plugin"
        } else {
            "Install the Amp plugin"
        };
        vec![Fix::single(title, path, before, PLUGIN_SRC.to_string())]
    }
}

pub(crate) fn plugin_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    // Amp loads `plugins/*.ts` from the global config dir
    // (~/.config/amp) and per-project (.amp).
    let mut plugin_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        plugin_dirs.push(home.join(".config").join("amp").join("plugins"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        plugin_dirs.push(cwd.join(".amp").join("plugins"));
    }
    for dir in &plugin_dirs {
        let candidate = dir.join("sidekick.ts");
        if candidate.is_file() {
            matched.push(candidate);
        }
    }

    matched.sort();
    matched.dedup();
    matched
}
