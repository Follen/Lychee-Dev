//! The single pinned-addon route is Go 3.2.3 CheckClosure, not matrix
//! compatibility validation. Missing declarations are unresolved observations.
use crate::{
    closure::{Closure, Document},
    index::Index,
};
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    selection::{SourcePin, source_interface},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn checked(category: &str) -> bool {
    matches!(
        category,
        "call" | "event-registration" | "xml-inherits" | "xml-function" | "xml-method"
    )
}

fn indexable_target(doc: &Document, ordinal: usize) -> Result<bool> {
    // The public single entry always freshly parses Closure document bytes.
    // A deserialized Facts value cannot certify the pre-JSON byte identity;
    // never infer it from a lossy display name or a coincident file/line.
    let bytes = doc
        .facts
        .relationship_target_bytes(ordinal)
        .ok_or_else(|| {
            ToolkitError::new(
                "codebase.source_integrity",
                "Compatibility reference lacks its original byte identity",
                4,
            )
            .with_details(json!({"path":doc.path,"relationshipOrdinal":ordinal}))
        })?;
    // Pinned index records have crossed encoding/json's UTF-8 boundary. Go's
    // invalid-byte Lua name cannot equal any such indexed Unicode string.
    Ok(std::str::from_utf8(bytes).is_ok())
}

pub(super) fn assess(
    pin: &SourcePin,
    closure: &Closure,
    index: &Index,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    let mut wanted = BTreeMap::<(&str, &str), Vec<&crate::index::Symbol>>::new();
    let mut count = 0usize;
    for doc in &closure.documents {
        for (ordinal, edge) in doc.facts.relationships.iter().enumerate() {
            control.check()?;
            if !checked(&edge.category) {
                continue;
            }
            count += 1;
            if count > 100_000 {
                return Err(ToolkitError::new(
                    "codebase.reference_budget",
                    "Compatibility reference budget exceeded",
                    3,
                ));
            }
            if edge.confidence != "dynamic-unresolved" && indexable_target(doc, ordinal)? {
                wanted.entry((&edge.to, &edge.category)).or_default();
            }
        }
    }
    // One pass over the immutable index; each usage keeps its own location.
    let mut by_name = BTreeMap::<&str, Vec<&str>>::new();
    for &(name, category) in wanted.keys() {
        by_name.entry(name).or_default().push(category);
    }
    if !wanted.is_empty() {
        for symbol in &index.symbols {
            control.check()?;
            if symbol.kind != "declaration" {
                continue;
            }
            let Some(categories) = by_name.get(symbol.name.as_str()) else {
                continue;
            };
            for &category in categories {
                let eligible = match category {
                    "event-registration" => symbol.category == "api-event",
                    "xml-inherits" => symbol.category.starts_with("xml-"),
                    "xml-function" | "xml-method" => {
                        matches!(symbol.category.as_str(), "function" | "api-function")
                    }
                    _ => matches!(
                        symbol.category.as_str(),
                        "api-function" | "api-scriptobject" | "api-callback"
                    ),
                };
                if eligible {
                    let sites = wanted
                        .get_mut(&(symbol.name.as_str(), category))
                        .expect("requested reference key");
                    sites.push(symbol);
                    if sites.len() > 20 {
                        return Err(ToolkitError::new(
                            "codebase.reference_ambiguity_budget",
                            "Compatibility declaration ambiguity exceeds budget",
                            3,
                        ));
                    }
                }
            }
        }
    }
    for sites in wanted.values_mut() {
        sites.sort_by(|left, right| left.path.cmp(&right.path).then(left.line.cmp(&right.line)));
    }
    let mut references = Vec::new();
    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    for doc in &closure.documents {
        for (ordinal, edge) in doc.facts.relationships.iter().enumerate() {
            control.check()?;
            if !checked(&edge.category) {
                continue;
            }
            let mut reference = json!({"path":doc.path,"line":edge.line,"name":edge.to,"category":edge.category,"status":"unresolved","evidence":[]});
            let sites =
                if edge.confidence != "dynamic-unresolved" && indexable_target(doc, ordinal)? {
                    wanted
                        .get(&(edge.to.as_str(), edge.category.as_str()))
                        .filter(|sites| !sites.is_empty())
                } else {
                    None
                };
            if edge.confidence == "dynamic-unresolved" {
                reference["reason"] = json!("dynamic source expression");
            } else if let Some(sites) = sites {
                reference["status"] = json!("source-present");
                reference["evidence"] = json!(sites.iter().map(|s| json!({"path":s.path,"line":s.line,"endLine":s.end_line,"signature":s.signature})).collect::<Vec<_>>());
            } else {
                reference["reason"] = json!(
                    "no authoritative declaration found; may be local, dynamically supplied, or outside indexed coverage"
                );
            }
            if reference["status"] == "source-present" {
                resolved += 1;
            } else {
                unresolved += 1;
            }
            references.push(reference);
        }
    }
    let expected = source_interface(pin);
    let mut interface_status = "unresolved";
    // The shared native closure retains bounded cycles as warnings. Fixed Go
    // CheckClosure exposes the underlying error; only matrix validation lowers
    // it to a legacy warning. Keep this single-entry distinction explicit.
    let load_valid = closure.valid
        && !closure
            .issues
            .iter()
            .any(|issue| issue["code"] == "load_cycle");
    let mut valid = load_valid;
    if let Some(expected) = expected {
        let values = closure
            .documents
            .iter()
            .filter(|doc| doc.path == closure.manifest)
            .flat_map(|doc| &doc.facts.headers)
            .filter(|header| header.key.eq_ignore_ascii_case("Interface"))
            .map(|header| header.value.as_str())
            .collect::<Vec<_>>();
        if values.len() == 1 {
            if values[0]
                .split(',')
                .any(|value| value.trim() == expected.to_string())
            {
                interface_status = "matched";
            } else {
                interface_status = "mismatch";
                valid = false;
            }
        } else if values.len() > 1 {
            interface_status = "ambiguous";
            valid = false;
        }
    }
    let complete = load_valid
        && index.summary["complete"] == true
        && unresolved == 0
        && interface_status == "matched";
    let mut load = closure.value();
    load["loadValid"] = json!(load_valid);
    if let Some(issues) = load["issues"].as_array_mut() {
        for issue in issues
            .iter_mut()
            .filter(|issue| issue["code"] == "load_cycle")
        {
            issue["severity"] = json!("error");
        }
    }
    load["compatibilityComplete"] = json!(complete);
    let mut value = json!({"source":pin,"load":load,"references":references,"interfaceStatus":interface_status,"resolved":resolved,"unresolved":unresolved,"staticValid":valid,"complete":complete,
        "checks":["load-paths","load-order","syntax","source-name-presence","project-interface-baseline"],
        "notChecked":["binding-identity","argument-types","dynamic-code","combat","taint","runtime-behavior"]});
    if let Some(expected) = expected {
        value["expectedInterface"] = json!(expected);
    }
    Ok(value)
}
