//! Declaration syntax is retained independently of post-parse restrictions.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn declaration_syntax_and_semantics_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/declaration_conformance.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/declaration_conformance.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
