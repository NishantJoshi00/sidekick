//! Integration tests for the hook decision pipeline.
//!
//! These tests assert only the behaviors that drive sidekick's correctness:
//!
//! - **Event routing.** PreToolUse fires the deny path, PostToolUse fires
//!   the refresh path, UserPromptSubmit fires the selection-injection
//!   path. Wrong event = wrong code path.
//! - **File path extraction.** The extracted path is the input to the deny
//!   decision. Wrong path = wrong file checked.
//! - **Tool engagement.** Edit/Write/MultiEdit/apply_patch engage sidekick;
//!   Bash/Read and unknown tools must not. Over-engaging blocks shell
//!   commands; under-engaging misses edits.
//! - **Multi-file patch coverage.** Codex's apply_patch can touch multiple
//!   files in one tool call; missing one means its deny check is skipped.
//!
//! Pass-through fields (`session_id`, `cwd`, `transcript_path`,
//! `old_string`, `new_string`) are not asserted — sidekick never decides
//! on those values.

use sidekick::handler::{classify_event, extract_mutations};

// ---------------------------------------------------------------------------
// Claude shape (also covers Codex with PascalCase tools, opencode-bridge,
// pi-bridge — all four normalize to this envelope).
// ---------------------------------------------------------------------------

#[test]
fn claude_pre_tool_use_edit_extracts_file_path() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": { "file_path": "/p/src/lib.rs", "old_string": "a", "new_string": "b" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn claude_pre_tool_use_write_extracts_file_path() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": "/p/src/new.rs", "content": "hello" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("write".to_string(), "/p/src/new.rs".to_string())]
    );
}

#[test]
fn claude_pre_tool_use_multiedit_extracts_file_path() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "MultiEdit",
        "tool_input": { "file_path": "/p/src/lib.rs" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("multiedit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn claude_pre_tool_use_bash_does_not_engage() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": "ls -la", "description": "list" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn claude_pre_tool_use_read_does_not_engage() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "Read",
        "tool_input": { "file_path": "/p/src/lib.rs" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn claude_post_tool_use_routes_as_post_event() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PostToolUse",
        "tool_name": "Edit",
        "tool_input": { "file_path": "/p/src/lib.rs" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PostToolUse");
    // Same extraction logic — PostToolUse drives buffer refresh on the
    // same paths PreToolUse would have denied.
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn claude_user_prompt_submit_routes_to_prompt_path() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "UserPromptSubmit",
        "prompt": "do the thing"
    }"#;

    assert_eq!(classify_event(json).unwrap(), "UserPromptSubmit");
    // UserPromptSubmit never carries file mutations — it's for selection
    // injection, not edit gating.
    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn claude_unknown_pascalcase_tool_does_not_engage() {
    // Anything sidekick doesn't recognise (a future Claude Code tool, an
    // MCP tool, etc.) must NOT engage. Over-engagement blocks unrelated
    // tool calls.
    let json = r#"{
        "session_id": "s",
        "transcript_path": "/tmp/t",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "WebFetch",
        "tool_input": { "url": "https://example.com" }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Codex apply_patch — Codex (and opencode on GPT models) bundles
// multiple file edits into one tool call. Every file in the patch must
// surface as a mutation, or its deny check is silently skipped.
// ---------------------------------------------------------------------------

#[test]
fn codex_apply_patch_extracts_every_marker_kind() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": null,
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "apply_patch",
        "tool_input": {
            "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n*** Add File: src/new.rs\n*** Delete File: src/old.rs\n*** Move to: src/moved.rs\n*** End Patch\n"
        }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![
            ("edit".to_string(), "src/lib.rs".to_string()),
            ("edit".to_string(), "src/new.rs".to_string()),
            ("edit".to_string(), "src/old.rs".to_string()),
            ("edit".to_string(), "src/moved.rs".to_string()),
        ]
    );
}

#[test]
fn codex_apply_patch_with_null_transcript_path_parses() {
    // Regression: Codex sometimes emits transcript_path as null, which
    // used to break parse_hook before transcript_path became Option.
    let json = r#"{
        "session_id": "s",
        "turn_id": "t",
        "transcript_path": null,
        "cwd": "/p",
        "model": "gpt-5.5",
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": "apply_patch",
        "tool_use_id": "u",
        "tool_input": { "patch": "*** Begin Patch\n*** Update File: a.rs\n*** End Patch\n" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "a.rs".to_string())]
    );
}

#[test]
fn codex_apply_patch_without_file_markers_does_not_engage() {
    // A patch body with no Add/Update/Delete/Move markers touches no
    // files — sidekick must not engage on an empty file list.
    let json = r#"{
        "session_id": "s",
        "transcript_path": null,
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "apply_patch",
        "tool_input": {
            "patch": "*** Begin Patch\n@@ no header\n+content\n*** End Patch\n"
        }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Crush — `event` instead of `hook_event_name`, no `transcript_path`,
// lowercase tool names. The bridge that's already in production.
// ---------------------------------------------------------------------------

#[test]
fn crush_pre_tool_use_edit_extracts_file_path() {
    let json = r#"{
        "event": "PreToolUse",
        "session_id": "s",
        "cwd": "/p",
        "tool_name": "edit",
        "tool_input": { "file_path": "/p/src/lib.rs", "old_string": "a", "new_string": "b" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn crush_pre_tool_use_write_extracts_file_path() {
    let json = r#"{
        "event": "PreToolUse",
        "session_id": "s",
        "cwd": "/p",
        "tool_name": "write",
        "tool_input": { "file_path": "/p/src/new.rs", "content": "hello" }
    }"#;

    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("write".to_string(), "/p/src/new.rs".to_string())]
    );
}

#[test]
fn crush_pre_tool_use_multiedit_extracts_file_path() {
    let json = r#"{
        "event": "PreToolUse",
        "session_id": "s",
        "cwd": "/p",
        "tool_name": "multiedit",
        "tool_input": {
            "file_path": "/p/src/lib.rs",
            "edits": [{ "old_string": "a", "new_string": "b" }]
        }
    }"#;

    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("multiedit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn crush_pre_tool_use_bash_does_not_engage() {
    let json = r#"{
        "event": "PreToolUse",
        "session_id": "s",
        "cwd": "/p",
        "tool_name": "bash",
        "tool_input": { "command": "ls -la" }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn crush_unknown_lowercase_tool_does_not_engage() {
    let json = r#"{
        "event": "PreToolUse",
        "session_id": "s",
        "cwd": "/p",
        "tool_name": "fetch_url",
        "tool_input": { "url": "https://example.com" }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Lowercase-via-Codex envelopes — Codex sometimes emits lowercase tool
// names alongside apply_patch. The handler normalizes via to_ascii_lower
// case, so lowercase variants should engage identically to Crush's
// lowercase tools even when carried by the Claude-shape envelope.
// ---------------------------------------------------------------------------

#[test]
fn lowercase_edit_under_claude_shape_engages() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": null,
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "edit",
        "tool_input": { "file_path": "/p/a.rs" }
    }"#;

    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/a.rs".to_string())]
    );
}

// ---------------------------------------------------------------------------
// Amp — the plugin in plugins/amp/sidekick.ts maps edit_file/create_file
// to Edit/Write before constructing the envelope, so day-to-day Amp
// traffic is covered by the Claude-shape tests above. These cover the
// raw lowercase names sidekick normalizes server-side as defense in
// depth (and as the executable spec for handler.rs::other_tool_mutations).
// ---------------------------------------------------------------------------

#[test]
fn amp_edit_file_extracts_file_path() {
    let json = r#"{
        "session_id": "thread-1",
        "transcript_path": "",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "edit_file",
        "tool_input": { "path": "/p/src/lib.rs" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn amp_create_file_extracts_file_path() {
    let json = r#"{
        "session_id": "thread-1",
        "transcript_path": "",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "create_file",
        "tool_input": { "file_path": "/p/src/new.rs", "content": "hello" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("write".to_string(), "/p/src/new.rs".to_string())]
    );
}

#[test]
fn amp_post_tool_use_edit_file_routes_as_post() {
    let json = r#"{
        "session_id": "thread-1",
        "transcript_path": "",
        "cwd": "/p",
        "hook_event_name": "PostToolUse",
        "tool_name": "edit_file",
        "tool_input": { "path": "/p/src/lib.rs" }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PostToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn amp_unknown_tool_does_not_engage() {
    // Amp's built-in surface includes Bash, Read, finder, oracle, etc.
    // (17 tools per `amp tools list`). Only edit_file/create_file mutate
    // files — everything else must pass through without engaging.
    let json = r#"{
        "session_id": "thread-1",
        "transcript_path": "",
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "finder",
        "tool_input": { "query": "lib.rs" }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Antigravity — no top-level event tag, camelCase fields, nested toolCall,
// PascalCase argument keys (`TargetFile`). sidekick only registers for
// PreToolUse, so every incoming envelope is one. PostToolUse omits the
// tool name/args entirely, so buffer refresh isn't possible — those
// envelopes never reach the handler in production.
// ---------------------------------------------------------------------------

#[test]
fn antigravity_pre_tool_use_write_to_file_extracts_target_file() {
    let json = r#"{
        "conversationId": "ec33ebf9-0cba-4100-8142-c61503f6c587",
        "workspacePaths": ["/workspace/project"],
        "transcriptPath": "/workspace/project/.gemini/jetski/transcript.jsonl",
        "artifactDirectoryPath": "/workspace/project/.gemini/jetski/artifacts",
        "stepIdx": 19,
        "toolCall": {
            "name": "write_to_file",
            "args": {
                "TargetFile": "/workspace/project/src/new.rs",
                "Overwrite": false,
                "CodeContent": "fn main() {}",
                "Description": "create entry point"
            }
        }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![(
            "write".to_string(),
            "/workspace/project/src/new.rs".to_string()
        )]
    );
}

#[test]
fn antigravity_pre_tool_use_replace_file_content_extracts_target_file() {
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "stepIdx": 3,
        "toolCall": {
            "name": "replace_file_content",
            "args": {
                "TargetFile": "/p/src/lib.rs",
                "Instruction": "swap a for b",
                "TargetContent": "a",
                "ReplacementContent": "b"
            }
        }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn antigravity_pre_tool_use_multi_replace_file_content_extracts_target_file() {
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "stepIdx": 5,
        "toolCall": {
            "name": "multi_replace_file_content",
            "args": {
                "TargetFile": "/p/src/lib.rs",
                "ReplacementChunks": []
            }
        }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("multiedit".to_string(), "/p/src/lib.rs".to_string())]
    );
}

#[test]
fn antigravity_pre_tool_use_view_file_does_not_engage() {
    // view_file is a read tool; sidekick must not engage on reads.
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "stepIdx": 1,
        "toolCall": {
            "name": "view_file",
            "args": { "AbsolutePath": "/p/src/lib.rs" }
        }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn antigravity_pre_tool_use_run_command_does_not_engage() {
    // Bash analog — engages no buffer-modification gate.
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "stepIdx": 2,
        "toolCall": {
            "name": "run_command",
            "args": { "CommandLine": "ls -la", "Cwd": "/p" }
        }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn antigravity_unknown_tool_does_not_engage() {
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "stepIdx": 4,
        "toolCall": {
            "name": "grep_search",
            "args": { "SearchPath": "/p", "Query": "TODO" }
        }
    }"#;

    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn antigravity_post_tool_use_routes_as_post_event() {
    // PostToolUse omits `toolCall` entirely — the structural discriminator
    // for Antigravity's two events. extract_mutations returns empty (no
    // toolCall = no per-file path), and classify_event reports Post.
    let json = r#"{
        "conversationId": "ec33ebf9-0cba-4100-8142-c61503f6c587",
        "workspacePaths": ["/workspace/project"],
        "transcriptPath": "/workspace/project/.gemini/jetski/transcript.jsonl",
        "artifactDirectoryPath": "/workspace/project/.gemini/jetski/artifacts",
        "stepIdx": 5,
        "error": ""
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PostToolUse");
    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn antigravity_post_tool_use_with_error_parses() {
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "stepIdx": 5,
        "error": "exit status 1"
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PostToolUse");
    assert!(extract_mutations(json).unwrap().is_empty());
}

#[test]
fn antigravity_optional_fields_parse() {
    // Minimum required: conversationId, workspacePaths, toolCall.
    // transcriptPath, artifactDirectoryPath, stepIdx are optional.
    let json = r#"{
        "conversationId": "c",
        "workspacePaths": ["/p"],
        "toolCall": {
            "name": "write_to_file",
            "args": { "TargetFile": "/p/a.rs" }
        }
    }"#;

    assert_eq!(classify_event(json).unwrap(), "PreToolUse");
    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("write".to_string(), "/p/a.rs".to_string())]
    );
}

// ---------------------------------------------------------------------------
// Field name fallback — older opencode payloads used `filePath`; pi used
// `path` in some cases. Handler tries `file_path`, `filePath`, `path` in
// order. If any drift, the wrong key gets read and the wrong file gets
// checked.
// ---------------------------------------------------------------------------

#[test]
fn camelcase_file_path_field_is_accepted() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": null,
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "edit",
        "tool_input": { "filePath": "/p/camel.rs" }
    }"#;

    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("edit".to_string(), "/p/camel.rs".to_string())]
    );
}

#[test]
fn bare_path_field_is_accepted() {
    let json = r#"{
        "session_id": "s",
        "transcript_path": null,
        "cwd": "/p",
        "hook_event_name": "PreToolUse",
        "tool_name": "write",
        "tool_input": { "path": "/p/bare.rs" }
    }"#;

    assert_eq!(
        extract_mutations(json).unwrap(),
        vec![("write".to_string(), "/p/bare.rs".to_string())]
    );
}
