//! Data sections preserve command-name and primary-expression boundaries.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn data_statements_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/data_statements.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/data_statements.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
