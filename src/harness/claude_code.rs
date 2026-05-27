//! Claude Code harness — native, plain-config integration.
//!
//! Claude Code emits the sidekick wire protocol directly from its hook
//! config in `~/.claude/settings.json`. Install is a JSON merge that
//! preserves the user's other keys.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::fix::Fix;
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub struct ClaudeCode;

impl Harness for ClaudeCode {
    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn accent(&self) -> &'static str {
        "38;2;217;119;87" // #D97757
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: true,
            selection_inject: true,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets Claude Code check your unsaved Neovim buffers before it edits."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("claude")
            || dirs::home_dir()
                .map(|h| h.join(".claude").is_dir())
                .unwrap_or(false)
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let paths = hook_files();
        if paths.is_empty() {
            Err(HarnessError::NotConfigured {
                label: "hook not registered".into(),
                remedy: vec![
                    "Run `sidekick doctor --fix` to register the hooks in ~/.claude/settings.json"
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
        let path = home.join(".claude").join("settings.json");
        let before = std::fs::read_to_string(&path).ok();
        let Ok(after) = settings_after(before.as_deref()) else {
            return Vec::new();
        };
        vec![Fix::single(
            "Register the Claude Code hooks",
            path,
            before,
            after,
        )]
    }
}

/// Config files that already wire up `sidekick hook` for Claude Code.
fn hook_files() -> Vec<PathBuf> {
    let mut matched: Vec<PathBuf> = Vec::new();

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".claude").join("settings.json"));
        candidates.push(home.join(".claude").join("settings.local.json"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".claude").join("settings.json"));
    }

    for path in &candidates {
        if crate::harness::shared::file_mentions_sidekick_hook(path) {
            matched.push(path.clone());
        }
    }

    if let Some(home) = dirs::home_dir() {
        walk_for_json_mentioning_hook(&home.join(".claude").join("plugins"), &mut matched, 4);
    }

    matched.sort();
    matched.dedup();
    matched
}

fn walk_for_json_mentioning_hook(dir: &Path, matched: &mut Vec<PathBuf>, depth: usize) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_for_json_mentioning_hook(&path, matched, depth - 1);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json")
            && crate::harness::shared::file_mentions_sidekick_hook(&path)
        {
            matched.push(path);
        }
    }
}

/// Merge sidekick's three hooks into `settings.json`, leaving every other
/// key — and the user's key order — untouched.
pub(super) fn settings_after(before: Option<&str>) -> Result<String> {
    let mut root: serde_json::Value = match before {
        Some(s) if !s.trim().is_empty() => {
            serde_json::from_str(s).context("~/.claude/settings.json isn't valid JSON")?
        }
        _ => serde_json::json!({}),
    };
    {
        let obj = root
            .as_object_mut()
            .context("~/.claude/settings.json isn't a JSON object")?;
        let hooks = obj
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("`hooks` in settings.json isn't an object")?;
        for (event, matcher) in [
            ("PreToolUse", "Edit|Write|MultiEdit"),
            ("PostToolUse", "Edit|Write|MultiEdit"),
            ("UserPromptSubmit", ""),
        ] {
            let arr = hooks
                .entry(event)
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .with_context(|| format!("`hooks.{event}` in settings.json isn't an array"))?;
            arr.push(serde_json::json!({
                "matcher": matcher,
                "hooks": [{ "type": "command", "command": "sidekick hook" }],
            }));
        }
    }
    let mut s = serde_json::to_string_pretty(&root)?;
    s.push('\n');
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::settings_after;

    #[test]
    fn merges_three_hooks_into_empty_settings() {
        let out = settings_after(None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let hooks = &v["hooks"];
        for event in ["PreToolUse", "PostToolUse", "UserPromptSubmit"] {
            let arr = hooks[event].as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0]["hooks"][0]["command"], "sidekick hook");
        }
        assert_eq!(hooks["PreToolUse"][0]["matcher"], "Edit|Write|MultiEdit");
        assert_eq!(hooks["UserPromptSubmit"][0]["matcher"], "");
    }

    #[test]
    fn keeps_existing_keys_order_and_hooks() {
        let before = r#"{"model":"opus","hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[]}]}}"#;
        let out = settings_after(Some(before)).unwrap();
        assert!(out.find("\"model\"").unwrap() < out.find("\"hooks\"").unwrap());

        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "opus");
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["matcher"], "Bash");
        assert_eq!(pre[1]["hooks"][0]["command"], "sidekick hook");
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(settings_after(Some("{ not json")).is_err());
    }
}
