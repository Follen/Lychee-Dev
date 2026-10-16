//! Compare authenticated indexed declarations, retaining duplicate sites and original blob identities.
use crate::{
    git,
    index::{self, Index},
};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError, evidence,
    selection::{self, SourcePin},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationSite {
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    pub signature: String,
}
#[derive(Debug, Serialize)]
pub struct DeclarationChange {
    pub name: String,
    pub category: String,
    pub status: String,
    pub before: Vec<DeclarationSite>,
    pub after: Vec<DeclarationSite>,
}
#[derive(Debug, Serialize)]
pub struct DocumentChange {
    pub path: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}
fn documents(index: &Index) -> Result<BTreeMap<String, Value>> {
    if index.documents.len() > 100000 {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "codebase.diff_document_budget",
            3,
        ));
    }
    let mut result = BTreeMap::new();
    for doc in &index.documents {
        if !git::source_path(&doc.path)
            || !selection::valid_digest(&doc.sha256, 64)
            || result
                .insert(
                    doc.path.clone(),
                    json!({"sha256":doc.sha256,"bytes":doc.bytes}),
                )
                .is_some()
        {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "ambiguous indexed document identity",
                4,
            ));
        }
    }
    Ok(result)
}
fn declarations(index: &Index) -> Result<BTreeMap<(String, String), Vec<DeclarationSite>>> {
    let mut count = 0usize;
    let mut result: BTreeMap<(String, String), Vec<DeclarationSite>> = BTreeMap::new();
    for symbol in index
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == "declaration")
    {
        count += 1;
        if count > 500000 {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "codebase.diff_declaration_budget",
                3,
            ));
        }
        result
            .entry((symbol.name.clone(), symbol.category.clone()))
            .or_default()
            .push(DeclarationSite {
                path: symbol.path.clone(),
                line: symbol.line,
                end_line: symbol.end_line,
                signature: symbol.signature.clone(),
            });
    }
    for sites in result.values_mut() {
        sites.sort();
    }
    Ok(result)
}
fn verified_blob(
    home: &Path,
    pin: &SourcePin,
    path: &str,
    expected: &Value,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Value> {
    let (raw, object) = git::blob_with_control(home, pin, path, git::MAX_SOURCE_BYTES, control)?;
    let hash = lycheedev_core::storage::sha256(&raw);
    if expected["sha256"] != hash || expected["bytes"] != raw.len() {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "indexed comparison blob differs from the fixed Git object",
            4,
        ));
    }
    evidence::publish_blob_bounded(home, &raw, git::MAX_SOURCE_BYTES, control)?;
    Ok(json!({"commit":pin.exact_commit,"gitObject":object,"sha256":hash,"bytes":raw.len()}))
}
fn ready(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<bool> {
    match index::is_ready_with_control(home, pin, control) {
        Err(error) if error.code == "vault.record_missing" && error.details.is_null() => Ok(false),
        result => result,
    }
}
pub fn compare(home: &Path, from: &SourcePin, to: &SourcePin, limit: usize) -> Result<Value> {
    compare_with_control(home, from, to, limit, &Default::default())
}
pub fn compare_with_control(
    home: &Path,
    from: &SourcePin,
    to: &SourcePin,
    limit: usize,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Value> {
    control.check()?;
    if !(1..=200).contains(&limit)
        || from.repository != to.repository
        || from.parser_revision != to.parser_revision
    {
        return Err(ToolkitError::new(
            "codebase.incompatible_comparison",
            "codebase.incompatible_comparison",
            5,
        ));
    }
    git::validate_pin(from)?;
    git::validate_pin(to)?;
    if !ready(home, from, control)? || !ready(home, to, control)? {
        return Err(ToolkitError::new(
            "codebase.index_not_ready",
            "codebase.index_not_ready: run source index with the same snapshot",
            5,
        ));
    }
    let left = index::build_with_control(home, from, control)?;
    control.check()?;
    let right = index::build_with_control(home, to, control)?;
    control.check()?;
    if left.summary["complete"] != true || right.summary["complete"] != true {
        return Err(ToolkitError::new("codebase.incomplete_index", "codebase.incomplete_index: inspect source index diagnostics before comparing declarations", 5)
            .with_details(json!({"from":left.summary,"to":right.summary,"coverage":{"from":left.coverage(),"to":right.coverage()}})));
    }
    let before = documents(&left)?;
    let after = documents(&right)?;
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
    let mut changed_documents = Vec::new();
    let mut document_changes = 0usize;
    let mut proofs = Vec::new();
    for path in paths {
        control.check()?;
        let a = before.get(&path);
        let z = after.get(&path);
        if a == z {
            continue;
        }
        document_changes += 1;
        if changed_documents.len() == limit {
            continue;
        }
        let a_proof = a
            .map(|blob| verified_blob(home, from, &path, blob, control))
            .transpose()?;
        let z_proof = z
            .map(|blob| verified_blob(home, to, &path, blob, control))
            .transpose()?;
        proofs.push(json!({"path":path,"before":a_proof,"after":z_proof}));
        changed_documents.push(DocumentChange {
            path,
            status: if a.is_none() {
                "added"
            } else if z.is_none() {
                "removed"
            } else {
                "changed"
            }
            .into(),
            before: a.cloned(),
            after: z.cloned(),
        });
    }
    let old = declarations(&left)?;
    let new = declarations(&right)?;
    let keys: BTreeSet<_> = old.keys().chain(new.keys()).cloned().collect();
    let mut changed_declarations = Vec::new();
    let mut declaration_changes = 0usize;
    for (name, category) in keys {
        control.check()?;
        let key = (name.clone(), category.clone());
        let a = old.get(&key);
        let z = new.get(&key);
        if a == z {
            continue;
        }
        declaration_changes += 1;
        if changed_declarations.len() == limit {
            continue;
        }
        changed_declarations.push(DeclarationChange {
            name,
            category,
            status: if a.is_none() {
                "added"
            } else if z.is_none() {
                "removed"
            } else {
                "changed"
            }
            .into(),
            before: a.cloned().unwrap_or_default(),
            after: z.cloned().unwrap_or_default(),
        });
    }
    let truncated = document_changes > changed_documents.len()
        || declaration_changes > changed_declarations.len();
    let data = json!({"from":left.summary,"to":right.summary,"documents":changed_documents,"declarations":changed_declarations,"documentChanges":document_changes,"declarationChanges":declaration_changes,"truncated":truncated,
        "sourceProofs":proofs,"coverage":{"from":left.coverage(),"to":right.coverage(),"complete":true},"analysis":{"engine":"tree-sitter","scope":"indexed-declarations","runtimeCompatibility":"not-evaluated"}});
    if serde_json::to_vec(&data)?.len() > 16 << 20 {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "comparison output exceeds 16 MiB",
            3,
        ));
    }
    Ok(data)
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let from = inv.required("--from")?;
    let to = inv.required("--to")?;
    let left = selection::load_pinned(&inv.home, from)?;
    let right = selection::load_pinned(&inv.home, to)?;
    let pair = left
        .source
        .as_ref()
        .zip(right.source.as_ref())
        .ok_or_else(|| {
            ToolkitError::new(
                "codebase.source_pin_required",
                "codebase.source_pin_required",
                5,
            )
        })?;
    let data = compare_with_control(
        &inv.home,
        pair.0,
        pair.1,
        inv.usize_flag("--limit", 50, 1, 200)?,
        &inv.control,
    )?;
    inv.control.check()?;
    let complete = data["truncated"] != true;
    let capture = evidence::commit_capture_bounded(
        &inv.home,
        lycheedev_core::evidence::CaptureInput {
            bytes: &serde_json::to_vec(&data)?,
            maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"source-diff","locator":format!("{from}..{to}"),"snapshot":to,"sourceCommit":pair.1.exact_commit,"baseSnapshot":from,"baseSourceCommit":pair.0.exact_commit}),
            complete: complete,
            truncated: !complete,
        },
        &inv.control,
    )?;
    let mut result = Outcome::new(data);
    result.complete = complete;
    result.selection = json!({"fromSnapshot":from,"toSnapshot":to});
    result.coverage = result.data["coverage"].clone();
    result.provenance.push(capture);
    Ok(result)
}
