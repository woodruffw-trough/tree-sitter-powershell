//! Empty invocation and attribute lists retain their argument-list nodes.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn empty_arguments_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/empty_arguments.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/empty_arguments.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 500);
}
