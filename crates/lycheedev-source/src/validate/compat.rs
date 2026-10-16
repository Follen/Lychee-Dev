//! Matrix compatibility resolution against one fixed public source index.
use super::usage::Usage;
use crate::index::{Index, Symbol};
use lycheedev_core::{
    Result,
    control::OperationControl,
    selection::{SourcePin, source_interface},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Match {
    path: String,
    line: usize,
    role: String,
    signature: String,
    detail: String,
}
#[derive(Default)]
struct Resolution {
    matches: Vec<Match>,
    known: bool,
}
type Resolutions = BTreeMap<&'static str, BTreeMap<String, Resolution>>;
pub(super) struct Assessment {
    pub facts: Vec<Value>,
    pub unresolved: Vec<Value>,
    pub diagnostics: Vec<Value>,
}
fn kind(value: &str) -> &'static str {
    match value.trim() {
        "api" | "api-candidate" | "function" => "api",
        "event" => "event",
        "template" | "xml-template" => "template",
        "mixin" => "mixin",
        "frame-type" | "frametype" | "frame" => "frame-type",
        "interface" => "interface",
        _ => "",
    }
}
fn interface_matches(value: &str, name: &str) -> bool {
    value.split(',').any(|part| {
        !part.trim().is_empty()
            && name
                .split(',')
                .any(|requested| part.trim() == requested.trim())
    })
}
fn base(pin: &SourcePin, snapshot: &str, usage: &Usage) -> Value {
    let mut evidence = json!({"sourceId":pin.repository,"product":pin.product,"requestedRef":pin.requested_ref,"matchedTag":null,"resolvedCommit":pin.exact_commit,"snapshotId":snapshot});
    if !usage.file.is_empty() {
        evidence["usage"] = json!({"file":usage.file,"line":usage.line,"column":usage.column});
    }
    evidence
}
fn unresolved(pin: &SourcePin, snapshot: &str, usage: &Usage, reason: &str) -> Value {
    json!({"kind":usage.kind,"expression":if usage.expression.trim().is_empty(){usage.name.trim()}else{usage.expression.trim()},"file":usage.file,"line":usage.line,"column":usage.column,"reason":reason,"evidence":base(pin,snapshot,usage)})
}
fn missing(kind: &str, name: &str) -> (&'static str, String) {
    match kind {
        "api" => (
            "api_not_found",
            format!("API {name} does not exist in the target snapshot"),
        ),
        "event" => (
            "event_not_found",
            format!("event {name} does not exist in the target snapshot"),
        ),
        "template" => (
            "xml_template_not_found",
            format!("XML template {name} does not exist in the target snapshot"),
        ),
        "interface" => (
            "toc_interface_mismatch",
            format!("TOC Interface {name} does not match the target snapshot"),
        ),
        _ => (
            "compatibility_reference_not_found",
            format!("{kind} {name} does not exist in the target snapshot"),
        ),
    }
}
fn append_match(resolutions: &mut Resolutions, kind: &str, name: &str, symbol: &Symbol) {
    let Some(resolution) = resolutions
        .get_mut(kind)
        .and_then(|names| names.get_mut(name))
    else {
        return;
    };
    // Fixed Go retains at most 51 matches internally, exposing five locations.
    if resolution.matches.len() < 51 {
        resolution.matches.push(Match {
            path: symbol.path.clone(),
            line: symbol.line,
            role: crate::index::role(&symbol.path).into(),
            signature: if kind == "interface" {
                symbol.target.clone()
            } else {
                symbol.signature.clone()
            },
            detail: symbol.category.clone(),
        });
    }
}
fn lookup(
    pin: &SourcePin,
    snapshot: &str,
    usage: &Usage,
    kind: &str,
    resolution: &mut Resolution,
) -> Option<Value> {
    let mut no_matches = [];
    let matches = if usage.invalid_name_bytes.is_some() {
        &mut no_matches[..]
    } else {
        &mut resolution.matches[..]
    };
    if matches.is_empty() && (!resolution.known || matches!(kind, "mixin" | "frame-type")) {
        return None;
    }
    let mut evidence = base(pin, snapshot, usage);
    evidence["matchCount"] = json!(matches.len());
    evidence["matchesTruncated"] = json!(matches.len() > 5);
    // Go stably sorts the retained slice's first five entries in place before
    // choosing the signature. Repeated locations reuse that resolution cache.
    let limit = matches.len().min(5);
    matches[..limit]
        .sort_by(|left, right| left.path.cmp(&right.path).then(left.line.cmp(&right.line)));
    let signature = matches
        .first()
        .map(|value| value.signature.clone())
        .unwrap_or_default();
    evidence["matches"]=json!(matches[..limit].iter().map(|value|json!({"path":value.path,"line":value.line,"role":value.role,"signature":value.signature,"detail":value.detail})).collect::<Vec<_>>());
    if matches.is_empty() {
        evidence["categoryPresent"] = json!(true);
    }
    Some(
        json!({"kind":kind,"name":usage.name.trim(),"exists":!matches.is_empty(),"signature":signature,"evidence":evidence}),
    )
}

pub(super) fn assess(
    pin: &SourcePin,
    snapshot: &str,
    index: &Index,
    usages: &[Usage],
    interface: &str,
    control: &OperationControl,
) -> Result<Assessment> {
    control.check()?;
    let mut resolutions = Resolutions::new();
    for usage in usages {
        control.check()?;
        let normalized = usage.kind.trim().to_ascii_lowercase();
        let kind = kind(&normalized);
        let name = usage.name.trim();
        if !kind.is_empty() && !name.is_empty() {
            resolutions
                .entry(kind)
                .or_default()
                .entry(name.into())
                .or_default();
        }
    }
    if !interface.trim().is_empty() {
        resolutions
            .entry("interface")
            .or_default()
            .entry(interface.trim().into())
            .or_default();
    }
    let requested_interfaces = resolutions
        .get("interface")
        .map(|names| names.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let mut known = BTreeSet::new();
    if !resolutions.is_empty() {
        for symbol in &index.symbols {
            control.check()?;
            if symbol.kind == "declaration" {
                match symbol.category.as_str() {
                    "api-function" | "api-scriptobject" => {
                        known.insert("api");
                        append_match(&mut resolutions, "api", &symbol.name, symbol);
                    }
                    "api-event" => {
                        known.insert("event");
                        append_match(&mut resolutions, "event", &symbol.name, symbol);
                    }
                    "mixin" => {
                        known.insert("mixin");
                        append_match(&mut resolutions, "mixin", &symbol.name, symbol);
                    }
                    _ => {}
                }
                if let Some(frame_type) = symbol.category.strip_prefix("xml-") {
                    known.insert("frame-type");
                    append_match(&mut resolutions, "frame-type", frame_type, symbol);
                    if !symbol.name.is_empty() {
                        known.insert("template");
                        append_match(&mut resolutions, "template", &symbol.name, symbol);
                    }
                }
            } else if symbol.kind == "header"
                && symbol.name.to_ascii_lowercase().starts_with("interface")
            {
                known.insert("interface");
                for requested in &requested_interfaces {
                    if interface_matches(&symbol.target, requested) {
                        append_match(&mut resolutions, "interface", requested, symbol);
                    }
                }
            }
        }
    }
    for (&kind, names) in &mut resolutions {
        for (name, resolution) in names {
            control.check()?;
            resolution.known = known.contains(kind) && !index.compatibility_gaps.contains(kind);
            if kind == "interface" && pin.repository == "wow-ui-source" {
                if let Some(expected) = source_interface(pin) {
                    resolution.known = true;
                    if resolution.matches.is_empty()
                        && interface_matches(&expected.to_string(), name)
                    {
                        resolution.matches.push(Match {
                            path: "selection-baseline".into(),
                            line: 0,
                            role: "project".into(),
                            signature: expected.to_string(),
                            detail: "Interface".into(),
                        });
                    }
                }
            }
        }
    }
    let mut result = Assessment {
        facts: vec![],
        unresolved: vec![],
        diagnostics: vec![],
    };
    let mut interface_seen = false;
    for usage in usages {
        control.check()?;
        let normalized = usage.kind.trim().to_ascii_lowercase();
        let kind = kind(&normalized);
        if kind == "interface" && usage.name.trim() == interface.trim() {
            interface_seen = true;
        }
        let reason = if kind.is_empty() {
            Some("unsupported compatibility evidence kind")
        } else if usage.name.trim().is_empty() {
            Some("dynamic or empty reference cannot be resolved statically")
        } else {
            None
        };
        if let Some(reason) = reason {
            result
                .unresolved
                .push(unresolved(pin, snapshot, usage, reason));
            continue;
        }
        let resolution = resolutions
            .get_mut(kind)
            .and_then(|names| names.get_mut(usage.name.trim()))
            .expect("requested compatibility reference");
        let Some(fact) = lookup(pin, snapshot, usage, kind, resolution) else {
            result.unresolved.push(unresolved(
                pin,
                snapshot,
                usage,
                "the indexed snapshot cannot prove presence or absence for this reference",
            ));
            continue;
        };
        if normalized == "api-candidate" && fact["exists"] == false {
            result.unresolved.push(unresolved(
                pin,
                snapshot,
                usage,
                "the call is not an indexed API and may be an AddOn-defined global",
            ));
            continue;
        }
        add_fact(&mut result, usage, fact);
    }
    if !interface.trim().is_empty() && !interface_seen {
        let mut usage = Usage::new("interface", interface.trim(), "", 0);
        usage.expression = interface.trim().into();
        let resolution = resolutions
            .get_mut("interface")
            .and_then(|names| names.get_mut(interface.trim()))
            .expect("requested interface");
        if let Some(fact) = lookup(pin, snapshot, &usage, "interface", resolution) {
            add_fact(&mut result, &usage, fact);
        } else {
            result.unresolved.push(unresolved(
                pin,
                snapshot,
                &usage,
                "the indexed snapshot has no authoritative Interface evidence",
            ));
        }
    }
    Ok(result)
}
fn add_fact(result: &mut Assessment, usage: &Usage, fact: Value) {
    if fact["exists"] == false {
        let (code, message) = missing(
            fact["kind"].as_str().unwrap(),
            fact["name"].as_str().unwrap(),
        );
        result.diagnostics.push(json!({"severity":"error","code":code,"file":usage.file,"line":usage.line,"column":usage.column,"message":message,"evidence":fact["evidence"]}));
    }
    result.facts.push(fact);
}
