# Instructions for coding agents

## Toolchain and bindings

- Use the versions in `rust-toolchain.toml`, CI, and `scripts/requirements-ci.txt`: Rust 1.90.0, tree-sitter CLI 0.27.1, and PowerShell 7.5.4.
- Install the CLI with `cargo install tree-sitter-cli --version 0.27.1 --locked --no-default-features --features qjs-rt`.
- Set `TREE_SITTER_JS_RUNTIME=native`. Generate with `tree-sitter generate --js-runtime native`, including temporary generations used for checks. Do not invoke Node, npm, or npx. GitHub Actions may use official actions that run Node internally.
- Enable only the Rust binding in `tree-sitter.json`. Keep `bindings/rust` as the sole binding directory. Commit the C parser sources required by the Rust crate.

## Editing and collaboration

- Inspect the working tree before editing. Preserve changes made by the user or other agents.
- Agree on file ownership before parallel edits. Coordinate changes to `grammar.js`, external token order in `src/scanner.c`, and generated files with one owner responsible for regeneration.
- Give simultaneous checkouts distinct `XDG_CACHE_HOME` directories. The CLI caches compiled parsers by language name; rebuild after source changes before trusting parse results.
- Keep agent instructions here. Write README, contributor, and design documentation for human readers; omit task history, agent coordination, and unsupported coverage claims.
- Prefer direct tool commands and small changes. Avoid custom wrappers, tracking, or other infrastructure without a concrete need.
- Format handwritten Rust, C, and Python with the configured tools. Preserve generated parser files, Unicode tables, and imported fixture bytes.

## Grammar and tests

- Test the grammar, scanner, CST, queries, and Rust binding. Do not add tests for test-only helpers or infrastructure.
- Use Microsoft's documentation, the pinned parser source, and PowerShell 7.5.4 `Parser::ParseInput` as syntax authorities. See `docs/grammar-sources.md` for links.
- Parse fixture code without executing it. Distinguish syntax from name resolution and compiler checks when choosing oracle cases.
- Add regression coverage for grammar or scanner changes. Assert relevant nodes, fields, and grouping, or compare corresponding source extents with PowerShell's AST. A successful parse alone does not establish a correct tree.
- Keep public nodes aligned with recognizable syntax constructs and use fields for their roles. A scalar field must identify one complete node, including composite arguments. Keep implementation helpers hidden; preserve meaningful argument lists, declaration bodies, pipeline boundaries, and distinct statement kinds. Update node-schema queries, oracle comparisons, corpus trees, and human tree-design documentation together.
- Verify public queries with `QueryCursor`, including nested and argument contexts. Declare a supertype only when runtime queries capture its promised members; `node-types.json` membership alone is insufficient.
- Derive corpus expectations independently from PowerShell. Inspect every snapshot update; retain intended precedence, argument boundaries, interpolation, and declaration structure. Update expected trees only when the new representation agrees with the oracle.
- Keep imported project files unmodified. Record the source repository, immutable revision, license, original path, and SHA-256 hash with their fixture collection.
- Exercise incomplete input and incremental edits when scanner or recovery behavior changes.
- Run `tree-sitter fuzz` directly. The pinned CLI can exit zero after reporting failures, so inspect its output as well as its status; CI checks the log. Preserve and fix failing cases instead of skipping them.

## Validation and publication

Before committing a finished chunk, run:

```sh
tree-sitter test --rebuild
TREE_SITTER_SEED=0 tree-sitter fuzz --rebuild --iterations 100 --edits 3
cargo test --locked
python3 scripts/generate-fixtures.py
python3 scripts/generate-fixtures.py --variables
scripts/check-generated.sh
scripts/lint.sh
```

- Set `PWSH_BIN` if the pinned PowerShell executable is not on `PATH`.
- `scripts/lint.sh` checks formatting, compiler warnings, shell/Python checks, and strict, pedantic zizmor. A local audit can set `ZIZMOR_OFFLINE=true`; CI also performs the online action-vulnerability audit.
- Follow the user's existing commit and push authorization. Coordinate publication with the task lead and keep commits focused on reviewable changes.
- Use the authenticated GitHub identity. The repository's automation identity is `woodruffw-bot` with email `270137793+woodruffw-bot@users.noreply.github.com`. Do not invent an author identity, print credentials, or commit them.
