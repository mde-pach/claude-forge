//! Prints tree-sitter-bash's tree for each command of a file, commands separated by a `---` line
//! (development aid).
fn main() {
    let path = std::env::args().nth(1).expect("a file");
    let text = std::fs::read_to_string(path).expect("readable");
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_bash::LANGUAGE.into())
        .expect("bash grammar");
    for src in text.split("\n---\n") {
        let t = p.parse(src, None).expect("a tree");
        println!("{src}\n  => {}\n", t.root_node().to_sexp());
    }
}
