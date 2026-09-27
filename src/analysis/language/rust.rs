use super::{
    join_rel, operator_text, parent_dir, statements, terminator_for, ImportContext,
    LanguageAnalyzer, ParsedFile,
};
use crate::analysis::logic::{LogicNode, TerminatorKind};
use crate::intelligence::{
    is_declaration_name, ExtractedReference, ReferenceKind, ReferenceSpec, SymbolKind, SymbolSpec,
};
use std::path::Path;
use tree_sitter::Node;

pub struct RustAnalyzer;

impl LanguageAnalyzer for RustAnalyzer {
    fn language_id(&self) -> &'static str {
        "rust"
    }

    fn version(&self) -> &'static str {
        "4"
    }

    fn grammar_for(&self, _path: &Path) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn symbol_specs(
        &self,
        node: Node,
        parsed: &ParsedFile,
        parent: Option<SymbolKind>,
    ) -> Vec<SymbolSpec> {
        let name = || parsed.field_text(node, "name").unwrap_or("").to_string();
        let kind = match node.kind() {
            "mod_item" => SymbolKind::Module,
            "struct_item" | "union_item" => SymbolKind::Struct,
            "enum_item" => SymbolKind::Enum,
            "trait_item" => SymbolKind::Trait,
            "type_item" => SymbolKind::TypeAlias,
            "const_item" | "static_item" => SymbolKind::Constant,
            "macro_definition" => SymbolKind::Macro,
            "enum_variant" => SymbolKind::Variant,
            "field_declaration" if parent == Some(SymbolKind::Struct) => SymbolKind::Field,
            "function_item" | "function_signature_item" => match parent {
                Some(SymbolKind::Impl) | Some(SymbolKind::Trait) => SymbolKind::Method,
                _ => SymbolKind::Function,
            },
            "impl_item" => {
                let ty = parsed.field_text(node, "type").unwrap_or("?");
                let label = match parsed.field_text(node, "trait") {
                    Some(tr) => format!("impl {tr} for {ty}"),
                    None => format!("impl {ty}"),
                };
                return vec![SymbolSpec::new(SymbolKind::Impl, label)];
            }
            _ => return Vec::new(),
        };
        let mut modifiers = Vec::new();
        let mut cursor = node.walk();
        for c in node.children(&mut cursor) {
            match c.kind() {
                "visibility_modifier" => modifiers.push(parsed.text(c).to_string()),
                "function_modifiers" => {
                    modifiers.extend(parsed.text(c).split_whitespace().map(str::to_string))
                }
                _ => {}
            }
        }
        vec![SymbolSpec::new(kind, name()).with_modifiers(modifiers)]
    }

    fn reference_specs(&self, node: Node, parsed: &ParsedFile) -> Vec<ReferenceSpec> {
        match node.kind() {
            "use_declaration" => {
                let mut paths = Vec::new();
                if let Some(arg) = node.child_by_field_name("argument") {
                    use_paths(arg, parsed, "", &mut paths);
                }
                paths
                    .into_iter()
                    .map(|p| ReferenceSpec::new(ReferenceKind::Import, p))
                    .collect()
            }
            "mod_item" if node.child_by_field_name("body").is_none() => {
                let name = parsed.field_text(node, "name").unwrap_or("");
                vec![ReferenceSpec::new(ReferenceKind::ModuleDecl, name)]
            }
            "call_expression" => call_spec(node, parsed).into_iter().collect(),
            "impl_item" => match node.child_by_field_name("trait") {
                Some(tr) => vec![ReferenceSpec::new(
                    ReferenceKind::Implements,
                    last_segment(parsed.text(tr)),
                )],
                None => Vec::new(),
            },
            "type_identifier" if !is_declaration_name(node) && !is_impl_trait(node) => {
                vec![ReferenceSpec::new(
                    ReferenceKind::TypeReference,
                    parsed.text(node),
                )]
            }
            _ => Vec::new(),
        }
    }

    fn logic_view<'t>(&self, node: Node<'t>, parsed: &ParsedFile) -> Option<LogicNode<'t>> {
        let field = |f: &str| node.child_by_field_name(f);
        match node.kind() {
            "if_expression" => Some(LogicNode::If {
                condition: field("condition")?,
                consequence: field("consequence")?,
                alternative: field("alternative").and_then(|e| e.named_child(0)),
            }),
            "while_expression" | "for_expression" => Some(LogicNode::Loop {
                condition: field("condition").or_else(|| field("value")),
                body: field("body")?,
            }),
            "loop_expression" => Some(LogicNode::Loop {
                condition: None,
                body: field("body")?,
            }),
            "block" => Some(LogicNode::Block {
                statements: statements(node),
            }),
            "expression_statement" => {
                let inner = node.named_child(0)?;
                match self.logic_view(inner, parsed)? {
                    view @ (LogicNode::Terminator { .. } | LogicNode::Assignment { .. }) => {
                        Some(view)
                    }
                    _ => None,
                }
            }
            "macro_invocation" => {
                let name = parsed.field_text(node, "macro").unwrap_or("");
                matches!(name, "panic" | "unreachable" | "todo" | "unimplemented").then_some(
                    LogicNode::Terminator {
                        kind: TerminatorKind::Throw,
                        conditional: false,
                    },
                )
            }
            "try_expression" => Some(LogicNode::Terminator {
                kind: TerminatorKind::Return,
                conditional: true,
            }),
            "binary_expression" => Some(LogicNode::Comparison {
                left: field("left")?,
                operator: operator_text(node, parsed),
                right: field("right")?,
            }),
            "assignment_expression" => Some(LogicNode::Assignment {
                target: field("left")?,
                value: field("right")?,
            }),
            "let_declaration" => {
                let value = field("value");
                match field("pattern") {
                    None => value.map(|v| LogicNode::Discard { value: v }),
                    Some(p) if parsed.text(p) == "_" => {
                        value.map(|v| LogicNode::Discard { value: v })
                    }
                    Some(p) if p.kind() == "identifier" => {
                        Some(LogicNode::Declaration { name: p, value })
                    }
                    _ => None,
                }
            }
            "boolean_literal" => Some(LogicNode::BoolLiteral(parsed.text(node) == "true")),
            "empty_statement" => Some(LogicNode::Empty),
            "function_item" | "closure_expression" | "impl_item" | "trait_item" => {
                Some(LogicNode::FunctionBoundary)
            }
            kind => terminator_for(kind),
        }
    }

    /// `mod name;` must be backed by `name.rs` or `name/mod.rs`.
    fn is_relative_import(&self, r: &ExtractedReference) -> bool {
        r.kind == ReferenceKind::ModuleDecl
    }

    fn resolve_import(
        &self,
        r: &ExtractedReference,
        from: &str,
        ctx: &ImportContext,
    ) -> Vec<String> {
        match r.kind {
            ReferenceKind::ModuleDecl => {
                let dir = module_dir(from);
                ctx.first_existing([
                    join_rel(&dir, &format!("{}.rs", r.target)),
                    join_rel(&dir, &format!("{}/mod.rs", r.target)),
                ])
            }
            ReferenceKind::Import => {
                let segments: Vec<&str> = r.target.split("::").collect();
                let (base, rest) = match segments.first().copied() {
                    Some("crate") => (crate_root(from, ctx), &segments[1..]),
                    Some("self") => (module_dir(from), &segments[1..]),
                    Some("super") => {
                        let mut dir = module_dir(from);
                        let mut i = 0;
                        while segments.get(i) == Some(&"super") {
                            dir = parent_dir(&dir).to_string();
                            i += 1;
                        }
                        (dir, &segments[i..])
                    }
                    // Paths starting with a sibling module name.
                    _ => (module_dir(from), &segments[..]),
                };
                // The longest module prefix that exists as a file wins; the
                // remaining segments name items inside it.
                for len in (1..=rest.len()).rev() {
                    let module = rest[..len].join("/");
                    let found = ctx.first_existing([
                        join_rel(&base, &format!("{module}.rs")),
                        join_rel(&base, &format!("{module}/mod.rs")),
                    ]);
                    if !found.is_empty() {
                        return found;
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

fn call_spec(node: Node, parsed: &ParsedFile) -> Option<ReferenceSpec> {
    let mut f = node.child_by_field_name("function")?;
    if f.kind() == "generic_function" {
        f = f.child_by_field_name("function")?;
    }
    let (name, qualifier) = match f.kind() {
        "identifier" => (parsed.text(f), None),
        "scoped_identifier" => (
            parsed.field_text(f, "name").unwrap_or(""),
            f.child_by_field_name("path").map(|p| parsed.text(p)),
        ),
        "field_expression" => (parsed.field_text(f, "field").unwrap_or(""), Some(".")),
        _ => return None,
    };
    let detail = qualifier.map(|q| vec![q.to_string()]).unwrap_or_default();
    Some(ReferenceSpec::new(ReferenceKind::Call, name).with_detail(detail))
}

fn is_impl_trait(node: Node) -> bool {
    node.parent().is_some_and(|p| {
        p.kind() == "impl_item"
            && p.child_by_field_name("trait")
                .is_some_and(|t| t.id() == node.id())
    })
}

/// Directory holding the child modules of the module defined in `path`:
/// `src/lib.rs` -> `src`, `src/a/mod.rs` -> `src/a`, `src/a.rs` -> `src/a`.
fn module_dir(path: &str) -> String {
    let dir = parent_dir(path);
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".rs");
    if matches!(stem, "lib" | "main" | "mod") {
        dir.to_string()
    } else {
        join_rel(dir, stem)
    }
}

/// Directory of the crate root (`lib.rs` / `main.rs`) that owns `path`.
fn crate_root(path: &str, ctx: &ImportContext) -> String {
    let mut dir = parent_dir(path).to_string();
    loop {
        if ["lib.rs", "main.rs"]
            .iter()
            .any(|f| ctx.paths.contains(&join_rel(&dir, f)))
        {
            return dir;
        }
        if dir.is_empty() {
            return parent_dir(path).to_string();
        }
        dir = parent_dir(&dir).to_string();
    }
}

/// Flattens a `use` argument into full paths: `a::{b, c::d}` -> `a::b`, `a::c::d`.
fn use_paths(node: Node, parsed: &ParsedFile, prefix: &str, out: &mut Vec<String>) {
    let join = |p: &str, s: &str| {
        if p.is_empty() {
            s.to_string()
        } else {
            format!("{p}::{s}")
        }
    };
    match node.kind() {
        "scoped_use_list" => {
            let base = node
                .child_by_field_name("path")
                .map(|p| join(prefix, parsed.text(p)))
                .unwrap_or_else(|| prefix.to_string());
            if let Some(list) = node.child_by_field_name("list") {
                use_paths(list, parsed, &base, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                use_paths(child, parsed, prefix, out);
            }
        }
        "use_as_clause" => {
            if let Some(p) = node.child_by_field_name("path") {
                out.push(join(prefix, parsed.text(p)));
            }
        }
        "use_wildcard" => {
            let text = parsed
                .text(node)
                .trim_end_matches('*')
                .trim_end_matches("::");
            out.push(join(prefix, text));
        }
        _ => out.push(join(prefix, parsed.text(node))),
    }
}

fn last_segment(text: &str) -> &str {
    let no_generics = text.split('<').next().unwrap_or(text);
    no_generics
        .rsplit("::")
        .next()
        .unwrap_or(no_generics)
        .trim()
}
