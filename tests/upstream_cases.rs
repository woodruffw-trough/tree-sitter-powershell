//! Individual parser inputs from Microsoft's pinned regression tests.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn upstream_parser_inputs_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("upstream/parser_cases.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/upstream/parser_cases.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
