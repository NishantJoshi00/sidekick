//! Antigravity harness — native, divergent wire shape. Antigravity's hook
//! envelope has no top-level event tag, uses camelCase field names, and
//! nests the tool call as `toolCall: { name, args }`. PostToolUse fires
//! but omits the toolCall, so the handler broadcasts `:checktime` to
//! every connected Neovim instead of refreshing a single known path.
//! Selection injection is unsupported — Antigravity has no
//! user-prompt-submit event to ride on.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub struct Antigravity;

impl Harness for Antigravity {
    fn name(&self) -> &'static str {
        "Antigravity"
    }

    fn accent(&self) -> &'static str {
        "38;2;66;133;244" // #4285F4
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            // PostToolUse omits toolCall, so the handler can't target a
            // single file. Instead it broadcasts `:checktime` to every
            // connected Neovim — clean buffers reload, dirty buffers
            // stay protected by Vim's W12 prompt.
            buffer_refresh: true,
            selection_inject: false,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets Antigravity respect your unsaved Neovim buffers before it writes."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("agy")
            || dirs::home_dir()
                .map(|h| h.join(".gemini").join("config").is_dir())
                .unwrap_or(false)
            || std::env::current_dir()
                .map(|c| c.join(".agents").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let paths = hook_files();
        if paths.is_empty() {
            Err(HarnessError::NotConfigured {
                label: "hook not registered".into(),
                remedy: vec![
                    "Run `sidekick doctor --fix` to merge the hook into ~/.gemini/config/hooks.json"
                        .into(),
                ],
            })
        } else {
            Ok(HarnessOk {
                label: "hook registered".into(),
                paths,
            })
        }
    }

    fn fix(&self) -> Vec<Fix> {
        if !self.is_installed() || !hook_files().is_empty() {
            return Vec::new();
        }
        let path = preferred_install_path();
        let before = std::fs::read_to_string(&path).ok();
        let Ok(after) = config_after(before.as_deref()) else {
            return Vec::new();
        };
        vec![Fix::single(
            "Register the Antigravity hook",
            path,
            before,
            after,
        )]
    }
}

/// Candidate Antigravity hooks-config files in precedence order: project
/// local (`.agents/hooks.json`) first, then global
/// (`~/.gemini/config/hooks.json`).
fn config_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".agents").join("hooks.json"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".gemini").join("config").join("hooks.json"));
    }

    candidates
}

/// Where the fix writes when no hook is configured anywhere. Prefer the
/// per-project file if a `.agents/` directory already exists; otherwise
/// fall back to the global path.
fn preferred_install_path() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        let project = cwd.join(".agents");
        if project.is_dir() {
            return project.join("hooks.json");
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".gemini").join("config").join("hooks.json"))
        .unwrap_or_else(|| PathBuf::from(".agents/hooks.json"))
}

pub(crate) fn hook_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = config_candidates()
        .into_iter()
        .filter(|p| crate::harness::shared::file_mentions_sidekick_hook(p))
        .collect();
    matched.sort();
    matched.dedup();
    matched
}

/// Merge sidekick's PreToolUse + PostToolUse handlers into an Antigravity
/// `hooks.json`, preserving the user's other top-level hook entries and
/// keys untouched.
pub(super) fn config_after(before: Option<&str>) -> Result<String> {
    let mut root: serde_json::Value = match before {
        Some(s) if !s.trim().is_empty() => {
            serde_json::from_str(s).context("hooks.json isn't valid JSON")?
        }
        _ => serde_json::json!({}),
    };
    {
        let obj = root
            .as_object_mut()
            .context("hooks.json isn't a JSON object")?;
        let sidekick = obj
            .entry("sidekick")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("`sidekick` in hooks.json isn't an object")?;
        let handler = serde_json::json!({
            "matcher": "^(write_to_file|replace_file_content|multi_replace_file_content)$",
            "hooks": [
                {
                    "type": "command",
                    "command": "sidekick hook",
                    "timeout": 10,
                },
            ],
        });
        sidekick
            .entry("PreToolUse")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("`sidekick.PreToolUse` in hooks.json isn't an array")?
            .push(handler.clone());
        sidekick
            .entry("PostToolUse")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("`sidekick.PostToolUse` in hooks.json isn't an array")?
            .push(handler);
    }
    let mut s = serde_json::to_string_pretty(&root)?;
    s.push('\n');
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::config_after;

    #[test]
    fn merges_hook_into_empty_config() {
        let out = config_after(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        for event in ["PreToolUse", "PostToolUse"] {
            let arr = v["sidekick"][event].as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(
                arr[0]["matcher"],
                "^(write_to_file|replace_file_content|multi_replace_file_content)$"
            );
            let handlers = arr[0]["hooks"].as_array().unwrap();
            assert_eq!(handlers.len(), 1);
            assert_eq!(handlers[0]["type"], "command");
            assert_eq!(handlers[0]["command"], "sidekick hook");
            assert_eq!(handlers[0]["timeout"], 10);
        }
    }

    #[test]
    fn keeps_existing_hooks_and_keys() {
        let before = r#"{
  "my-linter": {
    "PostToolUse": [
      {
        "matcher": "write_to_file",
        "hooks": [{ "type": "command", "command": "./lint.sh" }]
      }
    ]
  }
}"#;
        let out = config_after(Some(before)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        let lint = v["my-linter"]["PostToolUse"].as_array().unwrap();
        assert_eq!(lint.len(), 1);
        assert_eq!(lint[0]["hooks"][0]["command"], "./lint.sh");

        for event in ["PreToolUse", "PostToolUse"] {
            let sk = v["sidekick"][event].as_array().unwrap();
            assert_eq!(sk.len(), 1);
            assert_eq!(sk[0]["hooks"][0]["command"], "sidekick hook");
        }
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(config_after(Some("{ not json")).is_err());
    }
}
