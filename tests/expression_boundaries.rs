//! Postfix operators allow horizontal separation; parentheses contain one pipeline.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn expression_boundaries_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/expression_boundaries.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/expression_boundaries.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
