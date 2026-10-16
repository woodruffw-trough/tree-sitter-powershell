//! Configuration headers and legacy block statements retain their syntax roles.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn configuration_headers_and_block_statements_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/configuration_conformance.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/configuration_conformance.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
