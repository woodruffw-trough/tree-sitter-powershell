//! Complete, unmodified scripts from the pinned reference implementation.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use std::{fs, path::Path};

fn fixtures() -> Vec<Value> {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("upstream/fixtures.json")).unwrap();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/upstream");
    for case in &mut cases {
        let source = fs::read_to_string(directory.join(case["path"].as_str().unwrap())).unwrap();
        // .NET's ReadAllText consumes a UTF-8 BOM before ParseInput.
        case["source"] = Value::String(
            source
                .strip_prefix('\u{feff}')
                .unwrap_or(&source)
                .to_owned(),
        );
    }
    cases
}

#[test]
fn complete_upstream_scripts_parse() {
    common::assert_syntax(&fixtures());
}

#[test]
fn upstream_cst_fields_and_commands_match_powershell() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/upstream/fixtures.json");
    common::assert_commands(&cases, &results);
    let fields = cst::assert_structure(&cases, &results);
    assert!(
        fields > 5_000,
        "Expected substantial mapped field coverage, got {fields}"
    );
}
