//! pi harness — bridge extension model. The .ts extension translates
//! pi's in-process events into the sidekick wire protocol.

use std::path::PathBuf;

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub(crate) const EXTENSION_SRC: &str = include_str!("../../plugins/pi/sidekick.ts");

pub struct Pi;

impl Harness for Pi {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn accent(&self) -> &'static str {
        "38;2;157;124;216" // #9D7CD8
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: true,
            selection_inject: true,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets the pi agent respect your unsaved Neovim buffers before it writes."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("pi")
            || dirs::home_dir()
                .map(|h| h.join(".pi").join("agent").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let matched = extension_files();
        if matched.is_empty() {
            return Err(HarnessError::NotConfigured {
                label: "extension not installed".into(),
                remedy: vec![
                    "Run `sidekick doctor --fix` to install the extension into ~/.pi/agent/extensions/sidekick.ts"
                        .into(),
                ],
            });
        }
        let all_current = matched
            .iter()
            .all(|p| std::fs::read_to_string(p).ok().as_deref() == Some(EXTENSION_SRC));
        if all_current {
            Ok(HarnessOk {
                label: "extension installed".into(),
                paths: matched,
            })
        } else {
            Err(HarnessError::OutOfSync {
                label: "extension out of sync".into(),
                paths: matched,
                remedy: vec!["The installed extension differs from this sidekick build.".into()],
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
            .join(".pi")
            .join("agent")
            .join("extensions")
            .join("sidekick.ts");
        let path = extension_files().into_iter().next().unwrap_or(canonical);
        let before = std::fs::read_to_string(&path).ok();
        if before.as_deref() == Some(EXTENSION_SRC) {
            return Vec::new();
        }
        let title = if before.is_some() {
            "Update the pi extension"
        } else {
            "Install the pi extension"
        };
        vec![Fix::single(title, path, before, EXTENSION_SRC.to_string())]
    }
}

pub(crate) fn extension_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    // pi loads `extensions/*.{ts,js}` from its global agent config dir
    // (~/.pi/agent) and per-project (.pi).
    let mut extension_dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        extension_dirs.push(home.join(".pi").join("agent").join("extensions"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        extension_dirs.push(cwd.join(".pi").join("extensions"));
    }
    for dir in &extension_dirs {
        for ext in ["ts", "js"] {
            let candidate = dir.join(format!("sidekick.{ext}"));
            if candidate.is_file() {
                matched.push(candidate);
            }
        }
    }

    matched.sort();
    matched.dedup();
    matched
}
