//! Shared byte positions and complete tree comparisons for editor tests.
use tree_sitter::{Node, Point};

pub fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    let row = prefix.iter().filter(|&&byte| byte == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(offset, |position| offset - position - 1);
    Point::new(row, column)
}

pub fn fingerprint(node: Node<'_>, output: &mut String) {
    use std::fmt::Write;
    let mut cursor = node.walk();
    loop {
        let node = cursor.node();
        write!(
            output,
            "({:?}:{}:{}..{}:{:?}..{:?}:{}:{}",
            cursor.field_name(),
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            node.start_position(),
            node.end_position(),
            node.is_error(),
            node.is_missing()
        )
        .unwrap();
        if cursor.goto_first_child() {
            continue;
        }
        output.push(')');
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
            output.push(')');
        }
    }
}
