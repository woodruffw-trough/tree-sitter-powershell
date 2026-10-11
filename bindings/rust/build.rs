fn main() {
    let mut build = cc::Build::new();
    build.include("src").file("src/parser.c").warnings(false);
    if std::path::Path::new("src/scanner.c").exists() {
        build.file("src/scanner.c");
        println!("cargo:rerun-if-changed=src/scanner.c");
        println!("cargo:rerun-if-changed=src/variable_chars.h");
    }
    build.compile("tree-sitter-powershell");
    println!("cargo:rerun-if-changed=src/parser.c");
    println!("cargo:rerun-if-changed=src/tree_sitter/parser.h");
}
