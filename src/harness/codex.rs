//! Codex harness — native, plugin-bundled hooks plus a versioned cache
//! plus a trust-hash gate.
//!
//! Codex silently drops untrusted plugin hooks at discovery time, so the
//! fix has to stamp the `trusted_hash` Codex's interactive TUI would
//! write. Hashes are derived from a stable canonicalisation of our
//! `hooks.json` entries — see `crate::codex_trust`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, table, value};

use crate::fix::{FileFix, Fix};
use crate::harness::{Harness, HarnessCapabilities, HarnessError, HarnessOk};

pub(crate) const PLUGIN_SRC: &str = include_str!("../../plugins/codex/.codex-plugin/plugin.json");
pub(crate) const HOOKS_SRC: &str = include_str!("../../plugins/codex/hooks.json");

pub struct Codex;

impl Harness for Codex {
    fn name(&self) -> &'static str {
        "Codex"
    }

    fn accent(&self) -> &'static str {
        "38;2;16;185;129" // #10B981
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            edit_deny: true,
            buffer_refresh: true,
            selection_inject: true,
        }
    }

    fn init_description(&self) -> &'static str {
        "Lets Codex respect your unsaved Neovim buffers before it writes."
    }

    fn is_installed(&self) -> bool {
        crate::harness::shared::binary_on_path("codex") || codex_home().is_dir()
    }

    fn check(&self) -> Result<HarnessOk, HarnessError> {
        let direct_hooks = hook_files();
        if !direct_hooks.is_empty() {
            return Ok(HarnessOk {
                label: "hook registered".into(),
                paths: direct_hooks,
            });
        }

        if plugin_is_configured() {
            return Ok(HarnessOk {
                label: "plugin installed".into(),
                paths: plugin_detail_files(),
            });
        }

        let mut remedy = Vec::new();
        if !plugin_hooks_enabled() {
            remedy.push("Enable Codex plugin hooks in ~/.codex/config.toml".into());
        }
        if !plugin_enabled() {
            remedy.push("Enable the Sidekick Codex plugin as `sidekick@personal`".into());
        }
        if !plugin_files_current() {
            remedy.push("Install the Sidekick Codex plugin into your personal marketplace".into());
        }

        Err(HarnessError::NotConfigured {
            label: "plugin not installed".into(),
            remedy,
        })
    }

    fn fix(&self) -> Vec<Fix> {
        if !self.is_installed() || hook_is_configured() {
            return Vec::new();
        }
        fixes_for(dirs::home_dir().as_deref(), &codex_home())
    }
}

pub(crate) fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn codex_config_file() -> PathBuf {
    codex_home().join("config.toml")
}

fn hook_files() -> Vec<PathBuf> {
    let mut matched = Vec::new();

    for path in [
        codex_config_file(),
        codex_home().join("hooks.json"),
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".codex")
            .join("hooks.json"),
    ] {
        if crate::harness::shared::file_mentions_sidekick_hook(&path) {
            matched.push(path);
        }
    }

    matched.sort();
    matched.dedup();
    matched
}

fn hook_is_configured() -> bool {
    !hook_files().is_empty() || plugin_is_configured()
}

fn plugin_detail_files() -> Vec<PathBuf> {
    let mut matched = Vec::new();

    if let Some(home) = dirs::home_dir() {
        for path in [
            home.join("plugins")
                .join("sidekick")
                .join(".codex-plugin")
                .join("plugin.json"),
            home.join("plugins").join("sidekick").join("hooks.json"),
            home.join(".agents")
                .join("plugins")
                .join("marketplace.json"),
        ] {
            if path.is_file() {
                matched.push(path);
            }
        }
    }

    let config = codex_config_file();
    if config.is_file() {
        matched.push(config);
    }

    matched
}

fn plugin_is_configured() -> bool {
    plugin_enabled() && plugin_hooks_enabled() && plugin_files_current() && plugin_hooks_trusted()
}

fn plugin_hooks_trusted() -> bool {
    let Ok(config) = std::fs::read_to_string(codex_config_file()) else {
        return false;
    };
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    let Some(state) = doc.get("hooks").and_then(|hooks| hooks.get("state")) else {
        return false;
    };
    crate::codex_trust::expected_trust_entries()
        .iter()
        .all(|entry| {
            state
                .get(&entry.key)
                .and_then(|item| item.get("trusted_hash"))
                .and_then(|v| v.as_str())
                == Some(entry.trusted_hash.as_str())
        })
}

fn plugin_enabled() -> bool {
    let Ok(config) = std::fs::read_to_string(codex_config_file()) else {
        return false;
    };
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("plugins")
        .and_then(|p| p.get("sidekick@personal"))
        .and_then(|p| p.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn plugin_hooks_enabled() -> bool {
    let Ok(config) = std::fs::read_to_string(codex_config_file()) else {
        return false;
    };
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get("features")
        .and_then(|p| p.get("plugin_hooks"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn plugin_files_current() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let source_root = home.join("plugins").join("sidekick");
    let cache_root = codex_home()
        .join("plugins")
        .join("cache")
        .join("personal")
        .join("sidekick")
        .join(env!("CARGO_PKG_VERSION"));

    plugin_root_current(&source_root) && plugin_root_current(&cache_root)
}

fn plugin_root_current(root: &Path) -> bool {
    std::fs::read_to_string(root.join(".codex-plugin").join("plugin.json"))
        .ok()
        .as_deref()
        == Some(PLUGIN_SRC)
        && std::fs::read_to_string(root.join("hooks.json"))
            .ok()
            .as_deref()
            == Some(HOOKS_SRC)
}

pub(super) fn fixes_for(home: Option<&Path>, codex_home: &Path) -> Vec<Fix> {
    let mut files = Vec::new();

    if let Some(home) = home {
        // Codex personal marketplace entries use `./plugins/<name>` to mean
        // `~/plugins/<name>`, not a path relative to ~/.agents/plugins.
        let source_root = home.join("plugins").join("sidekick");
        push_file_fix(
            &mut files,
            source_root.join(".codex-plugin").join("plugin.json"),
            PLUGIN_SRC,
        );
        push_file_fix(&mut files, source_root.join("hooks.json"), HOOKS_SRC);

        let marketplace_path = home
            .join(".agents")
            .join("plugins")
            .join("marketplace.json");
        let before = std::fs::read_to_string(&marketplace_path).ok();
        if let Ok(after) = marketplace_after(before.as_deref())
            && before.as_deref() != Some(after.as_str())
        {
            files.push(FileFix {
                path: marketplace_path,
                before,
                after,
            });
        }
    }

    let cache_root = codex_home
        .join("plugins")
        .join("cache")
        .join("personal")
        .join("sidekick")
        .join(env!("CARGO_PKG_VERSION"));
    push_file_fix(
        &mut files,
        cache_root.join(".codex-plugin").join("plugin.json"),
        PLUGIN_SRC,
    );
    push_file_fix(&mut files, cache_root.join("hooks.json"), HOOKS_SRC);

    let config_path = codex_home.join("config.toml");
    let before = std::fs::read_to_string(&config_path).ok();
    if let Ok(after) = config_after(before.as_deref())
        && before.as_deref() != Some(after.as_str())
    {
        files.push(FileFix {
            path: config_path,
            before,
            after,
        });
    }

    if files.is_empty() {
        Vec::new()
    } else {
        vec![Fix {
            title: "Install and enable the Codex plugin".into(),
            files,
        }]
    }
}

fn push_file_fix(files: &mut Vec<FileFix>, path: PathBuf, after: &str) {
    let before = std::fs::read_to_string(&path).ok();
    if before.as_deref() == Some(after) {
        return;
    }
    files.push(FileFix {
        path,
        before,
        after: after.to_string(),
    });
}

pub(super) fn config_after(before: Option<&str>) -> Result<String> {
    let mut doc = match before {
        Some(s) if !s.trim().is_empty() => s
            .parse::<DocumentMut>()
            .context("~/.codex/config.toml isn't valid TOML")?,
        _ => DocumentMut::new(),
    };

    doc["features"].or_insert(table());
    doc["features"]["plugin_hooks"] = value(true);
    doc["plugins"].or_insert(table());
    doc["plugins"]["sidekick@personal"].or_insert(table());
    doc["plugins"]["sidekick@personal"]["enabled"] = value(true);

    doc["hooks"].or_insert(table());
    doc["hooks"]["state"].or_insert(table());
    let state = doc["hooks"]["state"]
        .as_table_mut()
        .context("`hooks.state` in ~/.codex/config.toml isn't a table")?;
    for entry in crate::codex_trust::expected_trust_entries() {
        state.entry(&entry.key).or_insert(table());
        let item = state
            .get_mut(&entry.key)
            .and_then(|item| item.as_table_mut())
            .with_context(|| {
                format!(
                    "`hooks.state.\"{}\"` in ~/.codex/config.toml isn't a table",
                    entry.key
                )
            })?;
        item["trusted_hash"] = value(entry.trusted_hash);
    }

    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

pub(super) fn marketplace_after(before: Option<&str>) -> Result<String> {
    let mut root: serde_json::Value = match before {
        Some(s) if !s.trim().is_empty() => serde_json::from_str(s)
            .context("~/.agents/plugins/marketplace.json isn't valid JSON")?,
        _ => serde_json::json!({
            "name": "personal",
            "interface": { "displayName": "Personal" },
            "plugins": []
        }),
    };

    let obj = root
        .as_object_mut()
        .context("~/.agents/plugins/marketplace.json isn't a JSON object")?;
    obj.entry("name")
        .or_insert_with(|| serde_json::json!("personal"));
    obj.entry("interface").or_insert_with(|| {
        serde_json::json!({
            "displayName": "Personal"
        })
    });
    let plugins = obj
        .entry("plugins")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .context("`plugins` in marketplace.json isn't an array")?;

    let sidekick_entry = serde_json::json!({
        "name": "sidekick",
        "source": {
            "source": "local",
            "path": "./plugins/sidekick"
        },
        "policy": {
            "installation": "AVAILABLE",
            "authentication": "ON_INSTALL"
        },
        "category": "Productivity"
    });

    if let Some(existing) = plugins
        .iter_mut()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("sidekick"))
    {
        *existing = sidekick_entry;
    } else {
        plugins.push(sidekick_entry);
    }

    let mut s = serde_json::to_string_pretty(&root)?;
    s.push('\n');
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::{config_after, fixes_for, marketplace_after};

    #[test]
    fn enables_plugin_hooks_in_empty_config() {
        let out = config_after(None).unwrap();

        assert!(out.contains("[features]"));
        assert!(out.contains("plugin_hooks = true"));
        assert!(out.contains("[plugins.\"sidekick@personal\"]"));
        assert!(out.contains("enabled = true"));

        for entry in crate::codex_trust::expected_trust_entries() {
            assert!(
                out.contains(&format!("[hooks.state.\"{}\"]", entry.key)),
                "missing [hooks.state.\"{}\"] in:\n{out}",
                entry.key,
            );
            assert!(
                out.contains(&format!("trusted_hash = \"{}\"", entry.trusted_hash)),
                "missing trusted_hash for {} in:\n{out}",
                entry.key,
            );
        }
    }

    #[test]
    fn preserves_existing_config() {
        let before = r#"model = "gpt-5.5"

[features]
other = true
"#;
        let out = config_after(Some(before)).unwrap();

        assert!(out.contains("model = \"gpt-5.5\""));
        assert!(out.contains("other = true"));
        assert!(out.contains("plugin_hooks = true"));
    }

    #[test]
    fn merges_personal_marketplace_entry() {
        let before = r#"{"name":"personal","interface":{"displayName":"Mine"},"plugins":[]}"#;
        let out = marketplace_after(Some(before)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();

        assert_eq!(v["interface"]["displayName"], "Mine");
        assert_eq!(v["plugins"][0]["name"], "sidekick");
        assert_eq!(v["plugins"][0]["source"]["path"], "./plugins/sidekick");
    }

    #[test]
    fn fixes_plans_personal_plugin_paths() {
        let root =
            std::env::temp_dir().join(format!("sidekick-codex-fixes-{}", std::process::id()));
        let home = root.join("home");
        let codex_home = root.join("codex");

        let fixes = fixes_for(Some(&home), &codex_home);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].title, "Install and enable the Codex plugin");
        let paths = fixes[0]
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>();

        assert!(paths.contains(&home.join("plugins/sidekick/.codex-plugin/plugin.json")));
        assert!(paths.contains(&home.join("plugins/sidekick/hooks.json")));
        assert!(paths.contains(&home.join(".agents/plugins/marketplace.json")));
        assert!(paths.contains(&codex_home.join("config.toml")));
        assert!(paths.contains(&codex_home.join(format!(
            "plugins/cache/personal/sidekick/{}/.codex-plugin/plugin.json",
            env!("CARGO_PKG_VERSION")
        ))));

        let _ = std::fs::remove_dir_all(root);
    }
}
