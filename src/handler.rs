//! Hook processing logic.
//!
//! Reads a Claude/Codex-shaped JSON envelope from stdin, decides allow/deny,
//! and writes the response JSON to stdout. The same protocol is also driven by
//! the bridges under `plugins/` — the opencode plugin and the pi extension —
//! which translate their host's tool and prompt events into this envelope.
//!
//! # Hook Flow
//!
//! 1. PreToolUse: Check if file has unsaved changes before the AI modifies it
//!    - If file is current buffer with unsaved changes → Deny
//!    - Otherwise → Allow
//!
//! 2. PostToolUse: Refresh buffer after the AI modifies it
//!    - Reload buffer from disk across all Neovim instances
//!    - Preserve cursor positions
//!
//! 3. UserPromptSubmit: Inject visual selection as additional context
//!    - If Neovim has a visual selection → inject as additionalContext
//!    - Otherwise → no-op
//!
//! # Example
//!
//! ```no_run
//! use sidekick::handler;
//!
//! // Called by Claude Code, Codex, or the opencode/pi bridges via stdin/stdout
//! handler::handle_hook().expect("Failed to process hook");
//! ```

use std::io::{self, Read, Write};
use std::time::Instant;

use chrono::Utc;

use crate::action::{Action, neovim::NeovimAction};
use crate::analytics::{
    self,
    event::{BufferRefresh, Decision, DecisionReason, Event, HookDecision, ToolKind},
};
use crate::hook::{
    self, FileToolInput, Hook, HookEvent, HookOutput, PermissionDecision, Tool, ToolHook,
};
use crate::utils;

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;

    // Resolve nvim instances once so we know how many we probed.
    let socket_paths = utils::find_matching_sockets().unwrap_or_default();
    let instances_probed = socket_paths.len();
    let nvim_action = if socket_paths.is_empty() {
        None
    } else {
        Some(NeovimAction::new(socket_paths))
    };

    // Handle based on hook type
    let output = match hook {
        Hook::Tool(h) => match h.hook_event_name {
            HookEvent::PreToolUse => {
                handle_pre_tool_use(&h, nvim_action.as_ref(), instances_probed)
            }
            HookEvent::PostToolUse => handle_post_tool_use(&h, nvim_action.as_ref()),
        },
        Hook::UserPrompt => handle_user_prompt_submit(nvim_action.as_ref()),
    };

    // Return hook output
    io::stdout().write_all(output.to_json()?.as_bytes())?;

    Ok(())
}

/// Handle PreToolUse hook - check if file has unsaved changes
fn handle_pre_tool_use(
    h: &ToolHook,
    nvim_action: Option<&NeovimAction>,
    instances_probed: usize,
) -> HookOutput {
    let mutations = tool_to_mutations(&h.tool);
    if mutations.is_empty() {
        return HookOutput::new();
    };

    for (tool_kind, file_path) in mutations {
        let started = Instant::now();
        let (output, reason) = check_buffer_modifications(nvim_action, &file_path);
        let decision = match reason {
            DecisionReason::BufferDirtyAndCurrent => Decision::Deny,
            _ => Decision::Allow,
        };

        analytics::store::append(&Event::HookDecision(HookDecision {
            at: Utc::now(),
            session_id: h.session_id.clone(),
            cwd: h.cwd.clone(),
            tool: tool_kind,
            file: file_path.clone(),
            decision,
            reason,
            instances_probed,
            latency_ms: started.elapsed().as_millis() as u64,
        }));

        if matches!(decision, Decision::Deny) {
            return output;
        }
    }

    HookOutput::new()
}

/// Handle PostToolUse hook - refresh buffers after modifications
fn handle_post_tool_use(h: &ToolHook, nvim_action: Option<&NeovimAction>) -> HookOutput {
    let mutations = tool_to_mutations(&h.tool);
    if mutations.is_empty() {
        return HookOutput::new();
    };

    for (tool_kind, file_path) in mutations {
        refresh_buffer(nvim_action, &file_path);

        // Only count refreshes when nvim was reachable — otherwise nothing happened
        // and recording the event would inflate the activity charts.
        if nvim_action.is_some() {
            analytics::store::append(&Event::BufferRefresh(BufferRefresh {
                at: Utc::now(),
                session_id: h.session_id.clone(),
                cwd: h.cwd.clone(),
                tool: tool_kind,
                file: file_path,
            }));
        }
    }

    HookOutput::new()
}

/// Handle UserPromptSubmit hook - inject visual selections as context
fn handle_user_prompt_submit(nvim_action: Option<&NeovimAction>) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    let Ok(selections) = action.get_visual_selections() else {
        return HookOutput::new();
    };

    if selections.is_empty() {
        return HookOutput::new();
    }

    let context = selections
        .iter()
        .map(|ctx| {
            format!(
                "[Selected from {}:{}-{}]\n```\n{}\n```",
                ctx.file_path, ctx.start_line, ctx.end_line, ctx.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    HookOutput::new().with_additional_context(context)
}

/// Check if buffer has unsaved modifications and block if necessary.
/// Returns the hook response alongside a `DecisionReason` for analytics.
fn check_buffer_modifications(
    nvim_action: Option<&NeovimAction>,
    file_path: &str,
) -> (HookOutput, DecisionReason) {
    let Some(action) = nvim_action else {
        return (HookOutput::new(), DecisionReason::NoNvimRunning);
    };

    let Ok(status) = action.buffer_status(file_path) else {
        return (HookOutput::new(), DecisionReason::StatusCheckFailed);
    };

    if status.has_unsaved_changes && status.is_current {
        if let Err(e) = action.send_message("Edit blocked — file has unsaved changes") {
            eprintln!("Warning: {}", e);
        }

        let output = HookOutput::new().with_permission_decision(
            PermissionDecision::Deny,
            Some("The file is being edited by the user, try again later".to_string()),
        );
        (output, DecisionReason::BufferDirtyAndCurrent)
    } else {
        (HookOutput::new(), DecisionReason::BufferAvailable)
    }
}

/// Refresh buffer after file modification
fn refresh_buffer(nvim_action: Option<&NeovimAction>, file_path: &str) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    if let Err(e) = action.refresh_buffer(file_path) {
        eprintln!("Warning: {}", e);
    }

    HookOutput::new()
}

fn tool_to_mutations(tool: &Tool) -> Vec<(ToolKind, String)> {
    match tool {
        Tool::Edit(f) => file_input_mutation(ToolKind::Edit, f),
        Tool::Write(f) => file_input_mutation(ToolKind::Write, f),
        Tool::MultiEdit(f) => file_input_mutation(ToolKind::MultiEdit, f),
        Tool::Other { name, input } => other_tool_mutations(name, input),
        _ => Vec::new(),
    }
}

fn file_input_mutation(tool_kind: ToolKind, input: &FileToolInput) -> Vec<(ToolKind, String)> {
    vec![(tool_kind, input.file_path.clone())]
}

fn other_tool_mutations(name: &str, input: &serde_json::Value) -> Vec<(ToolKind, String)> {
    let normalized = name.to_ascii_lowercase();

    if normalized == "apply_patch" {
        return dedupe_paths(
            collect_patch_paths(input)
                .into_iter()
                .map(|p| (ToolKind::Edit, p)),
        );
    }

    let tool_kind = match normalized.as_str() {
        "edit" => ToolKind::Edit,
        "write" => ToolKind::Write,
        "multiedit" | "multi_edit" => ToolKind::MultiEdit,
        _ => return Vec::new(),
    };

    if let Some(path) = pick_file_path(input) {
        return vec![(tool_kind, path.to_string())];
    }

    Vec::new()
}

fn pick_file_path(input: &serde_json::Value) -> Option<&str> {
    input.as_object().and_then(|obj| {
        ["file_path", "filePath", "path"]
            .into_iter()
            .find_map(|key| obj.get(key).and_then(|v| v.as_str()))
    })
}

fn collect_patch_paths(input: &serde_json::Value) -> Vec<String> {
    let mut paths = Vec::new();
    collect_patch_paths_inner(input, &mut paths);
    dedupe_paths(paths.into_iter().map(|p| (ToolKind::Edit, p)))
        .into_iter()
        .map(|(_, p)| p)
        .collect()
}

fn collect_patch_paths_inner(input: &serde_json::Value, paths: &mut Vec<String>) {
    match input {
        serde_json::Value::String(s) => paths.extend(parse_patch_paths(s)),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_patch_paths_inner(value, paths);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_patch_paths_inner(value, paths);
            }
        }
        _ => {}
    }
}

fn parse_patch_paths(patch: &str) -> Vec<String> {
    const PATCH_FILE_MARKERS: &[&str] = &[
        "*** Add File:",
        "*** Update File:",
        "*** Delete File:",
        "*** Move to:",
    ];

    let mut paths = Vec::new();
    for line in patch.lines() {
        let trimmed = line.trim_start();
        for marker in PATCH_FILE_MARKERS {
            if let Some(rest) = trimmed.strip_prefix(marker) {
                let path = rest.trim();
                if !path.is_empty() {
                    paths.push(path.to_string());
                }
                break;
            }
        }
    }
    paths
}

fn dedupe_paths(
    mutations: impl IntoIterator<Item = (ToolKind, String)>,
) -> Vec<(ToolKind, String)> {
    let mut deduped = Vec::new();
    for mutation in mutations {
        if !deduped.iter().any(|existing| existing == &mutation) {
            deduped.push(mutation);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::tool_to_mutations;
    use crate::analytics::event::ToolKind;
    use crate::hook::Tool;

    #[test]
    fn extracts_codex_apply_patch_paths() {
        let tool = Tool::Other {
            name: "apply_patch".into(),
            input: serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n*** Add File: src/new.rs\n*** Delete File: src/old.rs\n*** Move to: src/moved.rs\n*** End Patch\n"
            }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![
                (ToolKind::Edit, "src/lib.rs".into()),
                (ToolKind::Edit, "src/new.rs".into()),
                (ToolKind::Edit, "src/old.rs".into()),
                (ToolKind::Edit, "src/moved.rs".into()),
            ]
        );
    }

    #[test]
    fn extracts_lowercase_tool_path_variants() {
        let tool = Tool::Other {
            name: "write".into(),
            input: serde_json::json!({ "path": "README.md" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Write, "README.md".into())]
        );
    }

    #[test]
    fn extracts_crush_edit_file_path() {
        let tool = Tool::Other {
            name: "edit".into(),
            input: serde_json::json!({ "file_path": "src/foo.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Edit, "src/foo.rs".into())]
        );
    }

    #[test]
    fn extracts_crush_write_file_path() {
        let tool = Tool::Other {
            name: "write".into(),
            input: serde_json::json!({ "file_path": "README.md" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Write, "README.md".into())]
        );
    }

    #[test]
    fn extracts_crush_multiedit_file_path() {
        let tool = Tool::Other {
            name: "multiedit".into(),
            input: serde_json::json!({
                "file_path": "config.toml",
                "edits": [{ "old_string": "a", "new_string": "b" }]
            }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::MultiEdit, "config.toml".into())]
        );
    }

    #[test]
    fn ignores_apply_patch_without_file_markers() {
        let tool = Tool::Other {
            name: "apply_patch".into(),
            input: serde_json::json!({
                "patch": "*** Begin Patch\n@@\n-no file header\n+still no file header\n*** End Patch\n"
            }),
        };

        assert!(tool_to_mutations(&tool).is_empty());
    }
}
