use dscode::api;

// ── Tool definitions (public API) ──────────────────────────

#[test]
fn test_tool_definitions_have_at_least_20_tools() {
    let defs = dscode::api::tool_definitions();
    assert!(defs.len() >= 20, "should have at least 20 tools");
}

#[test]
fn test_tool_definitions_no_duplicate_names() {
    let defs = dscode::api::tool_definitions();
    let names: Vec<&str> = defs.iter()
        .filter_map(|d| d["function"]["name"].as_str())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names.len(), sorted.len(), "tool definitions should not contain duplicate names");
}

#[test]
fn test_tool_definitions_filtered_returns_subset() {
    let filtered = dscode::api::tool_definitions_filtered(&["read_file", "write_file"]);
    assert_eq!(filtered.len(), 2, "should return exactly 2 tools");
    let names: Vec<&str> = filtered.iter()
        .filter_map(|d| d["function"]["name"].as_str())
        .collect();
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"write_file"));
}

#[test]
fn test_tool_descriptions_exist_for_all_tools() {
    let defs = dscode::api::tool_definitions();
    for d in &defs {
        let name = d["function"]["name"].as_str().unwrap_or("?");
        let desc = d["function"]["description"].as_str().unwrap_or("");
        assert!(!desc.is_empty(), "tool '{name}' has no description");
    }
}

#[test]
fn test_tool_definitions_include_sub_agent_role_enum() {
    let defs = dscode::api::tool_definitions();
    let agent_open = defs.iter().find(|d| {
        d["function"]["name"].as_str() == Some("agent_open")
    }).expect("should have agent_open tool");
    let role_enum = agent_open["function"]["parameters"]["properties"]["role"]["enum"].as_array();
    assert!(role_enum.is_some(), "agent_open should have role enum");
    let roles: Vec<&str> = role_enum.unwrap().iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(roles.contains(&"explore"), "should include explore role");
    assert!(roles.contains(&"plan"), "should include plan role");
    assert!(roles.contains(&"verifier"), "should include verifier role");
    assert!(roles.contains(&"architect"), "should include architect role");
    assert!(roles.contains(&"coder"), "should include coder role");
    assert!(roles.contains(&"reviewer"), "should include reviewer role");
    assert!(roles.contains(&"tester"), "should include tester role");
}

// ── Tool name constants ────────────────────────────────────

#[test]
fn test_core_tool_names_non_empty() {
    let core = dscode::tools::CORE_TOOL_NAMES;
    assert!(!core.is_empty(), "CORE_TOOL_NAMES should not be empty");
    assert!(core.contains(&"read_file"));
    assert!(core.contains(&"write_file"));
    assert!(core.contains(&"run_shell"));
}

#[test]
fn test_all_tool_names_contains_core_tools() {
    let all = dscode::tools::ALL_TOOL_NAMES;
    assert!(!all.is_empty(), "ALL_TOOL_NAMES should not be empty");
    for core in dscode::tools::CORE_TOOL_NAMES {
        assert!(all.contains(core), "core tool '{core}' missing from ALL_TOOL_NAMES");
    }
}

#[test]
fn test_all_tool_names_contains_agent_tools() {
    let all = dscode::tools::ALL_TOOL_NAMES;
    assert!(all.contains(&"agent_open"));
    assert!(all.contains(&"agent_eval"));
    assert!(all.contains(&"agent_close"));
    assert!(all.contains(&"checklist_write"));
    assert!(all.contains(&"checklist_add"));
    assert!(all.contains(&"checklist_update"));
    assert!(all.contains(&"checklist_list"));
}

// ── MAX_TOOL_OUTPUT_CHARS ──────────────────────────────────

#[test]
fn test_max_tool_output_chars_in_range() {
    assert!(api::MAX_TOOL_OUTPUT_CHARS > 0, "must be positive");
    assert!(api::MAX_TOOL_OUTPUT_CHARS <= 200_000, "should be reasonable");
}
