//! Methods allow adjacency and newlines; only properties consume semicolons.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn class_member_delimiters_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/class_members.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/class_members.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
