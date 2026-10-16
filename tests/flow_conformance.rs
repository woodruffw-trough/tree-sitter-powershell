//! Leading-pipe continuations retain complete flow-control values.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn flow_control_continuation_and_structure_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/flow_conformance.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/flow_conformance.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
