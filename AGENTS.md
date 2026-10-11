# Development constraints

- Enable and generate only Rust bindings. Keep `tree-sitter.json` and `bindings/rust` consistent.
- Use tree-sitter CLI 0.27.1 compiled with `--no-default-features --features qjs-rt`.
- Set `TREE_SITTER_JS_RUNTIME=native` and always generate with `tree-sitter generate --js-runtime native`. Do not invoke Node, npm, or npx for tree-sitter. Official GitHub Actions may use Node internally.
- Treat Microsoft documentation, the pinned PowerShell parser source, and PowerShell 7.5.4 `Parser::ParseInput` as syntax authorities. Parse fixture code without executing it.
- Add regression cases for syntax changes. Review corpus trees against the oracle; accepting text without errors is insufficient when grouping can differ.
- Run the corpus, `cargo test --locked`, `scripts/check-generated.sh`, `python3 scripts/generate-fixtures.py` (including `--variables`), and `scripts/lint.sh` before pushing a finished change. CI runs an online strict, pedantic zizmor audit; local offline checks may set `ZIZMOR_OFFLINE=true`.
- Use the authenticated GitHub identity for commits. This repository's automation identity is `woodruffw-bot` (`270137793+woodruffw-bot@users.noreply.github.com`).
