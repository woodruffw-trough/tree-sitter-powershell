//! Complete external projects: original source bytes, parse-only validation.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use std::{fs, path::Path, sync::OnceLock};

fn fixtures() -> Vec<Value> {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("projects/fixtures.json")).unwrap();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/projects");
    for case in &mut cases {
        let source = fs::read_to_string(directory.join(case["path"].as_str().unwrap())).unwrap();
        // Match .NET ReadAllText, which consumes one UTF-8 byte-order mark.
        case["source"] = Value::String(
            source
                .strip_prefix('\u{feff}')
                .unwrap_or(&source)
                .to_owned(),
        );
    }
    cases
}

fn oracle(cases: &[Value]) -> &'static [Value] {
    static RESULTS: OnceLock<Vec<Value>> = OnceLock::new();
    RESULTS.get_or_init(|| common::oracle(cases, "tests/projects/fixtures.json"))
}

#[test]
fn complete_external_project_files_parse() {
    common::assert_syntax(&fixtures());
}

#[test]
fn external_project_command_grouping_matches_powershell() {
    let cases = fixtures();
    common::assert_commands(&cases, oracle(&cases));
}

#[test]
fn external_project_cst_fields_match_powershell() {
    let cases = fixtures();
    let fields = cst::assert_structure(&cases, oracle(&cases));
    assert!(
        fields > 15_000,
        "Expected substantial mapped field coverage, got {fields}"
    );
}
