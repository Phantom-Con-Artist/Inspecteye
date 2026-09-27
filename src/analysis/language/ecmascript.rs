//! Symbol and reference rules shared by TypeScript and JavaScript, whose
//! grammars use the same node kinds for the constructs they have in common.

use super::{
    child_texts, join_rel, operator_text, parent_dir, statements, strip_parens, terminator_for,
    ImportContext, ParsedFile,
};
use crate::analysis::logic::LogicNode;
use crate::intelligence::{
    is_declaration_name, ExtractedReference, ReferenceKind, ReferenceSpec, SymbolKind, SymbolSpec,
};
use tree_sitter::Node;

pub fn reference_specs(node: Node, parsed: &ParsedFile) -> Vec<ReferenceSpec> {
    match node.kind() {
        "import_statement" | "export_statement" => {
            let Some(source) = node.child_by_field_name("source") else {
                return Vec::new();
            };
            vec![
                ReferenceSpec::new(ReferenceKind::Import, unquote(parsed.text(source)))
                    .with_detail(imported_names(node, parsed)),
            ]
        }
        "call_expression" => {
            let Some(f) = node.child_by_field_name("function") else {
                return Vec::new();
            };
            match f.kind() {
                "identifier" if parsed.text(f) == "require" => {
                    let arg = node
                        .child_by_field_name("arguments")
                        .and_then(|a| a.named_child(0))
                        .filter(|a| a.kind() == "string");
                    arg.map(|a| ReferenceSpec::new(ReferenceKind::Import, unquote(parsed.text(a))))
                        .into_iter()
                        .collect()
                }
                "import" => Vec::new(),
                "identifier" => vec![ReferenceSpec::new(ReferenceKind::Call, parsed.text(f))],
                "member_expression" => {
                    let name = parsed.field_text(f, "property").unwrap_or("");
                    let object = parsed.field_text(f, "object").unwrap_or("");
                    vec![ReferenceSpec::new(ReferenceKind::Call, name)
                        .with_detail(vec![object.to_string()])]
                }
                _ => Vec::new(),
            }
        }
        "new_expression" => node
            .child_by_field_name("constructor")
            .filter(|c| c.kind() == "identifier")
            .map(|c| ReferenceSpec::new(ReferenceKind::Call, parsed.text(c)))
            .into_iter()
            .collect(),
        "extends_clause" => node
            .child_by_field_name("value")
            .filter(|v| v.kind() == "identifier")
            .map(|v| ReferenceSpec::new(ReferenceKind::Inherits, parsed.text(v)))
            .into_iter()
            .collect(),
        "implements_clause" | "extends_type_clause" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .map(|t| {
                    let name = t.child_by_field_name("name").unwrap_or(t);
                    ReferenceSpec::new(ReferenceKind::Implements, parsed.text(name))
                })
                .collect()
        }
        "type_identifier" if !is_declaration_name(node) => {
            let in_heritage = node.parent().is_some_and(|p| {
                matches!(p.kind(), "implements_clause" | "extends_type_clause")
                    || p.parent().is_some_and(|pp| {
                        matches!(pp.kind(), "implements_clause" | "extends_type_clause")
                    })
            });
            if in_heritage {
                Vec::new()
            } else {
                vec![ReferenceSpec::new(
                    ReferenceKind::TypeReference,
                    parsed.text(node),
                )]
            }
        }
        _ => Vec::new(),
    }
}

/// Names brought in by an import/re-export: `a` in `{ a as b }`, the default
/// import, or the namespace alias.
fn imported_names(node: Node, parsed: &ParsedFile) -> Vec<String> {
    let mut names = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "import_specifier" | "export_specifier" => {
                if let Some(name) = parsed.field_text(n, "name") {
                    names.push(name.to_string());
                }
                continue;
            }
            "import_clause" => {
                let mut cursor = n.walk();
                for c in n.named_children(&mut cursor) {
                    if c.kind() == "identifier" {
                        names.push("default".to_string());
                    }
                }
            }
            "string" => continue,
            _ => {}
        }
        let mut cursor = n.walk();
        stack.extend(n.named_children(&mut cursor));
    }
    names
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_string()
}

const EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "js", "jsx", "mjs", "cjs"];

/// Resolves relative specifiers (`./x`, `../y/z`) the way bundlers and the
/// TypeScript compiler do. Bare specifiers name packages and stay external.
/// `./x` and `../x`. Loader syntax (`./a.svg?raw`, `raw!./a`) is left out:
/// the bundler decides what those name.
pub fn is_relative_import(r: &ExtractedReference) -> bool {
    r.kind == ReferenceKind::Import
        && (r.target.starts_with("./") || r.target.starts_with("../"))
        && !r.target.contains(['?', '!', '*'])
}

pub fn resolve_import(r: &ExtractedReference, from: &str, ctx: &ImportContext) -> Vec<String> {
    if r.kind != ReferenceKind::Import || !r.target.starts_with('.') {
        return Vec::new();
    }
    let base = join_rel(parent_dir(from), &r.target);
    let mut candidates = vec![base.clone()];
    // ESM TypeScript imports name the emitted `.js` file.
    for (emitted, source) in [
        (".js", ".ts"),
        (".js", ".tsx"),
        (".mjs", ".mts"),
        (".jsx", ".tsx"),
    ] {
        if let Some(stem) = base.strip_suffix(emitted) {
            candidates.push(format!("{stem}{source}"));
        }
    }
    candidates.extend(EXTENSIONS.iter().map(|e| format!("{base}.{e}")));
    candidates.extend(EXTENSIONS.iter().map(|e| format!("{base}/index.{e}")));
    ctx.first_existing(candidates)
}

pub fn symbol_specs(
    node: Node,
    parsed: &ParsedFile,
    parent: Option<SymbolKind>,
) -> Vec<SymbolSpec> {
    let name = || parsed.field_text(node, "name").unwrap_or("").to_string();
    let kind = match node.kind() {
        "class_declaration" | "abstract_class_declaration" | "class" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        "type_alias_declaration" => SymbolKind::TypeAlias,
        "internal_module" | "module" => SymbolKind::Namespace,
        "function_declaration" | "generator_function_declaration" => SymbolKind::Function,
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            if name() == "constructor" {
                SymbolKind::Constructor
            } else {
                SymbolKind::Method
            }
        }
        "public_field_definition" | "field_definition" | "property_signature" => {
            SymbolKind::Property
        }
        "property_identifier" | "enum_assignment"
            if node.parent().is_some_and(|p| p.kind() == "enum_body") =>
        {
            let name = match node.kind() {
                "enum_assignment" => parsed.field_text(node, "name").unwrap_or(""),
                _ => parsed.text(node),
            };
            return vec![SymbolSpec::new(SymbolKind::Variant, name)];
        }
        "lexical_declaration" | "variable_declaration" => {
            return variable_specs(node, parsed, parent);
        }
        _ => return Vec::new(),
    };
    if kind == SymbolKind::Class && node.kind() == "class" && name().is_empty() {
        return Vec::new();
    }
    vec![SymbolSpec::new(kind, name()).with_modifiers(modifiers(node, parsed))]
}

/// `const f = () => {}` declares a function; a top-level `const X = …`
/// declares a constant. Local variables inside functions are not symbols.
fn variable_specs(node: Node, parsed: &ParsedFile, parent: Option<SymbolKind>) -> Vec<SymbolSpec> {
    if !is_module_level(node) || parent.is_some_and(|k| !k.is_container()) {
        return Vec::new();
    }
    let is_const = parsed.text(node).trim_start().starts_with("const");
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for decl in node.named_children(&mut cursor) {
        if decl.kind() != "variable_declarator" {
            continue;
        }
        let Some(name_node) = decl.child_by_field_name("name") else {
            continue;
        };
        if name_node.kind() != "identifier" {
            continue; // destructuring patterns
        }
        let is_function = decl.child_by_field_name("value").is_some_and(|v| {
            matches!(
                v.kind(),
                "arrow_function" | "function_expression" | "function" | "generator_function"
            )
        });
        let kind = if is_function {
            SymbolKind::Function
        } else if is_const {
            SymbolKind::Constant
        } else {
            continue;
        };
        out.push(
            SymbolSpec::new(kind, parsed.text(name_node)).with_modifiers(modifiers(node, parsed)),
        );
    }
    out
}

/// True when the declaration is not nested inside a function body.
fn is_module_level(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(p) = current {
        match p.kind() {
            "program" => return true,
            "statement_block" => {
                // Namespace bodies are statement blocks too.
                if p.parent()
                    .is_some_and(|pp| pp.kind() != "internal_module" && pp.kind() != "module")
                {
                    return false;
                }
            }
            "export_statement" | "internal_module" | "module" | "expression_statement" => {}
            _ => return false,
        }
        current = p.parent();
    }
    true
}

fn modifiers(node: Node, parsed: &ParsedFile) -> Vec<String> {
    let mut mods = child_texts(
        parsed,
        node,
        &[
            "accessibility_modifier",
            "static",
            "async",
            "abstract",
            "readonly",
            "override",
        ],
    );
    if node
        .parent()
        .is_some_and(|p| p.kind() == "export_statement")
    {
        mods.insert(0, "export".to_string());
    }
    mods
}

pub fn logic_view<'t>(node: Node<'t>, parsed: &ParsedFile) -> Option<LogicNode<'t>> {
    let field = |f: &str| node.child_by_field_name(f);
    match node.kind() {
        "if_statement" => Some(LogicNode::If {
            condition: strip_parens(field("condition")?),
            consequence: field("consequence")?,
            alternative: field("alternative").and_then(|e| e.named_child(0)),
        }),
        "while_statement" | "do_statement" => Some(LogicNode::Loop {
            condition: field("condition").map(strip_parens),
            body: field("body")?,
        }),
        "for_statement" => Some(LogicNode::Loop {
            // `for (;;)` has an empty condition.
            condition: field("condition").filter(|c| c.kind() != "empty_statement"),
            body: field("body")?,
        }),
        "for_in_statement" => Some(LogicNode::Loop {
            condition: field("right"),
            body: field("body")?,
        }),
        "statement_block" => Some(LogicNode::Block {
            statements: statements(node),
        }),
        "expression_statement" => match logic_view(node.named_child(0)?, parsed)? {
            view @ LogicNode::Assignment { .. } => Some(view),
            _ => None,
        },
        "assignment_expression" => Some(LogicNode::Assignment {
            target: field("left")?,
            value: field("right")?,
        }),
        "lexical_declaration" | "variable_declaration" if node.named_child_count() == 1 => {
            let decl = node.named_child(0)?;
            let name = decl.child_by_field_name("name")?;
            (name.kind() == "identifier").then(|| LogicNode::Declaration {
                name,
                value: decl.child_by_field_name("value"),
            })
        }
        "binary_expression" => Some(LogicNode::Comparison {
            left: field("left")?,
            operator: operator_text(node, parsed),
            right: field("right")?,
        }),
        "catch_clause" => Some(LogicNode::Catch {
            body: field("body")?,
        }),
        "true" => Some(LogicNode::BoolLiteral(true)),
        "false" => Some(LogicNode::BoolLiteral(false)),
        "empty_statement" => Some(LogicNode::Empty),
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "function"
        | "arrow_function"
        | "method_definition"
        | "class_declaration"
        | "class" => Some(LogicNode::FunctionBoundary),
        kind => terminator_for(kind),
    }
}
