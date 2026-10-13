//! Fixed ClientPin API definitions and authenticated frozen-input LuaLS check.
use crate::{environment, luals, xml_inline};
use lycheedev_core::{Result, control::OperationControl, evidence, selection::SourcePin};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
pub fn check(
    home: &Path,
    client: &SourcePin,
    snapshot: &str,
    runtime: &luals::Runtime,
    files: &BTreeMap<String, Vec<u8>>,
    inline: &BTreeMap<String, xml_inline::Script>,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    let catalog = environment::from_pin(home, client, control)?;
    let mut frozen = BTreeMap::new();
    let mut original = BTreeMap::new();
    for (path, bytes) in files {
        control.check()?;
        let canonical = format!("{}.lua", &path[..path.len() - 4]);
        if frozen.insert(canonical.clone(), bytes.clone()).is_some() {
            return Err(lycheedev_core::ToolkitError::new(
                "codebase.closure_integrity",
                "Case-normalized LuaLS input collision",
                4,
            ));
        }
        original.insert(canonical, path.clone());
    }
    let checked = runtime.check_artifacts_with_control(
        &home.join("tmp/source-semantic"),
        &frozen,
        catalog.definitions.as_bytes(),
        control,
    )?;
    let mut result = checked.assessment;
    result["positionEncoding"] = json!("utf-16");
    result["positionConvention"] =
        json!("zero-based lines and UTF16 characters; XML-inline segmented original byte mappings retain generated regions");
    if let Some(diagnostics) = result["diagnostics"].as_array_mut() {
        for d in diagnostics {
            let name = original
                .get(d["path"].as_str().unwrap_or(""))
                .cloned()
                .ok_or_else(|| {
                    lycheedev_core::ToolkitError::new(
                        "luals.check_failed",
                        "Diagnostic missing frozen input mapping",
                        5,
                    )
                })?;
            d["path"] = json!(name);
            if let Some(script) = inline.get(&name) {
                let mapped=script.position_from_utf16(&d["range"])?;
                d.as_object_mut().expect("diagnostic object").extend(mapped.as_object().expect("mapped position").clone());
            } else {
                d["positionAccuracy"] = json!("utf16-character");
            }
        }
    }
    let mut inputs = vec![];
    for (name, raw) in files {
        control.check()?;
        let provenance = json!({"kind":"source-semantic-input","locator":name,"snapshot":snapshot,"sourceCommit":client.exact_commit});
        let capture = evidence::commit_capture(home, raw, "text/x-lua", provenance, true, false)?;
        inputs.push(json!({"path":name,"blob":capture["blob"],"capture":capture}));
    }
    let mut artifacts = BTreeMap::new();
    let mut captures = BTreeMap::new();
    for (name, bytes, media) in [
        ("definitions", catalog.definitions.as_bytes(), "text/x-lua"),
        (
            "configuration",
            checked.configuration.as_slice(),
            "application/json",
        ),
        (
            "rawReport",
            checked.raw_report.as_slice(),
            "application/json",
        ),
    ] {
        control.check()?;
        let capture = evidence::commit_capture(
            home,
            bytes,
            media,
            json!({"kind":"source-semantic-artifact","locator":name,"snapshot":snapshot,"sourceCommit":client.exact_commit}),
            true,
            false,
        )?;
        artifacts.insert(name, capture["blob"].clone());
        captures.insert(name, capture);
    }
    let mut samples: Vec<_> = catalog
        .coverage
        .gaps
        .iter()
        .take(200)
        .map(|g| format!("{}:{}: {}", g.path, g.line, g.code))
        .collect();
    samples.sort();
    result["definitions"] = json!({"state":if catalog.coverage.complete{"complete"}else{"partial"},"documents":catalog.coverage.documents,"functions":catalog.mappings.len(),"skipped":catalog.coverage.gaps.len(),"skippedSample":samples,"truncated":catalog.coverage.truncated||catalog.coverage.gaps.len()>200,"sources":[]});
    result["inputs"] = json!(inputs);
    result["artifacts"] = json!(artifacts);
    result["captures"] = json!(captures);
    result["environment"] = json!({"manifest":catalog.manifest,"coverage":{"complete":catalog.coverage.complete,"records":catalog.facts.len(),"gaps":catalog.coverage.gaps.len()},"sourceSpanAccuracy":"utf8-byte"});
    result["notChecked"] = json!([
        "unknown or dynamic API metadata and client-specific globals",
        "injected frame globals",
        "external addon dependencies",
        "TOC execution order and addon varargs identity",
        "dynamic code",
        "combat, taint, secret values and runtime behavior"
    ]);
    Ok(result)
}
