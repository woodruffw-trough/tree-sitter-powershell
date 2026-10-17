//! Command-mode member boundaries retain literal text and following commands.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn command_member_trivia_matches_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/member_trivia.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/member_trivia.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 400);
}
