use super::{
    child_texts, join_rel, parent_dir, statements, terminator_for, ImportContext, LanguageAnalyzer,
    ParsedFile,
};
use crate::analysis::logic::LogicNode;
use crate::intelligence::{
    ExtractedReference, ReferenceKind, ReferenceSpec, SymbolKind, SymbolSpec,
};
use std::path::Path;
use tree_sitter::Node;

pub struct PythonAnalyzer;

impl LanguageAnalyzer for PythonAnalyzer {
    fn language_id(&self) -> &'static str {
        "python"
    }

    fn version(&self) -> &'static str {
        "4"
    }

    fn grammar_for(&self, _path: &Path) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn reference_specs(&self, node: Node, parsed: &ParsedFile) -> Vec<ReferenceSpec> {
        match node.kind() {
            "import_statement" => {
                let mut cursor = node.walk();
                node.children_by_field_name("name", &mut cursor)
                    .map(|n| {
                        let module = match n.kind() {
                            "aliased_import" => parsed.field_text(n, "name").unwrap_or(""),
                            _ => parsed.text(n),
                        };
                        ReferenceSpec::new(ReferenceKind::Import, module)
                    })
                    .collect()
            }
            "import_from_statement" => {
                let module = parsed.field_text(node, "module_name").unwrap_or("");
                let mut cursor = node.walk();
                let names = node
                    .children_by_field_name("name", &mut cursor)
                    .map(|n| match n.kind() {
                        "aliased_import" => parsed.field_text(n, "name").unwrap_or("").to_string(),
                        _ => parsed.text(n).to_string(),
                    })
                    .collect();
                vec![ReferenceSpec::new(ReferenceKind::Import, module).with_detail(names)]
            }
            "call" => {
                let Some(f) = node.child_by_field_name("function") else {
                    return Vec::new();
                };
                match f.kind() {
                    "identifier" => vec![ReferenceSpec::new(ReferenceKind::Call, parsed.text(f))],
                    "attribute" => {
                        let name = parsed.field_text(f, "attribute").unwrap_or("");
                        let object = parsed.field_text(f, "object").unwrap_or("");
                        vec![ReferenceSpec::new(ReferenceKind::Call, name)
                            .with_detail(vec![object.to_string()])]
                    }
                    _ => Vec::new(),
                }
            }
            "class_definition" => {
                let Some(bases) = node.child_by_field_name("superclasses") else {
                    return Vec::new();
                };
                let mut cursor = bases.walk();
                bases
                    .named_children(&mut cursor)
                    .filter_map(|b| match b.kind() {
                        "identifier" => Some(parsed.text(b)),
                        "attribute" => parsed.field_text(b, "attribute"),
                        _ => None,
                    })
                    .map(|name| ReferenceSpec::new(ReferenceKind::Inherits, name))
                    .collect()
            }
            // Type annotations: `x: Foo`, `-> Bar`.
            "type" => {
                let mut cursor = node.walk();
                let first = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "identifier");
                first
                    .map(|c| ReferenceSpec::new(ReferenceKind::TypeReference, parsed.text(c)))
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    fn logic_view<'t>(&self, node: Node<'t>, parsed: &ParsedFile) -> Option<LogicNode<'t>> {
        let field = |f: &str| node.child_by_field_name(f);
        match node.kind() {
            // `if` and each `elif` form a chain; the alternative of a branch
            // is the next elif/else clause of the enclosing `if`.
            "if_statement" => Some(LogicNode::If {
                condition: field("condition")?,
                consequence: field("consequence")?,
                alternative: field("alternative").map(else_body),
            }),
            "elif_clause" => {
                let mut next = node.next_named_sibling();
                while let Some(n) = next {
                    if matches!(n.kind(), "elif_clause" | "else_clause") {
                        break;
                    }
                    next = n.next_named_sibling();
                }
                Some(LogicNode::If {
                    condition: field("condition")?,
                    consequence: field("consequence")?,
                    alternative: next.map(else_body),
                })
            }
            "while_statement" => Some(LogicNode::Loop {
                condition: field("condition"),
                body: field("body")?,
            }),
            "for_statement" => Some(LogicNode::Loop {
                condition: field("right"),
                body: field("body")?,
            }),
            "block" => Some(LogicNode::Block {
                statements: statements(node),
            }),
            "expression_statement" => match self.logic_view(node.named_child(0)?, parsed)? {
                view @ LogicNode::Assignment { .. } => Some(view),
                _ => None,
            },
            "assignment" => Some(LogicNode::Assignment {
                target: field("left")?,
                value: field("right")?,
            }),
            "comparison_operator" if node.named_child_count() == 2 => {
                let mut cursor = node.walk();
                let operator = node
                    .children_by_field_name("operators", &mut cursor)
                    .map(|o| parsed.text(o))
                    .collect::<Vec<_>>()
                    .join(" ");
                Some(LogicNode::Comparison {
                    left: node.named_child(0)?,
                    operator,
                    right: node.named_child(1)?,
                })
            }
            "except_clause" => {
                let mut cursor = node.walk();
                let body = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "block")?;
                Some(LogicNode::Catch { body })
            }
            "true" => Some(LogicNode::BoolLiteral(true)),
            "false" => Some(LogicNode::BoolLiteral(false)),
            "pass_statement" => Some(LogicNode::Empty),
            "function_definition" | "lambda" | "class_definition" => {
                Some(LogicNode::FunctionBoundary)
            }
            kind => terminator_for(kind),
        }
    }

    /// Relative imports resolve against the importing file's package;
    /// absolute ones against the project root and each ancestor directory
    /// (covering `src/` layouts).
    /// `from . import x`, `from .mod import y`.
    fn is_relative_import(&self, r: &ExtractedReference) -> bool {
        r.kind == ReferenceKind::Import && r.target.starts_with('.')
    }

    fn resolve_import(
        &self,
        r: &ExtractedReference,
        from: &str,
        ctx: &ImportContext,
    ) -> Vec<String> {
        if r.kind != ReferenceKind::Import {
            return Vec::new();
        }
        let dots = r.target.chars().take_while(|c| *c == '.').count();
        let module_path = r.target[dots..].replace('.', "/");

        let roots: Vec<String> = if dots > 0 {
            let mut dir = parent_dir(from).to_string();
            for _ in 1..dots {
                dir = parent_dir(&dir).to_string();
            }
            vec![dir]
        } else {
            let mut roots = vec![String::new()];
            let mut dir = parent_dir(from).to_string();
            while !dir.is_empty() {
                roots.push(dir.clone());
                dir = parent_dir(&dir).to_string();
            }
            roots
        };

        let module_files = |base: &str| {
            [
                join_rel(base, &format!("{module_path}.py")),
                join_rel(base, &format!("{module_path}/__init__.py")),
            ]
        };
        for root in &roots {
            // `from . import x` / `from pkg import submodule`: names may be modules.
            let dir = join_rel(root, &module_path);
            let mut found: Vec<String> = r
                .detail
                .iter()
                .flat_map(|name| ctx.first_existing([join_rel(&dir, &format!("{name}.py"))]))
                .collect();
            if !module_path.is_empty() {
                found.extend(ctx.first_existing(module_files(root)));
            } else if found.is_empty() {
                found.extend(ctx.first_existing([join_rel(root, "__init__.py")]));
            }
            if !found.is_empty() {
                found.sort();
                found.dedup();
                return found;
            }
        }
        Vec::new()
    }

    fn symbol_specs(
        &self,
        node: Node,
        parsed: &ParsedFile,
        parent: Option<SymbolKind>,
    ) -> Vec<SymbolSpec> {
        match node.kind() {
            "class_definition" => {
                let name = parsed.field_text(node, "name").unwrap_or("");
                vec![SymbolSpec::new(SymbolKind::Class, name)
                    .with_modifiers(decorators(node, parsed))]
            }
            "function_definition" => {
                let name = parsed.field_text(node, "name").unwrap_or("");
                let kind = match parent {
                    Some(SymbolKind::Class) if name == "__init__" => SymbolKind::Constructor,
                    Some(SymbolKind::Class) => SymbolKind::Method,
                    _ => SymbolKind::Function,
                };
                let mut mods = child_texts(parsed, node, &["async"]);
                mods.extend(decorators(node, parsed));
                vec![SymbolSpec::new(kind, name).with_modifiers(mods)]
            }
            // Module-level UPPER_CASE assignments are constants by convention;
            // class-level assignments are class attributes.
            "expression_statement" => {
                let Some(assign) = node.named_child(0).filter(|n| n.kind() == "assignment") else {
                    return Vec::new();
                };
                let Some(left) = assign
                    .child_by_field_name("left")
                    .filter(|l| l.kind() == "identifier")
                else {
                    return Vec::new();
                };
                let name = parsed.text(left);
                match (parent, node.parent().map(|p| p.kind())) {
                    (None, Some("module")) if is_upper_snake(name) => {
                        vec![SymbolSpec::new(SymbolKind::Constant, name)]
                    }
                    (Some(SymbolKind::Class), Some("block")) => {
                        vec![SymbolSpec::new(SymbolKind::Field, name)]
                    }
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }
}

/// Decorator names on a decorated definition, e.g. `@property`.
fn decorators(node: Node, parsed: &ParsedFile) -> Vec<String> {
    match node.parent().filter(|p| p.kind() == "decorated_definition") {
        Some(p) => child_texts(parsed, p, &["decorator"]),
        None => Vec::new(),
    }
}

fn is_upper_snake(name: &str) -> bool {
    name.chars().any(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// An `else:` clause stands for its body; an `elif` clause for itself.
fn else_body(node: Node) -> Node {
    if node.kind() == "else_clause" {
        node.child_by_field_name("body").unwrap_or(node)
    } else {
        node
    }
}
