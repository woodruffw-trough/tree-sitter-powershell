//! Control-header and script-body productions follow Microsoft's parser.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn statement_syntax_and_fields_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/statement_conformance.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/statement_conformance.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
