//! HookOutput serialisation tests — the JSON shape sidekick writes back to
//! its host. Parse-side coverage lives in `tests/integration.rs`.

use sidekick::hook::{HookOutput, PermissionDecision};

#[test]
fn empty_output_serialises_to_empty_object() {
    let output = HookOutput::new();
    assert_eq!(output.to_json().unwrap(), "{}");
}

#[test]
fn deny_output_carries_permission_decision_and_reason() {
    let output = HookOutput::new().with_permission_decision(
        PermissionDecision::Deny,
        Some("File has unsaved changes".to_string()),
    );
    let json = output.to_json().unwrap();

    // Three keys the host reads — the wire contract.
    assert!(json.contains("\"permissionDecision\":\"deny\""));
    assert!(json.contains("\"permissionDecisionReason\":\"File has unsaved changes\""));
    assert!(json.contains("\"hookEventName\":\"PreToolUse\""));
}

#[test]
fn system_message_output_carries_system_message_key() {
    let output = HookOutput::new().with_system_message("Test message");
    let json = output.to_json().unwrap();
    assert!(json.contains("\"systemMessage\":\"Test message\""));
}

#[test]
fn additional_context_output_targets_user_prompt_submit() {
    let output = HookOutput::new().with_additional_context("Selected code here");
    let json = output.to_json().unwrap();
    assert!(json.contains("\"additionalContext\":\"Selected code here\""));
    assert!(json.contains("\"hookEventName\":\"UserPromptSubmit\""));
}
