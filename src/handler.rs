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
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use chrono::Utc;

use crate::analytics::{
    self,
    event::{BufferRefresh, Decision, DecisionReason, Event, HookDecision, ToolKind},
};
use crate::editor::{Editor, neovim::NeovimEditor};
use crate::hook::{
    self, AntigravityShape, AntigravityToolHook, ClaudeShape, CrushShape, FileToolInput, Hook,
    HookOutput, KnownTool, PermissionDecision, Tool,
};
use crate::utils;

/// Which output envelope the host expects on stdout. Claude-shape hosts
/// (Claude Code, Codex, opencode, pi, Crush, Amp) read
/// `hookSpecificOutput.permissionDecision`; Antigravity reads a top-level
/// `decision` field instead. Emitting the wrong shape means the host
/// silently treats every gating call as allow.
#[derive(Clone, Copy)]
enum WireFormat {
    Claude,
    Antigravity,
}

pub fn handle_hook() -> anyhow::Result<()> {
    // Read hook input from stdin
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Parse the hook
    let hook = hook::parse_hook(&input)?;
    let wire = match &hook {
        Hook::Antigravity(_) => WireFormat::Antigravity,
        _ => WireFormat::Claude,
    };

    // Resolve nvim instances. Start with the cwd-based search every
    // harness has relied on; fall back to walking up from the file path
    // in the envelope for harnesses whose plugin runtime sandboxes
    // process.cwd() (Amp), so sidekick can still find the matching
    // Neovim from any cwd as long as the edited file is inside the
    // Neovim launch directory.
    let socket_paths = resolve_socket_paths(&hook);
    let instances_probed = socket_paths.len();
    let nvim_action = if socket_paths.is_empty() {
        None
    } else {
        Some(NeovimEditor::new(socket_paths))
    };

    // Handle based on hook type
    let output = match hook {
        Hook::Claude(ClaudeShape::PreToolUse(h)) => handle_pre_tool_use(
            &h.session_id,
            &h.cwd,
            &h.tool,
            nvim_action.as_ref(),
            instances_probed,
            wire,
        ),
        Hook::Claude(ClaudeShape::PostToolUse(h)) => {
            handle_post_tool_use(&h.session_id, &h.cwd, &h.tool, nvim_action.as_ref())
        }
        Hook::Claude(ClaudeShape::UserPromptSubmit(_)) => {
            handle_user_prompt_submit(nvim_action.as_ref())
        }
        Hook::Crush(CrushShape::PreToolUse(h)) => handle_pre_tool_use(
            &h.session_id,
            &h.cwd,
            &h.tool,
            nvim_action.as_ref(),
            instances_probed,
            wire,
        ),
        Hook::Antigravity(AntigravityShape::PreToolUse(h)) => {
            let cwd = h.workspace_paths.first().map(String::as_str).unwrap_or("");
            let tool = antigravity_to_tool(&h);
            handle_pre_tool_use(
                &h.conversation_id,
                cwd,
                &tool,
                nvim_action.as_ref(),
                instances_probed,
                wire,
            )
        }
        Hook::Antigravity(AntigravityShape::PostToolUse(_)) => {
            handle_antigravity_post_tool_use(nvim_action.as_ref())
        }
    };

    // Return hook output
    io::stdout().write_all(output.to_json()?.as_bytes())?;

    Ok(())
}

/// Handle PreToolUse hook - check if file has unsaved changes
fn handle_pre_tool_use(
    session_id: &str,
    cwd: &str,
    tool: &Tool,
    nvim_action: Option<&NeovimEditor>,
    instances_probed: usize,
    wire: WireFormat,
) -> HookOutput {
    let mutations = tool_to_mutations(tool);
    if mutations.is_empty() {
        return build_allow(wire);
    };

    for (tool_kind, file_path) in mutations {
        let started = Instant::now();
        let (output, reason) = check_buffer_modifications(nvim_action, &file_path, wire);
        let decision = match reason {
            DecisionReason::BufferDirtyAndCurrent => Decision::Deny,
            _ => Decision::Allow,
        };

        analytics::store::append(&Event::HookDecision(HookDecision {
            at: Utc::now(),
            session_id: session_id.to_string(),
            cwd: cwd.to_string(),
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

    build_allow(wire)
}

/// Handle PostToolUse hook - refresh buffers after modifications
fn handle_post_tool_use(
    session_id: &str,
    cwd: &str,
    tool: &Tool,
    nvim_action: Option<&NeovimEditor>,
) -> HookOutput {
    let mutations = tool_to_mutations(tool);
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
                session_id: session_id.to_string(),
                cwd: cwd.to_string(),
                tool: tool_kind,
                file: file_path,
            }));
        }
    }

    HookOutput::new()
}

/// Handle Antigravity's PostToolUse — no toolCall on the wire, just
/// `stepIdx` and optional `error`. Sidekick doesn't know which file the
/// host just wrote, so it broadcasts `:checktime` to every connected
/// Neovim. Each instance reloads buffers whose disk file changed; dirty
/// buffers stay protected by Vim's W12 prompt.
fn handle_antigravity_post_tool_use(nvim_action: Option<&NeovimEditor>) -> HookOutput {
    if let Some(action) = nvim_action
        && let Err(e) = action.check_all_buffers()
    {
        eprintln!("Warning: {}", e);
    }
    HookOutput::new()
}

/// Handle UserPromptSubmit hook - inject visual selections as context
fn handle_user_prompt_submit(nvim_action: Option<&NeovimEditor>) -> HookOutput {
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
    nvim_action: Option<&NeovimEditor>,
    file_path: &str,
    wire: WireFormat,
) -> (HookOutput, DecisionReason) {
    let Some(action) = nvim_action else {
        return (build_allow(wire), DecisionReason::NoNvimRunning);
    };

    let Ok(status) = action.buffer_status(file_path) else {
        return (build_allow(wire), DecisionReason::StatusCheckFailed);
    };

    if status.has_unsaved_changes && status.is_current {
        if let Err(e) = action.send_message("Edit blocked — file has unsaved changes") {
            eprintln!("Warning: {}", e);
        }

        let reason = "The file is being edited by the user, try again later".to_string();
        (
            build_deny(wire, reason),
            DecisionReason::BufferDirtyAndCurrent,
        )
    } else {
        (build_allow(wire), DecisionReason::BufferAvailable)
    }
}

/// Construct an allow-shaped PreToolUse output. Claude-shape hosts treat
/// an empty `{}` as allow; Antigravity requires an explicit top-level
/// `decision: "allow"`.
fn build_allow(wire: WireFormat) -> HookOutput {
    match wire {
        WireFormat::Claude => HookOutput::new(),
        WireFormat::Antigravity => HookOutput::new().with_top_level_decision("allow", None),
    }
}

/// Construct a deny-shaped PreToolUse output for the host's wire format.
/// Claude-shape hosts read `hookSpecificOutput.permissionDecision`;
/// Antigravity reads the top-level `decision` field.
fn build_deny(wire: WireFormat, reason: String) -> HookOutput {
    match wire {
        WireFormat::Claude => {
            HookOutput::new().with_permission_decision(PermissionDecision::Deny, Some(reason))
        }
        WireFormat::Antigravity => HookOutput::new().with_top_level_decision("deny", Some(reason)),
    }
}

/// Refresh buffer after file modification
fn refresh_buffer(nvim_action: Option<&NeovimEditor>, file_path: &str) -> HookOutput {
    let Some(action) = nvim_action else {
        return HookOutput::new();
    };

    if let Err(e) = action.refresh_buffer(file_path) {
        eprintln!("Warning: {}", e);
    }

    HookOutput::new()
}

/// Extract the file mutations a hook envelope would trigger, as
/// `(kind, path)` tuples with stable string labels (`"edit"`, `"write"`,
/// `"multiedit"`).
///
/// This is the behavioral contract integration tests assert against: given
/// JSON in, what files would sidekick check or refresh? Returning string
/// labels (not internal enum names) keeps the test API resilient to
/// internal type renames.
#[allow(dead_code)] // called from tests/integration.rs (separate crate)
pub fn extract_mutations(input: &str) -> anyhow::Result<Vec<(String, String)>> {
    let hook_value = hook::parse_hook(input)?;
    let owned_tool: Tool;
    let tool: &Tool = match &hook_value {
        Hook::Claude(ClaudeShape::PreToolUse(h)) => &h.tool,
        Hook::Claude(ClaudeShape::PostToolUse(h)) => &h.tool,
        Hook::Crush(CrushShape::PreToolUse(h)) => &h.tool,
        Hook::Antigravity(AntigravityShape::PreToolUse(h)) => {
            owned_tool = antigravity_to_tool(h);
            &owned_tool
        }
        Hook::Claude(ClaudeShape::UserPromptSubmit(_))
        | Hook::Antigravity(AntigravityShape::PostToolUse(_)) => return Ok(Vec::new()),
    };
    Ok(tool_to_mutations(tool)
        .into_iter()
        .map(|(kind, path)| (tool_kind_label(kind).to_string(), path))
        .collect())
}

/// Classify the event in a hook envelope as one of `"PreToolUse"`,
/// `"PostToolUse"`, or `"UserPromptSubmit"`. Wrong event routing means the
/// wrong code path in the handler, so this is the second behavioral
/// invariant tests assert against.
#[allow(dead_code)] // called from tests/integration.rs (separate crate)
pub fn classify_event(input: &str) -> anyhow::Result<&'static str> {
    let hook_value = hook::parse_hook(input)?;
    Ok(match hook_value {
        Hook::Claude(ClaudeShape::PreToolUse(_)) => "PreToolUse",
        Hook::Claude(ClaudeShape::PostToolUse(_)) => "PostToolUse",
        Hook::Claude(ClaudeShape::UserPromptSubmit(_)) => "UserPromptSubmit",
        Hook::Crush(CrushShape::PreToolUse(_)) => "PreToolUse",
        // Antigravity has no event-name tag on the wire — the structural
        // discriminator on AntigravityShape (presence of toolCall) maps
        // to Pre vs Post.
        Hook::Antigravity(AntigravityShape::PreToolUse(_)) => "PreToolUse",
        Hook::Antigravity(AntigravityShape::PostToolUse(_)) => "PostToolUse",
    })
}

#[allow(dead_code)] // called from extract_mutations (test crate path)
fn tool_kind_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Edit => "edit",
        ToolKind::Write => "write",
        ToolKind::MultiEdit => "multiedit",
    }
}

fn resolve_socket_paths(hook: &Hook) -> Vec<PathBuf> {
    let cwd_based = utils::find_matching_sockets().unwrap_or_default();
    if !cwd_based.is_empty() {
        return cwd_based;
    }
    let owned_tool: Tool;
    let tool: Option<&Tool> = match hook {
        Hook::Claude(ClaudeShape::PreToolUse(h)) => Some(&h.tool),
        Hook::Claude(ClaudeShape::PostToolUse(h)) => Some(&h.tool),
        Hook::Crush(CrushShape::PreToolUse(h)) => Some(&h.tool),
        Hook::Antigravity(AntigravityShape::PreToolUse(h)) => {
            owned_tool = antigravity_to_tool(h);
            Some(&owned_tool)
        }
        Hook::Claude(ClaudeShape::UserPromptSubmit(_))
        | Hook::Antigravity(AntigravityShape::PostToolUse(_)) => None,
    };
    let Some(tool) = tool else {
        return Vec::new();
    };
    for (_, file_path) in tool_to_mutations(tool) {
        let sockets = utils::find_sockets_for_path(Path::new(&file_path)).unwrap_or_default();
        if !sockets.is_empty() {
            return sockets;
        }
    }
    Vec::new()
}

/// Lower an Antigravity PreToolUse envelope's nested `toolCall` into the
/// generic `Tool::Other` shape so the existing mutation-extraction
/// pipeline handles it the same way as Crush's lowercase tools and
/// Codex's `apply_patch`.
fn antigravity_to_tool(h: &AntigravityToolHook) -> Tool {
    Tool::Other {
        tool_name: h.tool_call.name.clone(),
        tool_input: h.tool_call.args.clone(),
    }
}

fn tool_to_mutations(tool: &Tool) -> Vec<(ToolKind, String)> {
    match tool {
        Tool::Known(KnownTool::Edit(f)) => file_input_mutation(ToolKind::Edit, f),
        Tool::Known(KnownTool::Write(f)) => file_input_mutation(ToolKind::Write, f),
        Tool::Known(KnownTool::MultiEdit(f)) => file_input_mutation(ToolKind::MultiEdit, f),
        Tool::Other {
            tool_name,
            tool_input,
        } => other_tool_mutations(tool_name, tool_input),
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
        // Amp's `edit_file` and `create_file` are normalized server-side in
        // case a raw envelope reaches the handler; the plugin in
        // `plugins/amp/sidekick.ts` already maps them to Edit/Write before
        // shelling out, so this is defense in depth.
        "edit" | "edit_file" | "replace_file_content" => ToolKind::Edit,
        "write" | "create_file" | "write_to_file" => ToolKind::Write,
        "multiedit" | "multi_edit" | "multi_replace_file_content" => ToolKind::MultiEdit,
        _ => return Vec::new(),
    };

    if let Some(path) = pick_file_path(input) {
        return vec![(tool_kind, path.to_string())];
    }

    Vec::new()
}

fn pick_file_path(input: &serde_json::Value) -> Option<&str> {
    input.as_object().and_then(|obj| {
        // `TargetFile` is Antigravity's PascalCase key on
        // write_to_file / replace_file_content / multi_replace_file_content.
        ["file_path", "filePath", "path", "TargetFile"]
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
    use super::{WireFormat, build_allow, build_deny, tool_to_mutations};
    use crate::analytics::event::ToolKind;
    use crate::hook::Tool;

    #[test]
    fn antigravity_deny_output_places_decision_at_top_level() {
        // Regression: the original Antigravity slice emitted Claude-shape
        // hookSpecificOutput.permissionDecision; Antigravity reads
        // top-level `decision` and silently allowed the edit. The fix is
        // wire-format-aware build_deny; this test pins it.
        let output = build_deny(WireFormat::Antigravity, "blocked".into());
        let json = output.to_json().unwrap();

        assert!(json.contains("\"decision\":\"deny\""));
        assert!(json.contains("\"reason\":\"blocked\""));
        assert!(!json.contains("\"hookSpecificOutput\""));
    }

    #[test]
    fn claude_deny_output_still_uses_nested_permission_decision() {
        // Claude/Crush/opencode/pi/Amp must keep their existing wire
        // contract — top-level `decision` is not what they read.
        let output = build_deny(WireFormat::Claude, "blocked".into());
        let json = output.to_json().unwrap();

        assert!(json.contains("\"permissionDecision\":\"deny\""));
        assert!(json.contains("\"hookEventName\":\"PreToolUse\""));
        assert!(!json.contains("\"decision\":\"deny\","));
    }

    #[test]
    fn antigravity_allow_output_explicit_decision() {
        // Antigravity docs mark `decision` as required, so emit an
        // explicit allow rather than the Claude-shape empty `{}`.
        let output = build_allow(WireFormat::Antigravity);
        let json = output.to_json().unwrap();
        assert!(json.contains("\"decision\":\"allow\""));
    }

    #[test]
    fn claude_allow_output_stays_empty() {
        // Claude treats empty `{}` as allow; emitting decision/allow on
        // every PreToolUse here would be a wire change for five existing
        // harnesses.
        let output = build_allow(WireFormat::Claude);
        assert_eq!(output.to_json().unwrap(), "{}");
    }

    #[test]
    fn extracts_codex_apply_patch_paths() {
        let tool = Tool::Other {
            tool_name: "apply_patch".into(),
            tool_input: serde_json::json!({
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
            tool_name: "write".into(),
            tool_input: serde_json::json!({ "path": "README.md" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Write, "README.md".into())]
        );
    }

    #[test]
    fn extracts_crush_edit_file_path() {
        let tool = Tool::Other {
            tool_name: "edit".into(),
            tool_input: serde_json::json!({ "file_path": "src/foo.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Edit, "src/foo.rs".into())]
        );
    }

    #[test]
    fn extracts_crush_write_file_path() {
        let tool = Tool::Other {
            tool_name: "write".into(),
            tool_input: serde_json::json!({ "file_path": "README.md" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Write, "README.md".into())]
        );
    }

    #[test]
    fn extracts_crush_multiedit_file_path() {
        let tool = Tool::Other {
            tool_name: "multiedit".into(),
            tool_input: serde_json::json!({
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
    fn extracts_amp_edit_file_path() {
        let tool = Tool::Other {
            tool_name: "edit_file".into(),
            tool_input: serde_json::json!({ "path": "src/lib.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Edit, "src/lib.rs".into())]
        );
    }

    #[test]
    fn extracts_amp_create_file_path() {
        let tool = Tool::Other {
            tool_name: "create_file".into(),
            tool_input: serde_json::json!({ "file_path": "src/new.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Write, "src/new.rs".into())]
        );
    }

    #[test]
    fn extracts_antigravity_write_to_file_target_file() {
        let tool = Tool::Other {
            tool_name: "write_to_file".into(),
            tool_input: serde_json::json!({ "TargetFile": "/p/src/new.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Write, "/p/src/new.rs".into())]
        );
    }

    #[test]
    fn extracts_antigravity_replace_file_content_target_file() {
        let tool = Tool::Other {
            tool_name: "replace_file_content".into(),
            tool_input: serde_json::json!({ "TargetFile": "/p/src/lib.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::Edit, "/p/src/lib.rs".into())]
        );
    }

    #[test]
    fn extracts_antigravity_multi_replace_file_content_target_file() {
        let tool = Tool::Other {
            tool_name: "multi_replace_file_content".into(),
            tool_input: serde_json::json!({ "TargetFile": "/p/src/lib.rs" }),
        };

        assert_eq!(
            tool_to_mutations(&tool),
            vec![(ToolKind::MultiEdit, "/p/src/lib.rs".into())]
        );
    }

    #[test]
    fn ignores_apply_patch_without_file_markers() {
        let tool = Tool::Other {
            tool_name: "apply_patch".into(),
            tool_input: serde_json::json!({
                "patch": "*** Begin Patch\n@@\n-no file header\n+still no file header\n*** End Patch\n"
            }),
        };

        assert!(tool_to_mutations(&tool).is_empty());
    }
}
