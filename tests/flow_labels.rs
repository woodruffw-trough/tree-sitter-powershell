//! Flow operands and colon-label fallback follow PowerShell's command-mode peek.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn flow_labels_and_following_statements_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/flow_labels.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/flow_labels.json");
    common::assert_commands(&cases, &results);
    let checks = cst::assert_structure(&cases, &results);
    assert!(checks > 200, "Only {checks} semantic relationship checks");
}
