//! Unary comma and physical newlines depend on the surrounding parser context.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn expression_contexts_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/expression_contexts.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/expression_contexts.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
