//! Dev helper: prints the tree-sitter S-expression for a file.
//! Usage: cargo run --example sexp -- <language> <file>
use inspecteye::analysis::language::LanguageRegistry;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (lang, path) = (&args[1], &args[2]);
    let src = std::fs::read_to_string(path).expect("read file");
    let parsed = LanguageRegistry::default()
        .parse(lang, Path::new(path), src)
        .expect("parse");
    let mut out = String::new();
    print_node(parsed.root(), &parsed.source, 0, None, &mut out);
    print!("{out}");
}

fn print_node(
    node: tree_sitter::Node,
    src: &str,
    depth: usize,
    field: Option<&str>,
    out: &mut String,
) {
    if !node.is_named() {
        return;
    }
    let text = node.utf8_text(src.as_bytes()).unwrap_or("");
    let short: String = text.lines().next().unwrap_or("").chars().take(40).collect();
    out.push_str(&format!(
        "{}{}{} [{}] {:?}\n",
        "  ".repeat(depth),
        field.map(|f| format!("{f}: ")).unwrap_or_default(),
        node.kind(),
        node.start_position().row + 1,
        short
    ));
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for (i, child) in children.into_iter().enumerate() {
        print_node(
            child,
            src,
            depth + 1,
            node.field_name_for_child(i as u32),
            out,
        );
    }
}
