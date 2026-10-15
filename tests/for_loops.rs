//! For-header separators retain PowerShell's initializer, condition, and iterator.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn for_header_syntax_and_clause_fields_match_powershell() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/for_loops.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/for_loops.json");
    common::assert_commands(&cases, &results);
    let checks = cst::assert_structure(&cases, &results);
    assert!(checks > 200, "Only {checks} semantic relationship checks");
}
