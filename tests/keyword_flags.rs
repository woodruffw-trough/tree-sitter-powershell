//! Check keyword flags and declaration modes against PowerShell's retained AST.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn keyword_flags_and_function_modes_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/keyword_flags.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/keyword_flags.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 2000);
}
