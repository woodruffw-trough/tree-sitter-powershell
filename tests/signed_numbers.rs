//! Adjacent numeric signs bind before members, with PowerShell's lookahead contexts.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn signed_numeric_members_and_generic_words_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/signed_numbers.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/signed_numbers.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
