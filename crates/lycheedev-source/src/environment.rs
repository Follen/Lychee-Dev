//! Fixed API-documentation literals and LuaLS declarations. No repository code
//! is executed. Dynamic/escaped metadata remains an explicit coverage gap.
mod emit;
mod parse;
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const GENERATOR: &str = "environment-ts-v2-script-object-owners";
pub const DECLARATION_RULES: &str = "api-script-object-owner-v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub repository: String,
    pub commit: String,
    pub client: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependency_commits: BTreeMap<String, String>,
    pub generator_version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub index: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub inner_type: String,
    pub nilable: bool,
    pub line: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationOwner {
    pub kind: String,
    pub name: String,
    pub source: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRecord {
    #[serde(skip)]
    pub observed_span: Value,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<DeclarationOwner>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub declaration_role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub namespace: String,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub returns: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payload: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Slot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Slot>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gap {
    pub path: String,
    #[serde(default, skip_serializing_if = "zero")]
    pub line: usize,
    pub code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}
fn zero(v: &usize) -> bool {
    *v == 0
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub callable: String,
    pub source_path: String,
    pub source_line: usize,
    pub generated_line: usize,
    #[serde(rename = "generatedUTF16Column")]
    pub generated_utf16_column: usize,
    pub kind: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub documents: usize,
    pub parsed_documents: usize,
    pub records: usize,
    pub emitted: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_types: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<Gap>,
    pub truncated: bool,
    pub complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: String,
    pub identity: Identity,
    #[serde(rename = "inputSHA256")]
    pub input_sha256: String,
    #[serde(rename = "definitionsSHA256")]
    pub definitions_sha256: String,
    #[serde(rename = "factsSHA256")]
    pub facts_sha256: String,
    pub documents: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    pub manifest: Manifest,
    pub definitions: String,
    pub facts: Vec<ApiRecord>,
    pub mappings: Vec<Mapping>,
    pub coverage: Coverage,
    #[serde(rename = "sourceSpans")]
    pub source_spans: Vec<Value>,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub documents: usize,
    pub file_bytes: usize,
    pub total_bytes: usize,
    pub records: usize,
    pub definition_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            documents: 4096,
            file_bytes: 4 << 20,
            total_bytes: 64 << 20,
            records: 100000,
            definition_bytes: 8 << 20,
        }
    }
}
fn budget() -> ToolkitError {
    ToolkitError::new(
        "environment.input_budget",
        "Fixed API environment budget exceeded",
        3,
    )
}
pub fn build(
    identity: Identity,
    documents: &BTreeMap<String, Vec<u8>>,
    limits: Limits,
    control: &OperationControl,
) -> Result<Environment> {
    if identity.repository.is_empty()
        || identity.client.is_empty()
        || !lycheedev_core::selection::valid_digest(&identity.commit, 40)
        || identity.generator_version.is_empty()
    {
        return Err(ToolkitError::new(
            "environment.invalid_identity",
            "API environment requires an exact source identity",
            4,
        ));
    }
    if documents.len() > limits.documents {
        return Err(budget());
    }
    let mut hash = Sha256::new();
    let mut total = 0usize;
    let mut facts = Vec::new();
    let mut coverage = Coverage::default();
    for (path, raw) in documents {
        control.check()?;
        total = total.checked_add(raw.len()).ok_or_else(budget)?;
        if !crate::git::source_path(path)
            || !path.to_ascii_lowercase().ends_with(".lua")
            || std::str::from_utf8(raw).is_err()
            || raw.len() > limits.file_bytes
            || total > limits.total_bytes
        {
            return Err(budget());
        }
        hash.update(format!("{}:{path}:{}:", path.len(), raw.len()).as_bytes());
        hash.update(raw);
        coverage.documents += 1;
        let (mut records, gaps) = parse::document(path, raw, control)?;
        if gaps.first().is_none_or(|g| g.code != "parse") {
            coverage.parsed_documents += 1;
        }
        facts.append(&mut records);
        coverage.gaps.extend(gaps);
        if facts.len() > limits.records || coverage.gaps.len() > 10000 {
            return Err(budget());
        }
    }
    facts.sort_by(|a, b| {
        (&a.path, a.line, &a.kind, &a.name).cmp(&(&b.path, b.line, &b.kind, &b.name))
    });
    let (definitions, mappings, unknown, gaps) =
        emit::definitions(&facts, limits.definition_bytes, control)?;
    coverage.records = facts.len();
    coverage.emitted = mappings.len();
    coverage.unknown_types = unknown;
    coverage.gaps.extend(gaps);
    coverage.complete = coverage.gaps.is_empty() && !coverage.truncated;
    // Preserve the frozen Go JSON framing for hashes, including HTML/line-separator escaping.
    let facts_json = serde_json::to_string(&facts)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let manifest = Manifest {
        schema: "lycheedev.source-environment.v1".into(),
        identity,
        input_sha256: format!("{:x}", hash.finalize()),
        definitions_sha256: storage::sha256(definitions.as_bytes()),
        facts_sha256: storage::sha256(facts_json.as_bytes()),
        documents: coverage.documents,
    };
    let source_spans = facts.iter().map(|r| r.observed_span.clone()).collect();
    Ok(Environment {
        manifest,
        definitions,
        facts,
        mappings,
        coverage,
        source_spans,
    })
}

/// Authenticate generated metadata directly from the immutable Git tree/blob
/// transport. This path neither reads user-global source data nor runs Lua.
pub fn from_pin(
    home: &std::path::Path,
    pin: &lycheedev_core::selection::SourcePin,
    control: &OperationControl,
) -> Result<Environment> {
    let docs = crate::research_material::measured("source.environment.git", || documents_from_pin(home, pin, control))?;
    crate::research_material::measured("source.environment.extract", || build(
        Identity {
            repository: pin.repository.clone(),
            commit: pin.exact_commit.clone(),
            client: pin.product.clone(),
            dependency_commits: BTreeMap::new(),
            generator_version: GENERATOR.into(),
        },
        &docs,
        Limits::default(),
        control,
    ))
}
pub fn documents_from_pin(
    home: &std::path::Path,
    pin: &lycheedev_core::selection::SourcePin,
    control: &OperationControl,
) -> Result<BTreeMap<String, Vec<u8>>> {
    if pin.repository != "wow-ui-source" {
        return Err(ToolkitError::new(
            "codebase.client_source_pin_required",
            "A fixed Blizzard ClientPin is required for API declarations",
            3,
        ));
    }
    let limits = Limits::default();
    let files: Vec<_> = crate::git::tree_with_control(home, pin, control)?
        .into_iter()
        .filter(|f| {
            let path = f.path.to_ascii_lowercase();
            path.contains("/blizzard_apidocumentationgenerated/") && path.ends_with(".lua")
        })
        .collect();
    if files.is_empty() {
        return Err(ToolkitError::new(
            "codebase.environment_not_ready",
            "The fixed source pin has no generated API metadata",
            3,
        ));
    }
    let total = files
        .iter()
        .try_fold(0usize, |sum, f| sum.checked_add(f.bytes).ok_or_else(budget))?;
    if files.len() > limits.documents
        || total > limits.total_bytes
        || files
            .iter()
            .any(|f| f.bytes > limits.file_bytes || f.oversized)
    {
        return Err(budget());
    }
    let mut docs = BTreeMap::new();
    crate::git::visit_blobs_with_control(home, pin, &files, control, |file, bytes| {
        docs.insert(file.path.clone(), bytes);
        Ok(())
    })?;
    Ok(docs)
}
