#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Class,
    Interface,
    Struct,
    Function,
    Method,
    Module,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub id: Option<i64>,
    pub file_id: i64,
    pub parent_id: Option<i64>,
    pub kind: SymbolKind,
    pub name: String,
    pub signature: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationshipKind {
    Imports,
    Calls,
    References,
    DependsOn,
    Inherits,
    Implements,
    GeneratedFrom,
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Relationship {
    pub id: Option<i64>,
    pub source_type: String,
    pub source_id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub kind: RelationshipKind,
    pub confidence: f32,
}
