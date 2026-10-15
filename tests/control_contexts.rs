//! Statement boundaries and continuation lines follow the reference parser.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn control_contexts_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/control_contexts.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/control_contexts.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
