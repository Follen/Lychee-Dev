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
/// Named callbacks are resolved on demand from this same authenticated catalog.
/// Other nominal declarations remain unavailable, never primitive stand-ins.
pub struct CallableInput<'a> {
    pub origin: OriginInput<'a>,
    pub declaration: &'a Value,
    pub catalog_identity: &'a str,
    pub aliases: &'a BTreeMap<String, String>,
}

/// The host checks declaration uniqueness before materializing one record.
/// Missing and ambiguous are distinct; neither permits raw engine inference.
#[derive(Debug)]
pub enum DeclarationLookup {
    Missing,
    Ambiguous,
    Unique(Value),
}

/// Small provenance for each named callback dependency, bound to the admitted
/// catalog identity. The original declaration and source graph stay with Source.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationProvenance {
    pub name: String,
    pub kind: String,
    pub catalog_identity: String,
    pub path: String,
    pub line: u64,
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
    pub declarations: Vec<DeclarationProvenance>,
}
