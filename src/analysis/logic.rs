//! Deterministic low-level logic checks.
//!
//! Each language maps its grammar onto a small normalized view
//! ([`LogicNode`]); the rules below are written once against that view. The
//! rules look for code whose meaning can be read from the syntax alone (a
//! condition that is a literal, a statement after `return`). They do not
//! judge business logic, and every finding explains what was observed.

use crate::analysis::language::{LanguageAnalyzer, ParsedFile};
use crate::issues::{
    render_excerpt, Certainty, Diagnostic, DiagnosticCategory, Severity, SourceRange,
};
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminatorKind {
    Return,
    Throw,
    Break,
    Continue,
}

impl TerminatorKind {
    fn as_str(&self) -> &'static str {
        match self {
            TerminatorKind::Return => "return",
            TerminatorKind::Throw => "throw",
            TerminatorKind::Break => "break",
            TerminatorKind::Continue => "continue",
        }
    }
}

/// Language-independent view of a syntax node, as far as logic checks care.
#[derive(Debug, Clone)]
pub enum LogicNode<'t> {
    /// `alternative` is the else branch: another `If` node or a body.
    If {
        condition: Node<'t>,
        consequence: Node<'t>,
        alternative: Option<Node<'t>>,
    },
    /// `condition` is `None` for loops without one (`loop {}`, `for (;;)`).
    Loop {
        condition: Option<Node<'t>>,
        body: Node<'t>,
    },
    Block {
        statements: Vec<Node<'t>>,
    },
    /// `conditional` exits (Rust `?`) may or may not leave.
    Terminator {
        kind: TerminatorKind,
        conditional: bool,
    },
    Comparison {
        left: Node<'t>,
        operator: String,
        right: Node<'t>,
    },
    Assignment {
        target: Node<'t>,
        value: Node<'t>,
    },
    Declaration {
        name: Node<'t>,
        value: Option<Node<'t>>,
    },
    Catch {
        body: Node<'t>,
    },
    /// A value deliberately thrown away (`let _ = f();`).
    Discard {
        value: Node<'t>,
    },
    BoolLiteral(bool),
    /// A statement that does nothing (`pass`, `;`).
    Empty,
    /// A nested function, lambda or class: control flow does not cross it.
    FunctionBoundary,
}

const COMPARISON_OPERATORS: &[&str] = &[
    "==", "!=", "===", "!==", "<", ">", "<=", ">=", "is", "is not",
];

pub fn logic_diagnostics(parsed: &ParsedFile, lang: &dyn LanguageAnalyzer) -> Vec<Diagnostic> {
    let mut checker = Checker {
        parsed,
        lang,
        out: Vec::new(),
    };
    let mut stack = vec![parsed.root()];
    while let Some(node) = stack.pop() {
        if !node.has_error() {
            checker.check(node);
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    checker
        .out
        .sort_by_key(|d| (d.range.start_byte, d.rule.clone()));
    checker.out
}

struct Checker<'a> {
    parsed: &'a ParsedFile,
    lang: &'a dyn LanguageAnalyzer,
    out: Vec<Diagnostic>,
}

impl<'a> Checker<'a> {
    fn view<'t>(&self, node: Node<'t>) -> Option<LogicNode<'t>>
    where
        'a: 't,
    {
        self.lang.logic_view(node, self.parsed)
    }

    fn check(&mut self, node: Node) {
        match self.view(node) {
            Some(LogicNode::If {
                condition,
                consequence,
                alternative,
            }) => {
                self.constant_condition(condition, "if");
                self.assignment_in_condition(condition);
                self.empty_if(node, consequence, alternative);
                self.identical_branches(node, consequence, alternative);
                if !self.is_else_if(node) {
                    self.duplicate_conditions(node);
                }
            }
            Some(LogicNode::Loop { condition, body }) => {
                if let Some(c) = condition {
                    self.constant_false_loop(c);
                    self.assignment_in_condition(c);
                }
                self.loop_without_exit(node, condition, body);
            }
            Some(LogicNode::Block { statements }) => {
                self.unreachable(&statements);
                self.overwritten_before_read(&statements);
            }
            Some(LogicNode::Comparison {
                left,
                operator,
                right,
            }) => self.self_comparison(node, left, &operator, right),
            // Statement wrappers view as their inner assignment (for block
            // rules); report on the assignment itself only.
            Some(LogicNode::Assignment { target, value }) if target.parent() == Some(node) => {
                self.self_assignment(node, target, value)
            }
            Some(LogicNode::Catch { body }) => self.empty_catch(node, body),
            Some(LogicNode::Discard { value }) => self.discarded_result(node, value),
            _ => {}
        }
    }

    // --- rules -------------------------------------------------------------

    fn constant_condition(&mut self, condition: Node, construct: &str) {
        if let Some(LogicNode::BoolLiteral(value)) = self.view(condition) {
            let dead = if value { "else branch" } else { "body" };
            let literal = self.text(condition);
            self.report(
                condition,
                "logic.constant_condition",
                Severity::Warning,
                Certainty::Fact,
                format!("`{construct}` condition is always {value}"),
                format!("The condition is the literal `{literal}`, so the {dead} never runs."),
            );
        }
    }

    fn constant_false_loop(&mut self, condition: Node) {
        if let Some(LogicNode::BoolLiteral(false)) = self.view(condition) {
            self.report(
                condition,
                "logic.constant_condition",
                Severity::Warning,
                Certainty::Fact,
                "Loop condition is always false".to_string(),
                format!(
                    "The condition is the literal `{}`, so the loop body never runs.",
                    self.text(condition)
                ),
            );
        }
    }

    fn assignment_in_condition(&mut self, condition: Node) {
        if let Some(LogicNode::Assignment { target, .. }) = self.view(condition) {
            self.report(
                condition,
                "logic.assignment_in_condition",
                Severity::Warning,
                Certainty::Inference,
                format!("Assignment to `{}` used as a condition", self.text(target)),
                "The condition assigns instead of comparing; if a comparison was intended, use `==`."
                    .to_string(),
            );
        }
    }

    fn empty_if(&mut self, node: Node, consequence: Node, alternative: Option<Node>) {
        if alternative.is_none() && self.is_empty_body(consequence) {
            self.report(
                node,
                "logic.empty_branch",
                Severity::Warning,
                Certainty::Fact,
                "`if` statement has an empty body".to_string(),
                "Nothing runs when the condition holds and there is no else branch; the \
                 condition only has an effect through side effects."
                    .to_string(),
            );
        }
    }

    fn identical_branches(&mut self, node: Node, consequence: Node, alternative: Option<Node>) {
        let Some(alt) = alternative else {
            return;
        };
        if matches!(self.view(alt), Some(LogicNode::If { .. })) || self.is_empty_body(consequence) {
            return;
        }
        if normalized(self.text(consequence)) == normalized(self.text(alt)) {
            self.report(
                node,
                "logic.identical_branches",
                Severity::Warning,
                Certainty::Fact,
                "Both branches of this `if` are identical".to_string(),
                format!(
                    "The then-branch (line {}) and else-branch (line {}) contain the same code, \
                     so the condition does not change what runs.",
                    consequence.start_position().row + 1,
                    alt.start_position().row + 1
                ),
            );
        }
    }

    /// `if a {} else if a {}`: the second branch can never run.
    fn duplicate_conditions(&mut self, first: Node) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut current = Some(first);
        while let Some(node) = current {
            let Some(LogicNode::If {
                condition,
                alternative,
                ..
            }) = self.view(node)
            else {
                break;
            };
            let text = normalized(self.text(condition));
            if !text.contains('(') {
                if let Some(line) = seen.get(&text) {
                    self.report(
                        condition,
                        "logic.duplicate_condition",
                        Severity::Warning,
                        Certainty::Inference,
                        format!(
                            "Condition `{}` repeats an earlier branch",
                            self.text(condition).trim()
                        ),
                        format!(
                            "The same condition is tested on line {line}; if it held, that branch \
                             already ran, so this branch is unreachable (assuming no side effects)."
                        ),
                    );
                } else {
                    seen.insert(text, condition.start_position().row + 1);
                }
            }
            current = alternative;
        }
    }

    fn unreachable(&mut self, statements: &[Node]) {
        let mut exit: Option<(Node, TerminatorKind)> = None;
        for &stmt in statements {
            match (exit, self.view(stmt)) {
                (
                    None,
                    Some(LogicNode::Terminator {
                        kind,
                        conditional: false,
                    }),
                ) => {
                    exit = Some((stmt, kind));
                }
                (Some((after, kind)), view) => {
                    // Declarations of nested functions are hoisted or inert.
                    if matches!(
                        view,
                        Some(LogicNode::FunctionBoundary) | Some(LogicNode::Empty)
                    ) {
                        continue;
                    }
                    let last = *statements.last().unwrap_or(&stmt);
                    let range = SourceRange {
                        end_line: last.end_position().row + 1,
                        end_byte: last.end_byte(),
                        ..SourceRange::from_node(stmt, &self.parsed.source)
                    };
                    self.report_range(
                        range,
                        "logic.unreachable_code",
                        Severity::Warning,
                        Certainty::Fact,
                        "Unreachable code".to_string(),
                        format!(
                            "This code follows an unconditional `{}` on line {} in the same block.",
                            kind.as_str(),
                            after.start_position().row + 1
                        ),
                    );
                    return;
                }
                _ => {}
            }
        }
    }

    /// `x = 1; x = 2;` — the first value is never read.
    fn overwritten_before_read(&mut self, statements: &[Node]) {
        for pair in statements.windows(2) {
            let first = match self.view(pair[0]) {
                Some(LogicNode::Assignment { target, value }) => (target, Some(value)),
                Some(LogicNode::Declaration {
                    name,
                    value: Some(v),
                }) => (name, Some(v)),
                _ => continue,
            };
            let Some(LogicNode::Assignment { target, value }) = self.view(pair[1]) else {
                continue;
            };
            let name = self.text(first.0);
            if !is_simple_name(name) || self.text(target) != name {
                continue;
            }
            if mentions(self.text(value), name) {
                continue;
            }
            self.report(
                pair[0],
                "logic.overwritten_before_read",
                Severity::Warning,
                Certainty::Inference,
                format!("Value assigned to `{name}` is overwritten before it is read"),
                format!(
                    "`{name}` is assigned again on line {} without being read in between.",
                    pair[1].start_position().row + 1
                ),
            );
        }
    }

    fn self_comparison(&mut self, node: Node, left: Node, operator: &str, right: Node) {
        if !COMPARISON_OPERATORS.contains(&operator) {
            return;
        }
        let (l, r) = (normalized(self.text(left)), normalized(self.text(right)));
        // Calls may return different values each time (`rand() == rand()`).
        if l != r || l.contains('(') || l.is_empty() {
            return;
        }
        let nan_idiom = matches!(operator, "!=" | "!==") && is_simple_name(&l);
        let outcome = match operator {
            "==" | "===" | "<=" | ">=" | "is" => "always true",
            _ => "always false",
        };
        self.report(
            node,
            "logic.self_comparison",
            if nan_idiom {
                Severity::Info
            } else {
                Severity::Warning
            },
            Certainty::Fact,
            format!("`{l}` is compared with itself"),
            if nan_idiom {
                format!("Both sides are `{l}`; this is {outcome} unless it is a NaN check.")
            } else {
                format!("Both sides are `{l}`, so the comparison is {outcome}.")
            },
        );
    }

    fn self_assignment(&mut self, node: Node, target: Node, value: Node) {
        let t = self.text(target);
        if !t.is_empty() && normalized(t) == normalized(self.text(value)) && !t.contains('(') {
            self.report(
                node,
                "logic.self_assignment",
                Severity::Warning,
                Certainty::Fact,
                format!("`{t}` is assigned to itself"),
                "The assignment does not change the value.".to_string(),
            );
        }
    }

    fn empty_catch(&mut self, node: Node, body: Node) {
        if self.is_empty_body(body) {
            self.report(
                node,
                "logic.empty_catch",
                Severity::Warning,
                Certainty::Fact,
                "Exception handler is empty".to_string(),
                "Errors caught here are discarded without handling, logging or cleanup."
                    .to_string(),
            );
        }
    }

    fn discarded_result(&mut self, node: Node, value: Node) {
        if !self.text(value).contains('(') {
            return;
        }
        self.report(
            node,
            "logic.discarded_result",
            Severity::Info,
            Certainty::Fact,
            format!("Result of `{}` is discarded", first_line(self.text(value))),
            "The value (possibly an error) is explicitly thrown away.".to_string(),
        );
    }

    fn loop_without_exit(&mut self, node: Node, condition: Option<Node>, body: Node) {
        let unconditional = match condition {
            None => true,
            Some(c) => matches!(self.view(c), Some(LogicNode::BoolLiteral(true))),
        };
        if !unconditional || self.has_exit(body, 0) {
            return;
        }
        self.report(
            node,
            "logic.infinite_loop",
            Severity::Warning,
            Certainty::Inference,
            "Loop has no exit".to_string(),
            "The loop condition is always true and the body contains no break, return or throw. \
             It only ends if a called function exits the process."
                .to_string(),
        );
    }

    // --- helpers -----------------------------------------------------------

    /// True if a break (at loop depth 0), return or throw is reachable
    /// inside `node` without crossing a nested function.
    fn has_exit(&self, node: Node, loop_depth: usize) -> bool {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match self.view(child) {
                Some(LogicNode::FunctionBoundary) => continue,
                Some(LogicNode::Terminator { kind, .. }) => match kind {
                    TerminatorKind::Return | TerminatorKind::Throw => return true,
                    TerminatorKind::Break if loop_depth == 0 => return true,
                    _ => {}
                },
                Some(LogicNode::Loop { .. }) => {
                    if self.has_exit(child, loop_depth + 1) {
                        return true;
                    }
                    continue;
                }
                _ => {}
            }
            if self.has_exit(child, loop_depth) {
                return true;
            }
        }
        false
    }

    fn is_empty_body(&self, node: Node) -> bool {
        match self.view(node) {
            Some(LogicNode::Block { statements }) => statements
                .iter()
                .all(|s| matches!(self.view(*s), Some(LogicNode::Empty))),
            Some(LogicNode::Empty) => true,
            _ => false,
        }
    }

    fn is_else_if(&self, node: Node) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        // Either the parent `if` holds this node as its alternative, or an
        // else-wrapper does.
        let holder = if matches!(self.view(parent), Some(LogicNode::If { .. })) {
            parent
        } else if let Some(gp) = parent.parent() {
            gp
        } else {
            return false;
        };
        matches!(self.view(holder), Some(LogicNode::If { alternative: Some(a), .. }) if a.id() == node.id())
    }

    fn text(&self, node: Node) -> &'a str {
        self.parsed.text(node)
    }

    fn report(
        &mut self,
        node: Node,
        rule: &str,
        severity: Severity,
        certainty: Certainty,
        message: String,
        evidence: String,
    ) {
        let range = SourceRange::from_node(node, &self.parsed.source);
        self.report_range(range, rule, severity, certainty, message, evidence);
    }

    fn report_range(
        &mut self,
        range: SourceRange,
        rule: &str,
        severity: Severity,
        certainty: Certainty,
        message: String,
        evidence: String,
    ) {
        self.out.push(Diagnostic {
            category: DiagnosticCategory::Logic,
            rule: rule.to_string(),
            severity,
            certainty,
            message,
            excerpt: render_excerpt(&self.parsed.source, &range),
            range,
            expected: Vec::new(),
            evidence: Some(evidence),
            symbol: None,
        });
    }
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_simple_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == '.')
        && !text.starts_with(|c: char| c.is_ascii_digit())
}

/// Whole-word occurrence of `name` in `text`.
fn mentions(text: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(name).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + name.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::language::LanguageRegistry;
    use std::path::Path;

    /// (rule, line) pairs found in `src`.
    fn findings(lang: &str, path: &str, src: &str) -> Vec<(String, usize)> {
        let registry = LanguageRegistry::default();
        let parsed = registry
            .parse(lang, Path::new(path), src.to_string())
            .unwrap();
        logic_diagnostics(&parsed, registry.get(lang).unwrap())
            .into_iter()
            .map(|d| (d.rule, d.range.start_line))
            .collect()
    }

    fn rules(lang: &str, path: &str, src: &str) -> Vec<String> {
        findings(lang, path, src)
            .into_iter()
            .map(|(r, _)| r)
            .collect()
    }

    #[test]
    fn test_each_rule_fires_once_per_finding() {
        let src = "fn f(x: i32) {\n  let mut y = 1;\n  y = y;\n}\n";
        assert_eq!(rules("rust", "a.rs", src), ["logic.self_assignment"]);
    }

    #[test]
    fn test_loops_with_exits_are_not_reported() {
        let ts = "function f(x) {\n  while (true) { if (x) break; }\n  for (;;) { return 1; }\n}\n";
        assert!(rules("typescript", "a.ts", ts).is_empty());
        // `?` may leave the loop.
        let rs = "fn f() -> Result<(), E> { loop { step()?; } }\n";
        assert!(rules("rust", "a.rs", rs).is_empty());
        // A break of an inner loop does not end the outer one.
        let py = "while True:\n    for x in y:\n        break\n";
        assert_eq!(
            findings("python", "a.py", py),
            [("logic.infinite_loop".to_string(), 1)]
        );
    }

    #[test]
    fn test_reads_between_assignments_are_respected() {
        let ts = "function f() {\n  let x = 1;\n  x = x + 1;\n  let y = g();\n  y = 2;\n}\n";
        assert_eq!(
            findings("typescript", "a.ts", ts),
            [("logic.overwritten_before_read".to_string(), 4)]
        );
    }

    #[test]
    fn test_hoisted_functions_after_return_are_reachable() {
        let js = "function f() {\n  return g();\n  function g() { return 1; }\n}\n";
        assert!(rules("javascript", "a.js", js).is_empty());
        let py = "def f():\n    raise E()\n    cleanup()\n";
        assert_eq!(
            findings("python", "a.py", py),
            [("logic.unreachable_code".to_string(), 3)]
        );
    }

    #[test]
    fn test_duplicate_condition_in_else_if_chain() {
        let cs = "class A { void M(int a) {\n  if (a > 1) { X(); } else if (a < 0) { Y(); } else if (a > 1) { Z(); }\n} }\n";
        assert_eq!(
            findings("csharp", "a.cs", cs),
            [("logic.duplicate_condition".to_string(), 2)]
        );
        let py = "if a:\n    x()\nelif b:\n    y()\nelif a:\n    z()\n";
        assert_eq!(
            findings("python", "a.py", py),
            [("logic.duplicate_condition".to_string(), 5)]
        );
    }

    #[test]
    fn test_identical_branches_and_empty_bodies() {
        let ts = "if (a) { run(); } else { run(); }\nif (b) {}\ntry { x(); } catch { }\n";
        assert_eq!(
            rules("typescript", "a.ts", ts),
            [
                "logic.identical_branches",
                "logic.empty_branch",
                "logic.empty_catch"
            ]
        );
    }

    #[test]
    fn test_self_comparison_severity_and_call_guard() {
        let registry = LanguageRegistry::default();
        let parsed = registry
            .parse(
                "javascript",
                Path::new("a.js"),
                "if (x !== x) {}\nif (rand() == rand()) {}\n".into(),
            )
            .unwrap();
        let d: Vec<_> = logic_diagnostics(&parsed, registry.get("javascript").unwrap())
            .into_iter()
            .filter(|d| d.rule == "logic.self_comparison")
            .collect();
        assert_eq!(d.len(), 1, "calls may differ between evaluations");
        assert_eq!(d[0].severity, Severity::Info, "x !== x is the NaN idiom");
    }

    #[test]
    fn test_constant_conditions_quote_source_literal() {
        let registry = LanguageRegistry::default();
        let parsed = registry
            .parse("python", Path::new("a.py"), "if False:\n    x()\n".into())
            .unwrap();
        let d = logic_diagnostics(&parsed, registry.get("python").unwrap());
        assert_eq!(d[0].rule, "logic.constant_condition");
        assert!(d[0].evidence.as_deref().unwrap().contains("`False`"));
    }

    #[test]
    fn test_files_with_syntax_errors_skip_broken_regions() {
        let rs = "fn f() { if true { return; } else {\n";
        assert!(rules("rust", "a.rs", rs).is_empty());
    }
}
