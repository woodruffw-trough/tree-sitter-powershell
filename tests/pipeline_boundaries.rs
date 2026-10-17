//! Pipeline continuation boundaries follow the reference tokenizer.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn leading_pipeline_trivia_matches_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/pipeline_boundaries.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/pipeline_boundaries.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
