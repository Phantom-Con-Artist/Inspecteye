use super::{
    child_texts, operator_text, statements, strip_parens, terminator_for, ImportContext,
    LanguageAnalyzer, ParsedFile,
};
use crate::analysis::logic::LogicNode;
use crate::intelligence::{
    is_declaration_name, ExtractedReference, ReferenceKind, ReferenceSpec, SymbolKind, SymbolSpec,
};
use std::path::Path;
use tree_sitter::Node;

pub struct CSharpAnalyzer;

impl LanguageAnalyzer for CSharpAnalyzer {
    fn language_id(&self) -> &'static str {
        "csharp"
    }

    fn version(&self) -> &'static str {
        "5"
    }

    fn grammar_for(&self, _path: &Path) -> tree_sitter::Language {
        tree_sitter_c_sharp::LANGUAGE.into()
    }

    fn reference_specs(&self, node: Node, parsed: &ParsedFile) -> Vec<ReferenceSpec> {
        match node.kind() {
            "using_directive" => {
                // `using X = A.B;` names the target in a field; plain usings do not.
                let mut cursor = node.walk();
                let target = node
                    .named_children(&mut cursor)
                    .filter(|c| matches!(c.kind(), "qualified_name" | "identifier"))
                    .last();
                target
                    .map(|t| ReferenceSpec::new(ReferenceKind::Import, parsed.text(t)))
                    .into_iter()
                    .collect()
            }
            "invocation_expression" => {
                let Some(f) = node.child_by_field_name("function") else {
                    return Vec::new();
                };
                let (name_node, qualifier) = match f.kind() {
                    "identifier" | "generic_name" => (f, None),
                    "member_access_expression" => match f.child_by_field_name("name") {
                        Some(n) => (
                            n,
                            f.child_by_field_name("expression").map(|e| parsed.text(e)),
                        ),
                        None => return Vec::new(),
                    },
                    _ => return Vec::new(),
                };
                let detail = qualifier.map(|q| vec![q.to_string()]).unwrap_or_default();
                vec![
                    ReferenceSpec::new(ReferenceKind::Call, simple_name(name_node, parsed))
                        .with_detail(detail),
                ]
            }
            "object_creation_expression" => node
                .child_by_field_name("type")
                .map(|t| ReferenceSpec::new(ReferenceKind::Call, simple_name(t, parsed)))
                .into_iter()
                .collect(),
            // Classes and interfaces share one base list; resolution decides
            // which relationship each entry is once the target is known.
            "base_list" => {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .map(|b| ReferenceSpec::new(ReferenceKind::Inherits, simple_name(b, parsed)))
                    .collect()
            }
            "identifier" | "generic_name" | "qualified_name"
                if is_type_position(node) && !is_declaration_name(node) =>
            {
                vec![ReferenceSpec::new(
                    ReferenceKind::TypeReference,
                    simple_name(node, parsed),
                )]
            }
            _ => Vec::new(),
        }
    }

    fn logic_view<'t>(&self, node: Node<'t>, parsed: &ParsedFile) -> Option<LogicNode<'t>> {
        let field = |f: &str| node.child_by_field_name(f);
        match node.kind() {
            "if_statement" => Some(LogicNode::If {
                condition: strip_parens(field("condition")?),
                consequence: field("consequence")?,
                alternative: field("alternative"),
            }),
            "while_statement" | "do_statement" | "for_statement" => Some(LogicNode::Loop {
                condition: field("condition"),
                body: field("body")?,
            }),
            "foreach_statement" => Some(LogicNode::Loop {
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
            "assignment_expression" => Some(LogicNode::Assignment {
                target: field("left")?,
                value: field("right")?,
            }),
            "local_declaration_statement" => {
                let decl = node.named_child(0)?;
                let mut cursor = decl.walk();
                let declarators: Vec<Node> = decl
                    .named_children(&mut cursor)
                    .filter(|c| c.kind() == "variable_declarator")
                    .collect();
                let [single] = declarators.as_slice() else {
                    return None;
                };
                let name = single.child_by_field_name("name")?;
                let mut c2 = single.walk();
                let value = single
                    .named_children(&mut c2)
                    .filter(|c| c.id() != name.id())
                    .last();
                Some(LogicNode::Declaration { name, value })
            }
            "binary_expression" => Some(LogicNode::Comparison {
                left: field("left")?,
                operator: operator_text(node, parsed),
                right: field("right")?,
            }),
            "catch_clause" => Some(LogicNode::Catch {
                body: field("body")?,
            }),
            "boolean_literal" => Some(LogicNode::BoolLiteral(parsed.text(node) == "true")),
            "empty_statement" => Some(LogicNode::Empty),
            "method_declaration"
            | "local_function_statement"
            | "lambda_expression"
            | "anonymous_method_expression"
            | "class_declaration" => Some(LogicNode::FunctionBoundary),
            kind => terminator_for(kind),
        }
    }

    /// A `using` names a namespace; every project file declaring that
    /// namespace is a candidate. This is an inference, not a file-level fact.
    fn resolve_import(
        &self,
        r: &ExtractedReference,
        _from: &str,
        ctx: &ImportContext,
    ) -> Vec<String> {
        if r.kind != ReferenceKind::Import {
            return Vec::new();
        }
        ctx.namespaces.get(&r.target).cloned().unwrap_or_default()
    }

    fn symbol_specs(
        &self,
        node: Node,
        parsed: &ParsedFile,
        _parent: Option<SymbolKind>,
    ) -> Vec<SymbolSpec> {
        let kind = match node.kind() {
            "namespace_declaration" => SymbolKind::Namespace,
            "file_scoped_namespace_declaration" => {
                let name = parsed.field_text(node, "name").unwrap_or("");
                let mut spec = SymbolSpec::new(SymbolKind::Namespace, name);
                spec.scopes_following_siblings = true;
                return vec![spec];
            }
            "class_declaration" | "record_declaration" => SymbolKind::Class,
            "interface_declaration" => SymbolKind::Interface,
            "struct_declaration" | "record_struct_declaration" => SymbolKind::Struct,
            "enum_declaration" => SymbolKind::Enum,
            "enum_member_declaration" => SymbolKind::Variant,
            "delegate_declaration" => SymbolKind::TypeAlias,
            "method_declaration" | "operator_declaration" => SymbolKind::Method,
            "constructor_declaration" => SymbolKind::Constructor,
            "property_declaration" | "indexer_declaration" | "event_declaration" => {
                SymbolKind::Property
            }
            "field_declaration" | "event_field_declaration" => {
                return field_specs(node, parsed);
            }
            _ => return Vec::new(),
        };
        let name = match node.kind() {
            "indexer_declaration" => "this[]",
            _ => parsed.field_text(node, "name").unwrap_or(""),
        };
        vec![SymbolSpec::new(kind, name).with_modifiers(modifiers(node, parsed))]
    }
}

/// `int a, b;` declares two fields; `const` fields are constants.
fn field_specs(node: Node, parsed: &ParsedFile) -> Vec<SymbolSpec> {
    let mods = modifiers(node, parsed);
    let kind = if mods.iter().any(|m| m == "const") {
        SymbolKind::Constant
    } else if node.kind() == "event_field_declaration" {
        SymbolKind::Property
    } else {
        SymbolKind::Field
    };
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for decl in node.named_children(&mut cursor) {
        if decl.kind() != "variable_declaration" {
            continue;
        }
        let mut c2 = decl.walk();
        for var in decl.named_children(&mut c2) {
            if var.kind() == "variable_declarator" {
                if let Some(name) = parsed.field_text(var, "name") {
                    out.push(SymbolSpec::new(kind, name).with_modifiers(mods.clone()));
                }
            }
        }
    }
    out
}

fn modifiers(node: Node, parsed: &ParsedFile) -> Vec<String> {
    child_texts(parsed, node, &["modifier"])
}

/// The unqualified, non-generic name: `A.B.Foo<int>` -> `Foo`.
fn simple_name(node: Node, parsed: &ParsedFile) -> String {
    let text = parsed.text(node);
    let no_generics = text.split('<').next().unwrap_or(text);
    no_generics
        .rsplit('.')
        .next()
        .unwrap_or(no_generics)
        .trim()
        .to_string()
}

/// True when the node is written where a type is expected.
fn is_type_position(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    // `new T()` is recorded as a call (instantiation), not a type use.
    if parent.kind() == "object_creation_expression" {
        return false;
    }
    let in_field = ["type", "returns"].iter().any(|f| {
        parent
            .child_by_field_name(f)
            .is_some_and(|t| t.id() == node.id())
    });
    in_field || parent.kind() == "type_argument_list"
}
