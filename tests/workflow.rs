//! Workflow contexts retain the native InlineScript command boundaries.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn workflow_inline_script_contexts_match_powershell() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/workflow.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/workflow.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
