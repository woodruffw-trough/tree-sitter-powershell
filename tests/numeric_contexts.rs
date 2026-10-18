//! Numeric token boundaries depend on initial lookahead and recursive operands.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::{Value, json};

#[test]
fn numeric_token_contexts_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/numeric_contexts.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/numeric_contexts.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn increment_target_restrictions_preserve_unary_syntax() {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/increment_semantics.json")).unwrap();
    let mut results = common::oracle(&cases, "tests/fixtures/increment_semantics.json");
    for (case, result) in cases.iter_mut().zip(&mut results) {
        let errors: Vec<_> = result["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["id"].as_str().unwrap())
            .collect();
        let expected: &[&str] = if case["valid"] == true {
            &[]
        } else {
            &["OperatorRequiresVariableOrProperty"]
        };
        assert_eq!(errors, expected, "{}", case["name"]);
        // SemanticChecks validates assignment targets after constructing the AST.
        case["valid"] = true.into();
        result["valid"] = true.into();
    }
    common::assert_syntax(&cases);
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn numeric_conversion_boundaries_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/numeric_values.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/numeric_values.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn quoted_numeric_comments_keep_expression_boundaries() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/numeric_quoted_comments.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/numeric_quoted_comments.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn signed_number_comments_keep_lexical_boundaries_despite_speculative_oracle_errors() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/numeric_comment_diagnostics.json")).unwrap();
    let results = common::oracle(&cases, "tests/fixtures/numeric_comment_diagnostics.json");
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        let source = case["source"].as_str().unwrap();
        let errors = result["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 1, "{source}");
        assert_eq!(errors[0]["id"], "TerminatorExpectedAtEndOfString");
        assert_eq!(errors[0]["start"], 3);
        assert_eq!(errors[0]["end"], source.len());
        assert_eq!(errors[0]["incomplete"], true);

        // Tokenizer.Resync removes speculative tokens but retains diagnostics
        // produced while scanning the initial generic word. UnaryExpressionRule
        // rescans the initial minus as part of a signed number. The final token
        // stream and AST contain a number followed by a line comment.
        let mut tokens = vec![
            json!({"kind": "Number", "text": "-1", "start": 0, "end": 2}),
            json!({"kind": "Comment", "text": &source[2..4], "start": 2, "end": 4}),
        ];
        if source.len() > 4 {
            tokens.push(json!({"kind": "NewLine", "text": "\n", "start": 4, "end": 5}));
            tokens.push(json!({"kind": "Number", "text": "2", "start": 5, "end": 6}));
        }
        tokens.push(json!({
            "kind": "EndOfInput", "text": "", "start": source.len() + 1,
            "end": source.len() + 1,
        }));
        assert_eq!(result["tokens"], json!(tokens), "{source}");
        assert_eq!(result["commands"], json!([]), "{source}");
        let constants = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "ConstantExpressionAst")
            .map(|node| {
                (
                    node["start"].as_u64().unwrap(),
                    node["end"].as_u64().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let expected = if source.len() > 4 {
            vec![(0, 2), (5, 6)]
        } else {
            vec![(0, 2)]
        };
        assert_eq!(constants, expected, "{source}");

        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error(), "{source}");
        let pipeline = root.named_child(0).unwrap();
        let command = pipeline.child_by_field_name("elements").unwrap();
        let number = command.child_by_field_name("expression").unwrap();
        assert_eq!(number.kind(), "unary_expression");
        assert_eq!(number.byte_range(), 0..2);
        assert_eq!(
            number.child_by_field_name("operator").unwrap().byte_range(),
            0..1
        );
        let argument = number.child_by_field_name("argument").unwrap();
        assert_eq!(argument.kind(), "integer_literal");
        assert_eq!(argument.byte_range(), 1..2);
        let comment = root.named_child(1).unwrap();
        assert_eq!(comment.kind(), "comment");
        assert_eq!(comment.byte_range(), 2..4);
        if source.len() > 4 {
            let following = root.named_child(2).unwrap();
            let command = following.child_by_field_name("elements").unwrap();
            let number = command.child_by_field_name("expression").unwrap();
            assert_eq!(number.kind(), "integer_literal");
            assert_eq!(number.byte_range(), 5..6);
        }
    }
}
