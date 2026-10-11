//! Small cases exercise normalizations used for the full project comparison.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn mapped_semantic_relationships_match_powershell() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/structure.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/structure.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
