//! Unary productions retain their complete operands before semantic validation.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn unary_and_attributed_expressions_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/expression_conformance.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/expression_conformance.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
