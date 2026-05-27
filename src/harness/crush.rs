//! Crush harness — native, divergent wire shape. Crush emits `event`
//! instead of `hook_event_name` and omits `transcript_path`. Crush also
//! supports only `PreToolUse` — no post-edit refresh, no selection
//! injection.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub struct Crush;

impl Harness for Crush {
    fn name(&self) -> &'static str {
        "Crush"
    }

    fn accent(&self) -> &'static str {
        "38;2;236;72;153" // #EC4899
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: false,
            selection_inject: false,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets Crush respect your unsaved Neovim buffers before it edits."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("crush")
            || dirs::home_dir()
                .map(|h| h.join(".config").join("crush").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let paths = hook_files();
        if paths.is_empty() {
            Err(HarnessError::NotConfigured {
                label: "hook not registered".into(),
                remedy: vec![
                    "Run `sidekick doctor --fix` to merge the hook into ~/.config/crush/crush.json"
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
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        let path = home.join(".config").join("crush").join("crush.json");
        let before = std::fs::read_to_string(&path).ok();
        let Ok(after) = settings_after(before.as_deref()) else {
            return Vec::new();
        };
        vec![Fix::single("Register the Crush hook", path, before, after)]
    }
}

/// Candidate Crush config files in precedence order: project local first,
/// then global (`$CRUSH_GLOBAL_CONFIG` overrides the default global path).
fn config_candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".crush.json"));
        candidates.push(cwd.join("crush.json"));
    }

    if let Some(path) = std::env::var_os("CRUSH_GLOBAL_CONFIG") {
        candidates.push(PathBuf::from(path));
    } else if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("crush").join("crush.json"));
    }

    candidates
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

/// Merge sidekick's PreToolUse hook into a Crush `crush.json`, preserving
/// any existing keys (and the user's own hooks) untouched.
pub(super) fn settings_after(before: Option<&str>) -> Result<String> {
    let mut root: serde_json::Value = match before {
        Some(s) if !s.trim().is_empty() => {
            serde_json::from_str(s).context("~/.config/crush/crush.json isn't valid JSON")?
        }
        _ => serde_json::json!({ "$schema": "https://charm.land/crush.json" }),
    };
    {
        let obj = root
            .as_object_mut()
            .context("~/.config/crush/crush.json isn't a JSON object")?;
        let hooks = obj
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("`hooks` in crush.json isn't an object")?;
        let arr = hooks
            .entry("PreToolUse")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("`hooks.PreToolUse` in crush.json isn't an array")?;
        arr.push(serde_json::json!({
            "matcher": "^(edit|write|multiedit)$",
            "command": "sidekick hook",
            "timeout": 10,
        }));
    }
    let mut s = serde_json::to_string_pretty(&root)?;
    s.push('\n');
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::settings_after;

    #[test]
    fn merges_hook_into_empty_config() {
        let out = settings_after(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["$schema"], "https://charm.land/crush.json");
        let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["matcher"], "^(edit|write|multiedit)$");
        assert_eq!(arr[0]["command"], "sidekick hook");
        assert_eq!(arr[0]["timeout"], 10);
    }

    #[test]
    fn keeps_existing_hooks_and_keys() {
        let before = r#"{
  "models": { "default": "claude" },
  "hooks": {
    "PreToolUse": [
      { "matcher": "^bash$", "command": "./hooks/safety.sh" }
    ]
  }
}"#;
        let out = settings_after(Some(before)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["models"]["default"], "claude");
        let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["matcher"], "^bash$");
        assert_eq!(arr[0]["command"], "./hooks/safety.sh");
        assert_eq!(arr[1]["command"], "sidekick hook");
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(settings_after(Some("{ not json")).is_err());
    }
}
