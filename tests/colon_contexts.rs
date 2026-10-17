//! Colon-prefixed words follow PowerShell's label and generic-token modes.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn colon_command_names_and_arguments_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/colon_contexts.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/colon_contexts.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 400);
}
