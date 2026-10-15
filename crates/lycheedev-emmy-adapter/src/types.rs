use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

/// Already-authenticated host provenance. Authentication remains in Source.
#[derive(Clone, Copy)]
pub struct OriginInput<'a> {
    pub material: &'a str,
    pub material_sha256: &'a str,
    pub pointer: &'a str,
    pub identity: &'a str,
}

/// Borrow the admitted declaration and alias table, not a Source graph or an
/// engine type. Only the selected callable is materialized for this query.
/// Nominal declarations remain unsupported by this fixed backend projection;
/// this boundary does not reinterpret them as primitive signatures.
pub struct CallableInput<'a> {
    pub origin: OriginInput<'a>,
    pub declaration: &'a Value,
    pub catalog_identity: &'a str,
    pub aliases: &'a BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseIssue {
    pub kind: String,
    pub range: ByteRange,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseIssues {
    pub count: usize,
    pub first: ParseIssue,
}

#[derive(Debug)]
pub enum DeclarationFailure<E> {
    Interrupted(E),
    InvalidInput(String),
    Parse(ParseIssues),
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CoverageRow {
    pub operation: String,
    pub state: String,
    pub proof: String,
    pub reason: Option<String>,
}

#[derive(Debug)]
pub struct TypeProjection {
    pub known: bool,
    pub rendered: String,
    pub coverage: Vec<CoverageRow>,
}
