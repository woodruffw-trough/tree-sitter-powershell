//! Pipeline precedence, public fields, and incremental parsing against PowerShell.
mod common;

use serde_json::{Value, json};
use std::collections::BTreeMap;
use tree_sitter::{InputEdit, Node, Point, Query, QueryCursor, StreamingIterator};

type Records = BTreeMap<(String, common::Extent), Value>;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/pipelines.json")).unwrap()
}

#[test]
fn pipeline_syntax_matches_powershell() {
    common::assert_syntax(&fixtures());
}

fn field<'a>(node: Node<'a>, name: &str) -> Node<'a> {
    let mut cursor = node.walk();
    let children = node
        .children_by_field_name(name, &mut cursor)
        .collect::<Vec<_>>();
    assert_eq!(
        children.len(),
        1,
        "{}.{} must identify one complete child: {}",
        node.kind(),
        name,
        node.to_sexp()
    );
    children[0]
}

fn fields<'a>(node: Node<'a>, name: &str) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children_by_field_name(name, &mut cursor).collect()
}

fn background(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "background_pipeline"
            && parent.child_by_field_name("pipeline") == Some(node)
    })
}

// A concrete background wrapper retains &, but Microsoft's pipeline, chain,
// assignment, and flow extents stop at the operand. Parentheses retain their
// closing delimiter and are deliberately not shortened through nested values.
fn semantic_end(node: Node<'_>) -> usize {
    match node.kind() {
        "background_pipeline" => field(node, "pipeline").end_byte(),
        "assignment_statement" | "flow_control_statement" => {
            let value = node.child_by_field_name(if node.kind() == "assignment_statement" {
                "right"
            } else {
                "value"
            });
            value
                .filter(|value| value.end_byte() == node.end_byte())
                .map_or(node.end_byte(), semantic_end)
        }
        _ => node.end_byte(),
    }
}

fn extent(node: Node<'_>, source: &str) -> Value {
    json!({
        "start": source[..node.start_byte()].encode_utf16().count(),
        "end": source[..semantic_end(node)].encode_utf16().count(),
    })
}

fn optional_extent(node: Node<'_>, name: &str, source: &str) -> Value {
    node.child_by_field_name(name)
        .map_or(Value::Null, |child| extent(child, source))
}

fn extents(nodes: &[Node<'_>], source: &str) -> Value {
    Value::Array(nodes.iter().map(|node| extent(*node, source)).collect())
}

fn is_unwrapped_assignment_pipeline(node: Node<'_>) -> bool {
    // Exact AssignmentStatementAst constructor normalization in pinned ast.cs:
    // direct RHS, one CommandExpressionAst element, and no background operation.
    // https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/ast.cs#L6399-L6410
    node.parent().is_some_and(|parent| {
        parent.kind() == "assignment_statement"
            && parent.child_by_field_name("right") == Some(node)
            && !background(node)
            && matches!(fields(node, "elements").as_slice(), [element] if element.kind() == "command_expression")
    })
}

fn assignment_operator(node: Node<'_>, source: &str) -> &'static str {
    match &source[field(node, "operator").byte_range()] {
        "=" => "Equals",
        "+=" => "PlusEquals",
        "-=" => "MinusEquals",
        "*=" => "MultiplyEquals",
        "/=" => "DivideEquals",
        "%=" => "RemainderEquals",
        "??=" => "QuestionQuestionEquals",
        operator => panic!("Unexpected assignment operator {operator:?}"),
    }
}

fn collect(node: Node<'_>, source: &str, records: &mut Records) {
    let record = match node.kind() {
        "pipeline" if !is_unwrapped_assignment_pipeline(node) => {
            let elements = fields(node, "elements");
            assert!(!elements.is_empty(), "{}", node.to_sexp());
            assert!(
                elements
                    .iter()
                    .all(|element| { matches!(element.kind(), "command" | "command_expression") })
            );
            Some((
                "PipelineAst",
                json!({"elements": extents(&elements, source), "background": background(node)}),
            ))
        }
        "pipeline_chain" => {
            let left = field(node, "left");
            let right = field(node, "right");
            let operator = field(node, "operator");
            assert!(matches!(left.kind(), "pipeline" | "pipeline_chain"));
            assert_eq!(right.kind(), "pipeline");
            assert_eq!(node.start_byte(), left.start_byte());
            assert_eq!(node.end_byte(), right.end_byte());
            assert!(left.end_byte() <= operator.start_byte());
            assert!(operator.end_byte() <= right.start_byte());
            let operator = match &source[operator.byte_range()] {
                "&&" => "AndAnd",
                "||" => "OrOr",
                operator => panic!("Unexpected chain operator {operator:?}"),
            };
            Some((
                "PipelineChainAst",
                json!({
                    "left": extent(left, source), "right": extent(right, source),
                    "operator": operator, "background": background(node),
                }),
            ))
        }
        "background_pipeline" => {
            let pipeline = field(node, "pipeline");
            let operator = field(node, "operator");
            assert!(matches!(pipeline.kind(), "pipeline" | "pipeline_chain"));
            assert_eq!(&source[operator.byte_range()], "&");
            assert_eq!(node.start_byte(), pipeline.start_byte());
            assert_eq!(node.end_byte(), operator.end_byte());
            assert!(pipeline.end_byte() <= operator.start_byte());
            None
        }
        "command_expression" => {
            let expression = field(node, "expression");
            let mut redirections = fields(node, "redirections");
            assert!(redirections.iter().all(|node| node.kind() == "redirection"));
            // The semantic AST stores redirections by stream slot, while the
            // concrete tree retains source order. Compare source-ordered ranges.
            redirections.sort_by_key(Node::start_byte);
            Some((
                "CommandExpressionAst",
                json!({
                    "expression": extent(expression, source),
                    "redirections": extents(&redirections, source),
                }),
            ))
        }
        "assignment_statement" => Some((
            "AssignmentStatementAst",
            json!({
                "left": extent(field(node, "left"), source),
                "right": extent(field(node, "right"), source),
                "operator": assignment_operator(node, source),
            }),
        )),
        "flow_control_statement" => {
            let kind = match field(node, "keyword").kind() {
                "return" => "ReturnStatementAst",
                "throw" => "ThrowStatementAst",
                "exit" => "ExitStatementAst",
                "break" => "BreakStatementAst",
                "continue" => "ContinueStatementAst",
                keyword => panic!("Unexpected flow keyword {keyword:?}"),
            };
            Some((
                kind,
                json!({"value": optional_extent(node, "value", source)}),
            ))
        }
        _ => None,
    };
    if let Some((kind, record)) = record {
        let span = extent(node, source);
        let key = (
            kind.to_owned(),
            (
                span["start"].as_u64().unwrap() as usize,
                span["end"].as_u64().unwrap() as usize,
            ),
        );
        assert!(
            records.insert(key.clone(), record).is_none(),
            "Duplicate CST node {key:?}"
        );
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect(child, source, records);
    }
}

fn expected_records(result: &Value) -> Records {
    let mut records = Records::new();
    for node in result["ast"].as_array().unwrap() {
        let kind = node["type"].as_str().unwrap();
        let record = match kind {
            "PipelineAst" => json!({
                "elements": node["fields"]["elements"], "background": node["background"],
            }),
            "PipelineChainAst" => json!({
                "left": node["fields"]["left"], "right": node["fields"]["right"],
                "operator": node["operator"], "background": node["background"],
            }),
            "CommandExpressionAst" => {
                let mut redirections = node["fields"]["redirections"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                redirections.sort_by_key(|node| node["start"].as_u64().unwrap());
                json!({"expression": node["fields"]["expression"], "redirections": redirections})
            }
            "AssignmentStatementAst" => json!({
                "left": node["fields"]["left"], "right": node["fields"]["right"],
                "operator": node["operator"],
            }),
            "ReturnStatementAst"
            | "ThrowStatementAst"
            | "ExitStatementAst"
            | "BreakStatementAst"
            | "ContinueStatementAst" => {
                json!({"value": node["fields"]["value"]})
            }
            _ => continue,
        };
        let key = (
            kind.to_owned(),
            (
                node["start"].as_u64().unwrap() as usize,
                node["end"].as_u64().unwrap() as usize,
            ),
        );
        assert!(
            records.insert(key.clone(), record).is_none(),
            "Duplicate oracle node {key:?}"
        );
    }
    records
}

#[test]
fn pipeline_ranges_fields_and_background_agree_with_live_oracle() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/fixtures/pipelines.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    let mut failures = Vec::new();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = Records::new();
        collect(tree.root_node(), source, &mut actual);
        let expected = expected_records(result);
        if actual != expected {
            failures.push(format!(
                "{}: pipeline/assignment/flow relationships differ\n{}\nexpected: {expected:#?}\nactual: {actual:#?}\n{}",
                case["name"], source, tree.root_node().to_sexp()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        common::failure_summary(&failures)
    );
}

#[test]
fn pipeline_public_fields_support_complete_range_queries() {
    let source = "1 > one | b && c > two\na&&b &";
    let language = tree_sitter_powershell::LANGUAGE.into();
    let query = Query::new(
        &language,
        r#"
        (pipeline
          elements: (command_expression
            expression: (_) @expression
            redirections: (redirection) @redirection)) @expression.pipeline
        (pipeline_chain
          left: (_) @left
          operator: _ @chain.operator
          right: (pipeline) @right) @chain
        (background_pipeline
          pipeline: (_) @background.value
          operator: _ @background.operator) @background
    "#,
    )
    .unwrap();
    let tree = common::parser().parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.captures(&query, tree.root_node(), source.as_bytes());
    let mut actual = BTreeMap::<String, Vec<String>>::new();
    while let Some((matched, index)) = matches.next() {
        let capture = matched.captures()[*index];
        let name = query.capture_names()[capture.index as usize];
        actual
            .entry(name.to_owned())
            .or_default()
            .push(source[capture.node.byte_range()].to_owned());
    }
    let expected = [
        ("expression", vec!["1"]),
        ("redirection", vec!["> one"]),
        ("expression.pipeline", vec!["1 > one | b"]),
        ("left", vec!["1 > one | b", "a"]),
        ("right", vec!["c > two", "b"]),
        ("chain.operator", vec!["&&", "&&"]),
        ("chain", vec!["1 > one | b && c > two", "a&&b"]),
        ("background.value", vec!["a&&b"]),
        ("background.operator", vec!["&"]),
        ("background", vec!["a&&b &"]),
    ]
    .into_iter()
    .map(|(name, values)| {
        (
            name.to_owned(),
            values.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
    })
    .collect::<BTreeMap<_, _>>();
    assert_eq!(actual, expected);
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    let row = prefix.iter().filter(|&&byte| byte == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(offset, |position| offset - position - 1);
    Point::new(row, column)
}

fn fingerprint(node: Node<'_>, output: &mut String) {
    use std::fmt::Write;
    write!(
        output,
        "({}:{}..{}:{:?}..{:?}:{}:{}",
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        node.start_position(),
        node.end_position(),
        node.is_missing(),
        node.is_error()
    )
    .unwrap();
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        write!(output, "{:?}", node.field_name_for_child(index as u32)).unwrap();
        fingerprint(child, output);
    }
    output.push(')');
}

#[test]
fn pipeline_operator_and_background_edits_match_fresh_trees() {
    for (source, old, new, initially_valid, edited_valid) in [
        ("a | b && c | d || e\n", "&&", "||", true, true),
        ("a | b && c | d || e\n", "||", "&&", true, true),
        ("a&&b&c", "b&c", "bc", true, true),
        ("$x=$y=a&&b&c", "b&c", "b &\nc", true, true),
        ("a && b", "b", "", true, false),
        ("a &&", "&&", "|| b", false, true),
        ("a && b |", "|", "| c", false, true),
        ("a&&b&c", "&&", "& &&", true, false),
        ("return a&b", "a&b", "a && b & c", true, true),
        ("a&&b", "&&", "\n&&", true, false),
        ("a&&b", "&&", "&&\n", true, true),
        ("a&&b", "b", "(b || c)", true, true),
        ("a&&b", "b", "($x=1)", true, true),
        ("a&&b", "b", "$x=1", true, false),
        ("a && b > two", "two", "", true, false),
        ("'世界' && a", "世界", "😀", true, true),
        ("a && (b || c)", ")", "", true, false),
    ] {
        let mut parser = common::parser();
        let mut original = parser.parse(source, None).unwrap();
        assert_eq!(
            !original.root_node().has_error(),
            initially_valid,
            "{source}"
        );
        let start = source.find(old).unwrap();
        let end = start + old.len();
        let edited = format!("{}{}{}", &source[..start], new, &source[end..]);
        original.edit(&InputEdit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + new.len(),
            start_position: point(source, start),
            old_end_position: point(source, end),
            new_end_position: point(&edited, start + new.len()),
        });
        let incremental = parser.parse(&edited, Some(&original)).unwrap();
        let fresh = parser.parse(&edited, None).unwrap();
        assert_eq!(!fresh.root_node().has_error(), edited_valid, "{edited}");
        let mut actual = String::new();
        let mut expected = String::new();
        fingerprint(incremental.root_node(), &mut actual);
        fingerprint(fresh.root_node(), &mut expected);
        assert_eq!(
            actual, expected,
            "Incremental pipeline tree differs for {edited}"
        );
    }
}
