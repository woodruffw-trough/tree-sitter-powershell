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

#[test]
fn using_constants_and_aliases_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/using_conformance.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/using_conformance.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn parameter_block_type_prefixes_retain_complete_headers() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/param_prefixes.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/param_prefixes.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
