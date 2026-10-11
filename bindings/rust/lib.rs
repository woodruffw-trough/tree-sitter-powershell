//! PowerShell support for tree-sitter.
//!
//! ```
//! let mut parser = tree_sitter::Parser::new();
//! parser.set_language(&tree_sitter_powershell::LANGUAGE.into()).unwrap();
//! let tree = parser.parse("Get-ChildItem | Where-Object { $_.Length -gt 0 }", None).unwrap();
//! assert!(!tree.root_node().has_error());
//! ```

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_powershell() -> *const ();
}

/// The PowerShell grammar, for use with
/// [`Parser::set_language`](https://docs.rs/tree-sitter/0.27.1/tree_sitter/struct.Parser.html#method.set_language).
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_powershell) };

/// Named node types and fields generated from the grammar.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// Syntax highlighting captures for editors.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");
