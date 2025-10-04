//! Integration tests for hook processing

use sidekick::hook::{HookEvent, HookOutput, PermissionDecision, Tool, parse_hook};

#[test]
fn test_parse_pre_tool_use_edit_hook() {
    let json = r#"{
        "session_id": "test-session",
        "transcript_path": "/tmp/transcript",
        "cwd": "/test/dir",
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {
            "file_path": "test.txt",
            "old_string": "old",
            "new_string": "new"
        }
    }"#;

    let hook = parse_hook(json).expect("Failed to parse hook");

    assert_eq!(hook.session_id, "test-session");
    assert_eq!(hook.cwd, "/test/dir");
    assert_eq!(hook.hook_event_name, HookEvent::PreToolUse);

    match hook.tool {
        Tool::Edit(input) => {
            assert_eq!(input.file_path, "test.txt");
            assert_eq!(input.old_string, Some("old".to_string()));
            assert_eq!(input.new_string, Some("new".to_string()));
        }
        _ => panic!("Expected Edit tool"),
    }
}

#[test]
fn test_parse_post_tool_use_write_hook() {
    let json = r#"{
        "session_id": "test-session",
        "transcript_path": "/tmp/transcript",
        "cwd": "/test/dir",
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {
            "file_path": "test.txt",
            "content": "file content"
        }
    }"#;

    let hook = parse_hook(json).expect("Failed to parse hook");

    assert_eq!(hook.hook_event_name, HookEvent::PostToolUse);

    match hook.tool {
        Tool::Write(input) => {
            assert_eq!(input.file_path, "test.txt");
            assert_eq!(input.content, Some("file content".to_string()));
        }
        _ => panic!("Expected Write tool"),
    }
}

#[test]
fn test_parse_bash_hook() {
    let json = r#"{
        "session_id": "test-session",
        "transcript_path": "/tmp/transcript",
        "cwd": "/test/dir",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {
            "command": "ls -la",
            "description": "List files"
        }
    }"#;

    let hook = parse_hook(json).expect("Failed to parse hook");

    match hook.tool {
        Tool::Bash(input) => {
            assert_eq!(input.command, "ls -la");
            assert_eq!(input.description, "List files");
        }
        _ => panic!("Expected Bash tool"),
    }
}

#[test]
fn test_hook_output_allow() {
    let output = HookOutput::new();
    let json = output.to_json().expect("Failed to serialize");

    assert_eq!(json, "{}");
}

#[test]
fn test_hook_output_deny() {
    let output = HookOutput::new().with_permission_decision(
        PermissionDecision::Deny,
        Some("File has unsaved changes".to_string()),
    );

    let json = output.to_json().expect("Failed to serialize");

    assert!(json.contains("\"permissionDecision\":\"deny\""));
    assert!(json.contains("\"permissionDecisionReason\":\"File has unsaved changes\""));
    assert!(json.contains("\"hookEventName\":\"PreToolUse\""));
}

#[test]
fn test_hook_output_with_system_message() {
    let output = HookOutput::new().with_system_message("Test message");

    let json = output.to_json().expect("Failed to serialize");

    assert!(json.contains("\"systemMessage\":\"Test message\""));
}

#[test]
fn test_parse_multiedit_hook() {
    let json = r#"{
        "session_id": "test-session",
        "transcript_path": "/tmp/transcript",
        "cwd": "/test/dir",
        "hook_event_name": "PreToolUse",
        "tool_name": "MultiEdit",
        "tool_input": {
            "file_path": "test.txt"
        }
    }"#;

    let hook = parse_hook(json).expect("Failed to parse hook");

    match hook.tool {
        Tool::MultiEdit(input) => {
            assert_eq!(input.file_path, "test.txt");
        }
        _ => panic!("Expected MultiEdit tool"),
    }
}
