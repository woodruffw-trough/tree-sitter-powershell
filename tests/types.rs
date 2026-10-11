mod common;

use serde_json::Value;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/types.json")).unwrap()
}

#[test]
fn type_name_boundaries_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/types.json");
    common::assert_commands(&cases, &results);
}
