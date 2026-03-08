use super::vscode::{classify_vscode_tool, summarize_vscode_tool};

#[test]
fn vscode_tool_classification_reading() {
    assert_eq!(format!("{:?}", classify_vscode_tool("read_file")), "Reading");
    assert_eq!(format!("{:?}", classify_vscode_tool("openFile")), "Reading");
    assert_eq!(format!("{:?}", classify_vscode_tool("LIST_FILES")), "Reading");
}

#[test]
fn vscode_tool_classification_editing() {
    assert_eq!(format!("{:?}", classify_vscode_tool("edit_file")), "Editing");
    assert_eq!(format!("{:?}", classify_vscode_tool("applyPatch")), "Editing");
    assert_eq!(format!("{:?}", classify_vscode_tool("write_file")), "Editing");
}

#[test]
fn vscode_tool_classification_approval_default() {
    // Unknown tools are treated conservatively as approval-required.
    assert_eq!(format!("{:?}", classify_vscode_tool("runCommand")), "Approval");
    assert_eq!(format!("{:?}", classify_vscode_tool("some_unknown_tool")), "Approval");
}

#[test]
fn vscode_tool_summary_run_command_uses_command() {
    let input = serde_json::json!({"command": "echo hello"});
    assert_eq!(summarize_vscode_tool("runCommand", Some(&input)), "echo hello".to_string());
}

#[test]
fn vscode_tool_summary_default_is_tool_name() {
    assert_eq!(summarize_vscode_tool("read_file", None), "read_file".to_string());
}
