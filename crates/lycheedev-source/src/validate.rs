//! The fixed Go single-addon and matrix contracts deliberately differ. Both
//! retain the independent Lua compiler and native XML/Lua semantic pipeline.
mod check;
mod compat;
#[cfg(test)]
mod tests;
mod usage;

use crate::{SourceTools, closure::Closure, index};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, selection::SourcePin};
use serde_json::{Value, json};
use std::path::Path;

struct Prepared {
    closure: Closure,
    map: index::Index,
    syntax: Value,
}
#[allow(clippy::too_many_arguments)]
fn prepare(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    root: &Path,
    manifest: &str,
    tools: &SourceTools<'_>,
    control: &OperationControl,
) -> Result<Prepared> {
    control.check()?;
    // Both fixed Go public usecases ensure the pinned index before inspecting
    // addon files. In particular an unavailable source must not publish a Lua
    // syntax capture or archive a closure before returning its first error.
    let map = index::build_with_control(home, pin, control)?;
    control.check()?;
    let compiler = tools.luac.ok_or_else(|| {
        ToolkitError::new(
            "luac.unavailable",
            "A verified bundled Lua 5.1.5 compiler is required",
            3,
        )
    })?;
    let closure = Closure::inspect_with_control(home, root, manifest, control)?;
    control.check()?;
    let (inputs, inline) = crate::xml_inline::syntax_inputs(&closure.documents, control)?;
    let mut syntax =
        compiler.check_with_control(&home.join("tmp/source-validate"), &inputs, control)?;
    crate::xml_inline::remap(&mut syntax, &inline)?;
    syntax["checkedLua"] = json!(
        closure
            .documents
            .iter()
            .filter(|doc| doc.path.to_ascii_lowercase().ends_with(".lua"))
            .count()
    );
    let audit = lycheedev_core::evidence::commit_capture_bounded(
        home,
        lycheedev_core::evidence::CaptureInput {
            bytes: &serde_json::to_vec(&syntax)?,
            maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"lua-syntax","locator":manifest,"snapshot":snapshot,"sourceCommit":pin.exact_commit}),
            complete: true,
            truncated: false,
        },
        control,
    )?;
    if let Some(rows) = syntax["diagnostics"].as_array_mut() {
        for row in rows {
            if let Some(object) = row.as_object_mut() {
                object.remove("rawError");
            }
            row["auditCapture"] = audit["id"].clone();
        }
    }
    syntax["auditCapture"] = audit;
    control.check()?;
    Ok(Prepared {
        closure,
        map,
        syntax,
    })
}

#[allow(clippy::too_many_arguments)]
fn semantic(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    tools: &SourceTools<'_>,
    requested: bool,
    control: &OperationControl,
    environment: Option<&SourcePin>,
    prepared: &Prepared,
) -> Result<Value> {
    if !requested {
        return Ok(json!({"state":"not_requested"}));
    }
    let client = if pin.repository == "wow-ui-source" {
        pin
    } else {
        environment.ok_or_else(|| {
            ToolkitError::new(
                "codebase.client_source_pin_required",
                "Semantic addon validation requires a fixed ClientPin",
                3,
            )
        })?
    };
    #[cfg(feature = "emmy-prototype")]
    {
        if prepared.syntax["passed"] == true {
            // Native XML profile authentication belongs to an explicit
            // semantic request. The public syntax pass accepts Go XML tokens.
            let (inputs, inline) = crate::xml_inline::inputs(&prepared.closure.documents)?;
            control.check()?;
            crate::native_semantic::check(
                home,
                client,
                snapshot,
                tools.luac.expect("verified compiler in preparation"),
                &prepared.closure,
                &inputs,
                &inline,
                control,
            )
        } else {
            Ok(
                json!({"state":"not_run","passed":false,"complete":false,"reason":"syntax_compilation_failed"}),
            )
        }
    }
    #[cfg(not(feature = "emmy-prototype"))]
    {
        let engine = tools.luals.ok_or_else(|| {
            ToolkitError::new(
                "luals.unavailable",
                "Semantic validation requires a verified bundled LuaLS runtime",
                3,
            )
        })?;
        let (inputs, inline) = crate::xml_inline::inputs(&prepared.closure.documents)?;
        control.check()?;
        crate::semantic::check(home, client, snapshot, engine, &inputs, &inline, control)
    }
}

fn diagnostics(prepared: &Prepared, matrix: bool) -> Vec<Value> {
    let mut values=prepared.closure.issues.iter().map(|issue|{
        let code=match issue["code"].as_str().unwrap_or(""){
            "load_cycle"=>"circular_load_reference","duplicate_load"=>"duplicate_load_reference",
            "load_unreadable"|"load_reference"=>"load_file_unreadable","unsupported_load"=>"unsupported_load_type",
            "syntax"=>if issue["path"].as_str().unwrap_or("").to_ascii_lowercase().ends_with(".xml"){"xml_parse_failed"}else{"lua_parse_failed"},other=>other,
        };
        let mut evidence=json!({});
        if issue["reference"].as_str().is_some_and(|value|!value.is_empty()){evidence["reference"]=issue["reference"].clone();}
        let severity=match code {
            "circular_load_reference" if !matrix=>json!("error"),
            "circular_load_reference"|"duplicate_load_reference"=>json!("warning"),
            _=>issue["severity"].clone(),
        };
        json!({"severity":severity,"code":code,"file":issue["path"],"line":issue["line"],"column":0,"message":issue["message"],"evidence":evidence})
    }).collect::<Vec<_>>();
    values.extend(
        prepared.syntax["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned(),
    );
    values
}

/// The single --snapshot/--path/--toc route retains Go CheckClosure's
/// source-present/unresolved result, rather than the matrix's absence facts.
#[allow(clippy::too_many_arguments)]
pub fn addon_with_environment(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    root: &Path,
    manifest: &str,
    tools: &SourceTools<'_>,
    requested: bool,
    control: &OperationControl,
    environment: Option<&SourcePin>,
) -> Result<Value> {
    let prepared = prepare(home, pin, snapshot, root, manifest, tools, control)?;
    let mut result = check::assess(pin, &prepared.closure, &prepared.map, control)?;
    let semantic = semantic(
        home,
        pin,
        snapshot,
        tools,
        requested,
        control,
        environment,
        &prepared,
    )?;
    result["staticValid"] = json!(
        result["staticValid"] == true
            && prepared.syntax["passed"] == true
            && (!requested || semantic["passed"] == true)
    );
    // Go's semantic assessment never certifies complete WoW/runtime coverage.
    // Native checker-family coverage remains available in its own exact report.
    result["complete"] =
        json!(result["complete"] == true && prepared.syntax["passed"] == true && !requested);
    result["load"]["compatibilityComplete"] = result["complete"].clone();
    if requested {
        result["checks"]
            .as_array_mut()
            .expect("check list")
            .push(json!(if cfg!(feature = "emmy-prototype") {
                "emmy-native-frozen-closure"
            } else {
                "luals-frozen-closure"
            }));
    }
    result["diagnostics"] = json!(diagnostics(&prepared, false));
    result["syntaxCompilation"] = prepared.syntax;
    result["structure"] =
        json!({"engine":"tree-sitter-lua-0.5.0","complete":prepared.closure.valid});
    result["semantic"] = semantic;
    Ok(result)
}

/// Matrix TOC compatibility entry. Kept separately from the single-addon
/// endpoint so consumers cannot accidentally exchange their public contracts.
pub fn toc(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    root: &Path,
    manifest: &str,
    tools: &SourceTools<'_>,
    requested: bool,
) -> Result<Value> {
    toc_with_control(
        home,
        pin,
        snapshot,
        root,
        manifest,
        tools,
        requested,
        &Default::default(),
    )
}
#[allow(clippy::too_many_arguments)]
pub fn toc_with_control(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    root: &Path,
    manifest: &str,
    tools: &SourceTools<'_>,
    requested: bool,
    control: &OperationControl,
) -> Result<Value> {
    toc_with_environment(
        home, pin, snapshot, root, manifest, tools, requested, control, None,
    )
}
#[allow(clippy::too_many_arguments)]
pub fn toc_with_environment(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    root: &Path,
    manifest: &str,
    tools: &SourceTools<'_>,
    requested: bool,
    control: &OperationControl,
    environment: Option<&SourcePin>,
) -> Result<Value> {
    let prepared = prepare(home, pin, snapshot, root, manifest, tools, control)?;
    let (usages, mut unresolved) = matrix_usages(&prepared.closure, control)?;
    let interface = prepared
        .closure
        .documents
        .iter()
        .filter(|doc| doc.path == prepared.closure.manifest)
        .flat_map(|doc| &doc.facts.headers)
        .filter(|header| header.key.eq_ignore_ascii_case("Interface"))
        .map(|header| header.value.trim())
        .last()
        .unwrap_or("");
    let compatibility = compat::assess(pin, snapshot, &prepared.map, &usages, interface, control)?;
    unresolved.extend(compatibility.unresolved);
    let mut diagnostics = diagnostics(&prepared, true);
    diagnostics.extend(compatibility.diagnostics);
    let mut load_closure = vec![];
    for doc in prepared
        .closure
        .documents
        .iter()
        .filter(|doc| doc.path != prepared.closure.manifest)
    {
        control.check()?;
        let loaded_by = prepared
            .closure
            .steps
            .iter()
            .filter(|step| {
                step["path"] == doc.path
                    && step["from"].as_str().is_some_and(|from| !from.is_empty())
            })
            .map(|step| json!({"file":step["from"],"line":step["line"],"kind":step["kind"]}))
            .collect::<Vec<_>>();
        load_closure.push(json!({"path":doc.path,"type":Path::new(&doc.path).extension().and_then(|value|value.to_str()).unwrap_or("").to_ascii_lowercase(),"loadedBy":loaded_by,"loadOrder":load_closure.len()}));
    }
    let semantic = semantic(
        home,
        pin,
        snapshot,
        tools,
        requested,
        control,
        environment,
        &prepared,
    )?;
    let valid = prepared.closure.valid
        && prepared.syntax["passed"] == true
        && (!requested || semantic["passed"] == true)
        && !diagnostics
            .iter()
            .any(|diagnostic| diagnostic["severity"] == "error");
    let complete = valid
        && unresolved.is_empty()
        && prepared.map.summary.complete
        && (!requested || semantic["complete"] == true);
    Ok(
        json!({"valid":valid,"path":prepared.closure.root,"sourceId":pin.repository,"product":pin.product,"ref":pin.requested_ref,"requestedRef":pin.requested_ref,"matchedTag":"","resolvedCommit":pin.exact_commit,
            "toc":prepared.closure.manifest,"interface":interface,"checkedLua":prepared.syntax["checkedLua"],"checkedXml":prepared.closure.documents.iter().filter(|doc|doc.path.to_ascii_lowercase().ends_with(".xml")).count(),
            "loadClosure":load_closure,"diagnostics":diagnostics,"unresolved":unresolved,"coverage":{"checked":usages.len()+usize::from(!interface.is_empty()),"resolved":compatibility.facts.len(),"unresolved":unresolved.len()},
            "sourceCoverage":prepared.map.coverage(),"facts":compatibility.facts,"complete":complete,"syntaxCompilation":prepared.syntax,"structure":{"engine":"tree-sitter-lua-0.5.0","complete":prepared.closure.valid},"semantic":semantic,"load":prepared.closure.value(),
            "notChecked":if requested{json!(["binding-identity","combat","taint","runtime-behavior"])}else{json!(["binding-identity","argument-types","combat","taint","runtime-behavior"])}
        }),
    )
}
fn matrix_usages(
    closure: &Closure,
    control: &OperationControl,
) -> Result<(Vec<usage::Usage>, Vec<Value>)> {
    let mut usages = vec![];
    let mut unresolved = vec![];
    for doc in &closure.documents {
        control.check()?;
        if doc.path == closure.manifest {
            continue;
        }
        let extension = Path::new(&doc.path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (found, undecided) = match extension.as_str() {
            // Go's usage parser is independent of bundled Lua 5.1 syntax
            // compilation (gopher-lua also parses goto/label statements).
            "lua" => usage::lua(&doc.path, &doc.bytes, control)?,
            "xml" => usage::xml(&doc.path, &doc.bytes, control)?,
            _ => continue,
        };
        usages.extend(found);
        unresolved.extend(undecided);
    }
    Ok((usage::dedupe(usages), unresolved))
}
