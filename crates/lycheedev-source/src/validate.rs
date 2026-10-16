//! Addon closure validation against an immutable source pin. Syntax compilation,
//! structural observations and name presence are independently labeled facts.
use crate::{SourceTools, closure::Closure, index};
use lycheedev_core::{
    Result, ToolkitError,
    selection::{SourcePin, source_interface},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
pub fn toc(
    home: &Path,
    pin: &SourcePin,
    snapshot: &str,
    root: &Path,
    manifest: &str,
    tools: &SourceTools<'_>,
    semantic: bool,
) -> Result<Value> {
    toc_with_control(
        home,
        pin,
        snapshot,
        root,
        manifest,
        tools,
        semantic,
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
    semantic: bool,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Value> {
    toc_with_environment(
        home, pin, snapshot, root, manifest, tools, semantic, control, None,
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
    semantic: bool,
    control: &lycheedev_core::control::OperationControl,
    environment: Option<&SourcePin>,
) -> Result<Value> {
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
    let (inputs, inline) = crate::xml_inline::inputs(&closure.documents)?;
    let mut syntax =
        compiler.check_with_control(&home.join("tmp/source-validate"), &inputs, control)?;
    crate::xml_inline::remap(&mut syntax, &inline)?;
    syntax["checkedLua"] = json!(
        closure
            .documents
            .iter()
            .filter(|d| d.path.to_ascii_lowercase().ends_with(".lua"))
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
    let map = index::build_with_control(home, pin, control)?;
    control.check()?;
    let mut diagnostics:Vec<Value>=closure.issues.iter().map(|i|json!({"severity":i["severity"],"code":match i["code"].as_str().unwrap_or(""){"load_cycle"=>"circular_load_reference","duplicate_load"=>"duplicate_load_reference","load_unreadable"=>"load_file_unreadable","unsupported_load"=>"unsupported_load_type",code=>code},"file":i["path"],"line":i["line"],"column":0,"columnKnown":false,"message":i["message"],"evidence":{"reference":i["reference"]}})).collect();
    diagnostics.extend(
        syntax["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned(),
    );
    let mut unresolved = vec![];
    let mut facts = vec![];
    let mut checked = 0usize;
    let mut resolved = 0usize;
    let mut interfaces = vec![];
    if let Some(toc) = closure
        .documents
        .iter()
        .find(|d| d.path == closure.manifest)
    {
        for h in &toc.facts.headers {
            if h.key.eq_ignore_ascii_case("Interface") {
                interfaces.push(h.value.clone());
            }
        }
    }
    let interface = interfaces.join(",");
    if let Some(expected) = source_interface(pin) {
        let matched = interfaces.len() == 1
            && interface
                .split(',')
                .any(|s| s.trim() == expected.to_string());
        checked += 1;
        resolved += 1;
        facts.push(json!({"kind":"interface","name":interface,"exists":matched,"signature":expected.to_string(),"evidence":{"snapshot":snapshot,"sourceCommit":pin.exact_commit,"product":pin.product}}));
        if !matched {
            diagnostics.push(json!({"severity":"error","code":"toc_interface_mismatch","file":closure.manifest,"line":0,"column":0,"message":format!("TOC Interface {interface} does not match fixed client Interface {expected}"),"evidence":{"expected":expected}}));
        }
    } else {
        unresolved.push(json!({"kind":"interface","expression":interface,"file":closure.manifest,"line":0,"column":0,"reason":"source commit has no approved Interface observation","evidence":{"sourceCommit":pin.exact_commit}}));
    }
    let local_names: BTreeSet<_> = closure
        .documents
        .iter()
        .flat_map(|d| d.facts.declarations.iter().map(|s| s.name.clone()))
        .collect();
    let mut seen = BTreeSet::new();
    for doc in &closure.documents {
        for r in &doc.facts.relationships {
            if ![
                "call",
                "alias-call",
                "event-registration",
                "xml-inherits",
                "xml-function",
                "xml-method",
            ]
            .contains(&r.category.as_str())
                || !seen.insert((doc.path.clone(), r.line, r.to.clone(), r.category.clone()))
            {
                continue;
            }
            if local_names.contains(&r.to) {
                continue;
            }
            let (kind, missing, eligible) = match r.category.as_str() {
                "event-registration" => ("event", "event_not_found", vec!["api-event"]),
                "xml-inherits" => (
                    "template",
                    "xml_template_not_found",
                    vec!["xml-frame", "xml-template"],
                ),
                "xml-function" | "xml-method" => (
                    "api-candidate",
                    "compatibility_reference_not_found",
                    vec!["function", "api-function"],
                ),
                _ => (
                    "api",
                    "api_not_found",
                    vec!["api-function", "api-scriptobject", "api-callback"],
                ),
            };
            let sites: Vec<_> = map
                .symbols
                .iter()
                .filter(|s| {
                    s.kind == "declaration"
                        && s.name == r.to
                        && (eligible.contains(&s.category.as_str())
                            || (kind == "template" && s.category.starts_with("xml-")))
                })
                .take(21)
                .collect();
            if sites.len() > 20 {
                return Err(ToolkitError::new(
                    "codebase.reference_ambiguity_budget",
                    "Compatibility declaration ambiguity exceeds budget",
                    3,
                ));
            }
            checked += 1;
            let proven = r.confidence != "dynamic-unresolved"
                && (kind == "event"
                    || kind == "template"
                    || r.to.starts_with("C_")
                    || !sites.is_empty());
            if !proven {
                unresolved.push(json!({"kind":kind,"expression":r.to,"file":doc.path,"line":r.line,"column":0,"reason":if r.confidence=="dynamic-unresolved"{"dynamic expression"}else{"binding or namespace is not proven by structural source facts"},"evidence":{"confidence":r.confidence}}));
                continue;
            }
            resolved += 1;
            let exists = !sites.is_empty();
            facts.push(json!({"kind":kind,"name":r.to,"exists":exists,"signature":sites.first().map(|s|s.signature.clone()).unwrap_or_default(),"evidence":{"snapshot":snapshot,"sourceCommit":pin.exact_commit,"sites":sites.iter().map(|s|json!({"path":s.path,"line":s.line,"endLine":s.end_line,"symbolId":s.id})).collect::<Vec<_>>()}}));
            if !exists {
                diagnostics.push(json!({"severity":"error","code":missing,"file":doc.path,"line":r.line,"column":0,"message":format!("{} {} does not exist in the target snapshot",kind,r.to),"evidence":{"snapshot":snapshot,"sourceCommit":pin.exact_commit}}));
            }
        }
    }
    let mut load_closure = vec![];
    for doc in closure
        .documents
        .iter()
        .filter(|d| d.path != closure.manifest)
    {
        let loaded_by: Vec<_> = closure
            .steps
            .iter()
            .filter(|s| s["path"] == doc.path)
            .map(|s| json!({"file":s["from"],"line":s["line"],"kind":s["kind"]}))
            .collect();
        load_closure.push(json!({"path":doc.path,"type":Path::new(&doc.path).extension().and_then(|s|s.to_str()).unwrap_or(""),"loadedBy":loaded_by,"loadOrder":load_closure.len()}));
    }
    let semantic_result = if semantic {
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
        #[cfg(feature="emmy-prototype")]
        {
            if syntax["passed"]==true {
                crate::native_semantic::check(home,client,snapshot,compiler,&closure,&inputs,&inline,control)?
            }else{json!({"state":"not_run","passed":false,"complete":false,"reason":"syntax_compilation_failed"})}
        }
        #[cfg(not(feature="emmy-prototype"))]
        {
            let engine=tools.luals.ok_or_else(||ToolkitError::new("luals.unavailable","Semantic validation requires a verified bundled LuaLS runtime",3))?;
            crate::semantic::check(home, client, snapshot, engine, &inputs, &inline, control)?
        }
    } else {
        json!({"state":"not_requested"})
    };
    let valid = closure.valid
        && syntax["passed"] == true
        && (!semantic || semantic_result["passed"] == true)
        && !diagnostics.iter().any(|d| d["severity"] == "error");
    let complete = valid && unresolved.is_empty() && map.summary["complete"] == true
        && (!semantic || semantic_result["complete"] == true);
    Ok(
        json!({"valid":valid,"path":closure.root,"sourceId":pin.repository,"product":pin.product,"ref":pin.requested_ref,"requestedRef":pin.requested_ref,"matchedTag":"","resolvedCommit":pin.exact_commit,"toc":closure.manifest,"interface":interface,"checkedLua":syntax["checkedLua"],"checkedXml":closure.documents.iter().filter(|d|d.path.to_ascii_lowercase().ends_with(".xml")).count(),"loadClosure":load_closure,"diagnostics":diagnostics,"unresolved":unresolved,"coverage":{"checked":checked,"resolved":resolved,"unresolved":unresolved.len(),"source":map.coverage()},"facts":facts,"complete":complete,"syntaxCompilation":syntax,"structure":{"engine":"tree-sitter-lua-0.5.0","complete":closure.valid},"semantic":semantic_result,"load":closure.value(),"notChecked":if semantic {json!(["binding-identity","combat","taint","runtime-behavior"])}else{json!(["binding-identity","argument-types","combat","taint","runtime-behavior"])}}),
    )
}
