//! Strict multi-target validation: every target resolves independently to a
//! fixed SourcePin and keeps its own syntax/catalog/semantic evidence.
use crate::{
    SourceTools, git,
    request::{MatrixValidationRequest, SourceContext},
    validate,
};
use lycheedev_core::{
    Outcome, Result, ToolkitError, control::OperationControl, evidence, selection::SourcePin, storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_CONFIG_BYTES: usize = 1 << 20;
const MAX_TARGETS: usize = 32;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub toc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub product: String,
    #[serde(rename = "ref")]
    pub reference: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub path: String,
    pub targets: Vec<Target>,
}

/// The JSON file accepted by `source validate --matrix`, before resolving each
/// target's fixed source. Single-addon --path/--toc inputs are not JSON files.
pub fn schema() -> Value {
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"Source validation matrix file",
        "type":"object","additionalProperties":false,"required":["targets"],
        "properties":{
            "path":{"type":"string","default":"","description":"Addon root. An omitted or empty path uses the config file's directory; a relative path is resolved from that directory, not the caller's working directory."},
            "targets":{"type":"array","minItems":1,"maxItems":MAX_TARGETS,"items":{"$ref":"#/$defs/Target"}}
        },
        "$defs":{"Target":{
            "type":"object","additionalProperties":false,"required":["id","toc","product","ref"],
            "properties":{
                "id":{"type":"string","minLength":1,"description":"Unique within this matrix; retained on the target's report and coverage."},
                "toc":{"type":"string","minLength":1,"description":"Ordered TOC load manifest relative to the addon root."},
                "source":{"type":"string","default":"","description":"Configured source repository key. Omitted or empty selects wow-ui-source; source list enumerates available repositories and product tracks."},
                "product":{"type":"string","minLength":1,"description":"Exact product track in the selected source repository; targets are resolved independently."},
                "ref":{"type":"string","minLength":1,"description":"Exact 40-character source commit or full refs/heads/... or refs/tags/... reference. Retain the resolved fixed commit; never infer a revision from the addon directory."}
            }
        }},
        "x-limits":{"fileBytes":MAX_CONFIG_BYTES,"targetIds":"unique"},
        "x-result":"Each target retains its fixed source, TOC load closure, diagnostics, unresolved facts and coverage; aggregate success does not upgrade partial target evidence.",
        "examples":[{"path":"./addon","targets":[{"id":"retail","toc":"Main.toc","product":"retail","ref":"31c7f7b9cc79e56c986b365c06a6afbcf3c9177b"}]}]
    })
}
fn invalid(detail: &str) -> ToolkitError {
    ToolkitError::new(
        "codebase.invalid_matrix_config",
        format!("codebase.invalid_matrix_config: {detail}"),
        4,
    )
}
pub fn read_config(path: &Path, control: &OperationControl) -> Result<(Config, PathBuf)> {
    control.check()?;
    let mut file = fs::File::open(path)?;
    let result = (|| {
        control.check()?;
        if !file.metadata()?.is_file() {
            return Err(invalid("matrix config is not a regular file"));
        }
        let mut bytes = Vec::new();
        let mut bounded = (&mut file).take(MAX_CONFIG_BYTES as u64 + 1);
        let mut chunk = [0; 8192];
        loop {
            control.check()?;
            let count = bounded.read(&mut chunk)?;
            bytes.extend_from_slice(&chunk[..count]);
            control.check()?;
            if count == 0 { break; }
        }
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(invalid("matrix config exceeds 1 MiB"));
        }
        let config: Config = serde_json::from_slice(&bytes)
            .map_err(|e| invalid(&format!("cannot parse matrix config: {e}")))?;
        control.check()?;
        if config.targets.is_empty() {
            return Err(invalid("matrix targets must not be empty"));
        }
        if config.targets.len() > MAX_TARGETS {
            return Err(ToolkitError::new(
                "codebase.matrix_budget",
                "Matrix target count exceeds 32",
                3,
            ));
        }
        let mut ids = BTreeSet::new();
        for target in &config.targets {
            if target.id.is_empty()
                || target.toc.is_empty()
                || target.product.is_empty()
                || target.reference.is_empty()
            {
                return Err(invalid("each target requires id, toc, product, and ref"));
            }
            if !ids.insert(&target.id) {
                return Err(ToolkitError::new(
                    "codebase.duplicate_target_id",
                    format!(
                        "codebase.duplicate_target_id: duplicate matrix target id: {}",
                        target.id
                    ),
                    4,
                ));
            }
        }
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let root = if config.path.is_empty() {
            base.to_path_buf()
        } else if Path::new(&config.path).is_absolute() {
            PathBuf::from(&config.path)
        } else {
            base.join(&config.path)
        };
        Ok((config, std::path::absolute(root)?))
    })();
    storage::finish_file(file, result, control)
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map_or(&[], Vec::as_slice)
}
fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn diagnostic_key(d: &Value) -> String {
    format!(
        "{}\0{}\0{}\0{:010}\0{:010}\0{}",
        text(d, "severity"),
        text(d, "code"),
        text(d, "file"),
        d["line"].as_u64().unwrap_or(0),
        d["column"].as_u64().unwrap_or(0),
        text(d, "message")
    )
}
fn unresolved_key(d: &Value) -> (String, String, String, u64, u64, String) {
    (
        text(d, "kind").into(),
        text(d, "expression").into(),
        text(d, "file").into(),
        d["line"].as_u64().unwrap_or(0),
        d["column"].as_u64().unwrap_or(0),
        text(d, "reason").into(),
    )
}
fn facts(targets: &[Value], kinds: &[&str]) -> Value {
    let mut by_name = BTreeMap::<String, BTreeMap<String, Value>>::new();
    for target in targets {
        for fact in array(target, "facts") {
            if !kinds.contains(&text(fact, "kind").to_ascii_lowercase().as_str()) {
                continue;
            }
            by_name
                .entry(text(fact, "name").into())
                .or_default()
                .insert(
                    text(target, "id").into(),
                    json!({"exists":fact["exists"]==true,"signature":text(fact,"signature")}),
                );
        }
    }
    let mut shared = vec![];
    let mut differences = vec![];
    for (name, observed) in by_name {
        let mut values = BTreeMap::new();
        let mut first = None;
        let mut same = true;
        for target in targets {
            let id = text(target, "id");
            let value = observed
                .get(id)
                .cloned()
                .unwrap_or_else(|| json!({"exists":false,"signature":""}));
            if !observed.contains_key(id) || first.as_ref().is_some_and(|old| old != &value) {
                same = false;
            }
            first.get_or_insert_with(|| value.clone());
            values.insert(id, value);
        }
        if same {
            shared.push(name);
        } else {
            differences.push(json!({"name":name,"targets":values}));
        }
    }
    json!({"shared":shared,"differences":differences})
}
pub fn merge(path: &Path, targets: &[Value]) -> Value {
    let mut interfaces = BTreeMap::new();
    let mut unresolved = BTreeMap::new();
    let mut all_files = BTreeSet::new();
    let mut by_files = BTreeMap::new();
    let mut all_diagnostics = BTreeSet::new();
    let mut by_diagnostics = BTreeMap::new();
    for target in targets {
        let id = text(target, "id");
        let mut facts = array(target, "facts")
            .iter()
            .filter(|f| text(f, "kind").to_ascii_lowercase().contains("interface"))
            .cloned()
            .collect::<Vec<_>>();
        facts.sort_by_key(|f| {
            (
                text(f, "kind").to_owned(),
                text(f, "name").to_owned(),
                f["exists"] == true,
                text(f, "signature").to_owned(),
            )
        });
        let mut diagnostics = array(target, "diagnostics")
            .iter()
            .filter(|f| text(f, "code").to_ascii_lowercase().contains("interface"))
            .cloned()
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(diagnostic_key);
        interfaces.insert(
            id,
            json!({"declared":text(target,"interface"),"facts":facts,"diagnostics":diagnostics}),
        );
        let mut items = array(target, "unresolved").to_vec();
        items.sort_by_key(unresolved_key);
        unresolved.insert(id, items);
        let files = array(target, "loadClosure")
            .iter()
            .map(|f| text(f, "path").to_owned())
            .collect::<BTreeSet<_>>();
        all_files.extend(files.iter().cloned());
        by_files.insert(id, files);
        let mut diagnostics = BTreeMap::new();
        for d in array(target, "diagnostics") {
            let key = diagnostic_key(d);
            all_diagnostics.insert(key.clone());
            diagnostics.entry(key).or_insert_with(|| d.clone());
        }
        by_diagnostics.insert(id, diagnostics);
    }
    let mut shared_files = vec![];
    let mut only_files = BTreeMap::<_, Vec<String>>::new();
    let mut shared_diagnostics = vec![];
    let mut only_diagnostics = BTreeMap::<_, Vec<Value>>::new();
    for target in targets {
        let id = text(target, "id");
        only_files.insert(id, vec![]);
        only_diagnostics.insert(id, vec![]);
    }
    for file in all_files {
        if targets
            .iter()
            .all(|t| by_files[text(t, "id")].contains(&file))
        {
            shared_files.push(file);
        } else {
            for target in targets {
                let id = text(target, "id");
                if by_files[id].contains(&file) {
                    only_files.get_mut(id).unwrap().push(file.clone());
                }
            }
        }
    }
    for key in all_diagnostics {
        if targets
            .iter()
            .all(|t| by_diagnostics[text(t, "id")].contains_key(&key))
        {
            shared_diagnostics.push(by_diagnostics[text(&targets[0], "id")][&key].clone());
        } else {
            for target in targets {
                let id = text(target, "id");
                if let Some(d) = by_diagnostics[id].get(&key) {
                    only_diagnostics.get_mut(id).unwrap().push(d.clone());
                }
            }
        }
    }
    json!({"path":path,"valid":targets.iter().all(|t|t["valid"]==true),"targets":targets,"summary":{"apis":facts(targets,&["api"]),"events":facts(targets,&["event"]),"xml":facts(targets,&["template","mixin","frame-type"]),"interfaces":interfaces,"sharedFiles":shared_files,"targetOnlyFiles":only_files,"sharedDiagnostics":shared_diagnostics,"targetOnlyDiagnostics":only_diagnostics,"unresolved":unresolved}})
}
pub fn dispatch(
    ctx: &SourceContext,
    request: &MatrixValidationRequest,
    tools: SourceTools<'_>,
) -> Result<Outcome> {
    validate_with_resolver(ctx, request, tools, &mut |target| {
        git::resolve_source_with_control(
            &ctx.home,
            if target.source.is_empty() {
                "wow-ui-source"
            } else {
                &target.source
            },
            &target.product,
            &target.reference,
            &ctx.control,
        )
    })
}
pub fn validate_with_resolver(
    ctx: &SourceContext,
    request: &MatrixValidationRequest,
    tools: SourceTools<'_>,
    resolve: &mut dyn FnMut(&Target) -> Result<SourcePin>,
) -> Result<Outcome> {
    let mut targets = Vec::with_capacity(request.targets.len());
    let mut output_budget = OutputBudget(16 << 20);
    for target in &request.targets {
        ctx.control.check()?;
        let pin = resolve(target)?;
        git::validate_pin(&pin)?;
        if pin.product != target.product
            || pin.repository
                != if target.source.is_empty() {
                    "wow-ui-source"
                } else {
                    &target.source
                }
        {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Matrix resolver returned another target identity",
                4,
            ));
        }
        let mut value = validate::toc_with_control(
            &ctx.home,
            &pin,
            "",
            &request.root,
            &target.toc,
            &tools,
            request.semantic,
            &ctx.control,
        )?;
        value["id"] = json!(target.id);
        value["ref"] = json!(target.reference);
        value["requestedRef"] = json!(target.reference);
        serde_json::to_writer(&mut output_budget, &value).map_err(|_| {
            ToolkitError::new(
                "codebase.matrix_budget",
                "Combined matrix target output exceeds16MiB",
                3,
            )
        })?;
        targets.push(value);
    }
    let mut out = Outcome::new(merge(&request.root, &targets));
    let valid = out.data["valid"] == true;
    out.complete = capture_complete(valid, &targets);
    out.coverage = json!({"targets":targets.len(),"complete":targets.iter().all(|t|t["complete"]==true),"scope":"independently validated target closures; target semantic and unresolved coverage remain explicit"});
    out.provenance.push(evidence::commit_capture_bounded(
        &ctx.home,
        lycheedev_core::evidence::CaptureInput {
            bytes: &serde_json::to_vec(&out.data)?,
            maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"addon-matrix-validate","locator":request.configuration}),
            complete: out.complete,
            truncated: false,
        },
        &ctx.control,
    )?);
    if !valid {
        return Err(ToolkitError::new("codebase.addon_static_check_failed","Addon validation found invalid source",4).with_details(json!({"result":out.data,"captures":out.provenance,"context":{"coverage":out.coverage}})));
    }
    Ok(out)
}
fn capture_complete(valid: bool, targets: &[Value]) -> bool {
    // Matrix capture completeness means valid static compatibility coverage.
    // Native semantic.complete/coverage are independent evidence dimensions;
    // a passed checker can still report partial or unavailable semantic facts.
    valid
        && targets.iter().all(|target| {
            target["coverage"]["unresolved"] == 0 && target["sourceCoverage"]["complete"] == true
        })
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn valid_matrix_with_unresolved_or_source_gap_is_not_complete() {
        let complete =
            json!({"coverage":{"unresolved":0},"sourceCoverage":{"complete":true},"complete":true});
        assert!(capture_complete(true, &[complete.clone()]));
        assert!(!capture_complete(false, &[complete.clone()]));
        let mut unresolved = complete.clone();
        unresolved["coverage"]["unresolved"] = json!(1);
        assert!(!capture_complete(true, &[complete.clone(), unresolved]));
        let mut gap = complete.clone();
        gap["sourceCoverage"]["complete"] = json!(false);
        assert!(!capture_complete(true, &[complete.clone(), gap]));
        let mut semantic = complete;
        semantic["complete"] = json!(false);
        semantic["semantic"] =
            json!({"passed":true,"complete":false,"coverage":[{"state":"unavailable"}]});
        assert!(capture_complete(true, &[semantic]));
    }
}
struct OutputBudget(usize);
impl Write for OutputBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("matrix output budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
