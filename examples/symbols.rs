//! Dev helper: prints extracted symbols for a file.
//! Usage: cargo run --example symbols -- <language> <file>
use inspecteye::analysis::language::LanguageRegistry;
use inspecteye::intelligence::extract_symbols;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (lang, path) = (&args[1], &args[2]);
    let src = std::fs::read_to_string(path).expect("read file");
    let registry = LanguageRegistry::default();
    let parsed = registry.parse(lang, Path::new(path), src).expect("parse");
    for s in extract_symbols(&parsed, registry.get(lang).unwrap()) {
        println!(
            "{:>3}-{:<3} {:<40} {:?}  | {}",
            s.range.start_line, s.range.end_line, s.identity, s.modifiers, s.signature
        );
    }
}
