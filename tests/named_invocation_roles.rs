//! Source roles and metadata agree with the reference parser.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn named_invocation_roles_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/named_invocation_roles.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/named_invocation_roles.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 700);
}
