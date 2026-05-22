//! Codex hook-trust hash computation.
//!
//! Codex stores hook trust in `~/.codex/config.toml` as
//! `[hooks.state."<key>"] trusted_hash = "sha256:<hex>"`. Without a matching
//! entry, plugin hooks are silently dropped at handler discovery
//! (codex-rs/hooks/src/engine/discovery.rs). We mirror Codex's hash algorithm
//! so `sidekick doctor --fix` can stamp the right entries without prompting
//! the user through Codex's interactive trust UI.
//!
//! The wire format is locked by Codex; if it ever changes, the tests at the
//! bottom will fail and the trust entries we write will look "Modified" to
//! Codex (which is functionally identical to Untrusted — fail-closed).

use serde::Serialize;
use sha2::{Digest, Sha256};

/// One hook entry sidekick's Codex plugin registers.
pub(crate) struct PluginHook {
    pub(crate) event: HookEvent,
    /// `None` for events Codex ignores matchers on (UserPromptSubmit, Stop).
    pub(crate) matcher: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub(crate) enum HookEvent {
    PreToolUse,
    PostToolUse,
    UserPromptSubmit,
}

impl HookEvent {
    /// Matches `codex_hooks::hook_event_key_label`.
    fn key_label(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "pre_tool_use",
            HookEvent::PostToolUse => "post_tool_use",
            HookEvent::UserPromptSubmit => "user_prompt_submit",
        }
    }
}

/// The three hooks our Codex plugin registers — kept in lock-step with
/// `plugins/codex/hooks.json`.
pub(crate) const PLUGIN_HOOKS: &[PluginHook] = &[
    PluginHook {
        event: HookEvent::PreToolUse,
        matcher: Some("Edit|Write|MultiEdit|apply_patch|edit|write|multi_edit"),
    },
    PluginHook {
        event: HookEvent::PostToolUse,
        matcher: Some("Edit|Write|MultiEdit|apply_patch|edit|write|multi_edit"),
    },
    PluginHook {
        event: HookEvent::UserPromptSubmit,
        matcher: None,
    },
];

/// The hook command our plugin runs. Must match `plugins/codex/hooks.json`.
const HOOK_COMMAND: &str = "sidekick hook";

/// Codex's `timeout_sec.unwrap_or(600).max(1)` default when hooks.json omits
/// the field, as we do.
const DEFAULT_TIMEOUT_SEC: u64 = 600;

/// Codex's plugin key prefix — `plugin_id:source_relative_path`. The path is
/// resolved through `paths.hooks` in plugin.json, which we set to
/// `./hooks.json`.
const PLUGIN_KEY_PREFIX: &str = "sidekick@personal:hooks.json";

pub(crate) struct TrustEntry {
    /// The TOML key Codex looks up under `[hooks.state]`.
    pub(crate) key: String,
    /// The `sha256:<hex>` value Codex compares against.
    pub(crate) trusted_hash: String,
}

/// Build the trust entries sidekick should write to `~/.codex/config.toml`.
///
/// Codex's hook key format is `{plugin_id}:{relative_path}:{event_label}:{group_index}:{handler_index}`
/// — and our hooks.json has exactly one matcher group with one handler per event,
/// so both indices are always 0.
pub(crate) fn expected_trust_entries() -> Vec<TrustEntry> {
    PLUGIN_HOOKS
        .iter()
        .map(|hook| TrustEntry {
            key: format!("{PLUGIN_KEY_PREFIX}:{}:0:0", hook.event.key_label()),
            trusted_hash: compute_hash(hook.event, hook.matcher),
        })
        .collect()
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum HookHandlerConfig {
    #[serde(rename = "command")]
    Command {
        command: &'static str,
        #[serde(rename = "commandWindows", skip_serializing_if = "Option::is_none")]
        command_windows: Option<&'static str>,
        #[serde(rename = "timeout", skip_serializing_if = "Option::is_none")]
        timeout_sec: Option<u64>,
        r#async: bool,
        #[serde(rename = "statusMessage", skip_serializing_if = "Option::is_none")]
        status_message: Option<&'static str>,
    },
}

#[derive(Serialize)]
struct MatcherGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    matcher: Option<&'static str>,
    hooks: Vec<HookHandlerConfig>,
}

#[derive(Serialize)]
struct NormalizedHookIdentity {
    event_name: &'static str,
    #[serde(flatten)]
    group: MatcherGroup,
}

fn compute_hash(event: HookEvent, matcher: Option<&'static str>) -> String {
    let identity = NormalizedHookIdentity {
        event_name: event.key_label(),
        group: MatcherGroup {
            matcher,
            hooks: vec![HookHandlerConfig::Command {
                command: HOOK_COMMAND,
                command_windows: None,
                timeout_sec: Some(DEFAULT_TIMEOUT_SEC),
                r#async: false,
                status_message: None,
            }],
        },
    };
    let json = serde_json::to_value(&identity).expect("normalized identity serializes");
    let canonical = canonical_json(&json);
    let serialized = serde_json::to_vec(&canonical).expect("canonical JSON serializes");
    let mut hasher = Sha256::new();
    hasher.update(&serialized);
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

fn canonical_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            for k in keys {
                if let Some(v) = map.get(&k) {
                    sorted.insert(k, canonical_json(v));
                }
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_json).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden hashes verified against Codex v0.133.0 by injecting them into a
    /// live `~/.codex/config.toml` and observing the hook fire (no
    /// `--bypass-hook-trust` flag). If Codex changes its hash format, these
    /// values will drift and the doctor check will flag the mismatch.
    #[test]
    fn trust_entries_match_codex_v0_133() {
        let entries = expected_trust_entries();
        assert_eq!(entries.len(), 3);

        assert_eq!(
            entries[0].key,
            "sidekick@personal:hooks.json:pre_tool_use:0:0"
        );
        assert_eq!(
            entries[0].trusted_hash,
            "sha256:29b7969ecbfc437bd9cace43bebee7c1f8c8326b5281b661be15b48e16179d35"
        );

        assert_eq!(
            entries[1].key,
            "sidekick@personal:hooks.json:post_tool_use:0:0"
        );
        assert_eq!(
            entries[1].trusted_hash,
            "sha256:4d17a12f16e242552cc3a752319accab989cd921f7d9bb30e20dd5fc5fccee08"
        );

        assert_eq!(
            entries[2].key,
            "sidekick@personal:hooks.json:user_prompt_submit:0:0"
        );
        assert_eq!(
            entries[2].trusted_hash,
            "sha256:273d8f0f9cac71688dd7a0c3fd472fcfd010dfbb17758a4eb43b8b0777cff6ea"
        );
    }
}
