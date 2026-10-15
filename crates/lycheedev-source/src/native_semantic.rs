//! Native selected-value diagnostic checking over authenticated addon bytes and API facts.
use crate::{closure::Closure, environment, luac, native::*, xml_inline};
use emmylua_code_analysis::{CheckConfig, DiagnosticCode, Emmyrc, EmmyrcLuaVersion};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, evidence, selection::SourcePin, storage,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

const RULES: &str = "source-native-semantic-api-v5-xml-objects";
const CHECKER_FAMILIES: [&str; 8] = [
    "param-type-mismatch",
    "return-type-mismatch",
    "missing-parameter",
    "redundant-parameter",
    "call-non-callable",
    "missing-return-value",
    "redundant-return-value",
    "missing-return",
];

#[allow(clippy::too_many_arguments)]
pub fn check(
    home: &Path,
    client: &SourcePin,
    snapshot: &str,
    compiler: &luac::Runtime,
    closure: &Closure,
    files: &BTreeMap<String, Vec<u8>>,
    inline: &BTreeMap<String, xml_inline::Script>,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    let catalog = environment::from_pin(home, client, control)?;
    let mut result = if files.is_empty() {
        json!({"passed":true,"complete":true,"diagnostics":[],"coverage":[],"gaps":[]})
    } else {
        analyze(client, compiler, closure, files, inline, &catalog, control)?
    };
    let mut inputs = Vec::new();
    for (path, bytes) in files {
        control.check()?;
        let capture = evidence::commit_capture(
            home,
            bytes,
            "text/x-lua",
            json!({"kind":"source-semantic-input","locator":path,"snapshot":snapshot,"sourceCommit":client.exact_commit}),
            true,
            false,
        )?;
        inputs.push(json!({"path":path,"blob":capture["blob"],"capture":capture,"generated":inline.contains_key(path)}));
    }
    let descriptors = serialize_json(
        &json!({"manifest":catalog.manifest,"records":catalog.facts}),
        control,
        32 << 20,
    )?;
    let source_maps = inline
        .values()
        .map(|script| {
            let mut mapping = script.mapping_manifest();
            mapping["xmlBlob"] = closure
                .documents
                .iter()
                .find(|document| document.path == script.path)
                .expect("inline document in inspected closure")
                .blob
                .clone();
            mapping
        })
        .collect::<Vec<_>>();
    let source_maps = serialize_json(
        &json!({"rules":xml_inline::SOURCE_MAP_RULES,"inputs":source_maps}),
        control,
        32 << 20,
    )?;
    let configuration = serde_json::to_vec(
        &json!({"rules":RULES,"language":"Lua51","checkerFamilies":CHECKER_FAMILIES,"sourceExecution":false,"entryArguments":"unspecified_except_XML_self"}),
    )?;
    let raw = serialize_json(&result, control, 32 << 20)?;
    let mut artifacts = BTreeMap::new();
    let mut captures = BTreeMap::new();
    for (name, bytes) in [
        ("definitions", descriptors.as_slice()),
        ("configuration", configuration.as_slice()),
        ("rawReport", raw.as_slice()),
        ("sourceMaps", source_maps.as_slice()),
    ] {
        let capture = evidence::commit_capture(
            home,
            bytes,
            "application/json",
            json!({"kind":"source-semantic-artifact","locator":name,"snapshot":snapshot,"sourceCommit":client.exact_commit}),
            true,
            false,
        )?;
        artifacts.insert(name, capture["blob"].clone());
        captures.insert(name, capture);
    }
    result["engine"] = json!("emmy_native_common_facts");
    result["sourceExecution"] = json!(false);
    result["definitions"] = json!({"state":if catalog.coverage.complete{"complete"}else{"partial"},"documents":catalog.coverage.documents,"functions":catalog.facts.iter().filter(|record|record.kind=="function").count(),"skipped":catalog.coverage.gaps.len(),"truncated":catalog.coverage.truncated,"format":"authenticated_api_descriptors"});
    result["inputs"] = json!(inputs);
    result["artifacts"] = json!(artifacts);
    result["captures"] = json!(captures);
    result["environment"] = json!({"manifest":catalog.manifest,"coverage":catalog.coverage});
    result["positionEncoding"] = json!("utf-16");
    result["positionConvention"] = json!(
        "zero-based lines and UTF16 characters; XML byte origins retain exact, entity, normalized-newline and synthetic segments; BOM bytes do not occupy a display column"
    );
    result["sourceMapRules"] = json!(xml_inline::SOURCE_MAP_RULES);
    result["notChecked"] = json!([
        "other diagnostic families",
        "script-object nominal identity and inheritance",
        "unknown or dynamic API metadata",
        "XML native lifecycle timing and native UI type inheritance",
        "combat, taint and game runtime behavior"
    ]);
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn analyze(
    client: &SourcePin,
    compiler: &luac::Runtime,
    closure: &Closure,
    files: &BTreeMap<String, Vec<u8>>,
    inline: &BTreeMap<String, xml_inline::Script>,
    catalog: &environment::Environment,
    control: &OperationControl,
) -> Result<Value> {
    let host =
        crate::native_research::api_environment(&catalog.facts, Some(&catalog.manifest), control)?;
    let mut config = Emmyrc::default();
    config.runtime.version = EmmyrcLuaVersion::Lua51;
    // Preserve the Source command's selected eight-code policy. The complete
    // owned registry also honors explicit file-level enables for other codes.
    config.diagnostics.disable = DiagnosticCode::all()
        .into_iter()
        .filter(|code| {
            !matches!(
                code,
                DiagnosticCode::ParamTypeMismatch
                    | DiagnosticCode::ReturnTypeMismatch
                    | DiagnosticCode::MissingParameter
                    | DiagnosticCode::RedundantParameter
                    | DiagnosticCode::CallNonCallable
                    | DiagnosticCode::MissingReturnValue
                    | DiagnosticCode::RedundantReturnValue
                    | DiagnosticCode::MissingReturn
            )
        })
        .collect();
    config.diagnostics.enables = vec![
        DiagnosticCode::ParamTypeMismatch,
        DiagnosticCode::ReturnTypeMismatch,
        DiagnosticCode::MissingParameter,
        DiagnosticCode::RedundantParameter,
        DiagnosticCode::CallNonCallable,
        DiagnosticCode::MissingReturnValue,
        DiagnosticCode::RedundantReturnValue,
        DiagnosticCode::MissingReturn,
    ];
    let mut source_files = Vec::new();
    for (path, bytes) in files {
        control.check()?;
        let text = String::from_utf8(bytes.clone()).map_err(|_| {
            ToolkitError::new("codebase.source_integrity", "semantic input is not UTF8", 4)
        })?;
        source_files.push(SourceFileInput {
            path: path.clone(),
            content_sha256: storage::sha256(bytes),
            analysis_utf8: text.strip_prefix('\u{feff}').unwrap_or(&text).into(),
            map: if text.starts_with('\u{feff}') {
                SourceMap::Utf8BomPrefix
            } else {
                SourceMap::Identity
            },
            original_utf8: text,
        });
    }
    let luac = compiler.manifest();
    let mut xml_inputs = Vec::new();
    for script in inline.values() {
        control.check()?;
        xml_inputs.push(json!({"path":script.key,"xmlSHA256":script.xml_sha256,"sourceMapSHA256":storage::sha256(&serde_json::to_vec(&script.mapping_manifest())?)}));
    }
    let xml_program = Arc::new(crate::xml_load::Program::from_closure(closure, control)?);
    let admitted = SourceInputs::admit(
        SourceInputSpec {
            files: source_files,
            workspace_root: PathBuf::from("C:/lycheedev/addon-semantic"),
            source_identity: json!({"rules":RULES,"client":client,"manifest":closure.manifest,"xmlSourceMapRules":xml_inline::SOURCE_MAP_RULES,"xmlInputs":xml_inputs}),
            phase: "after_load".into(),
            ordered_occurrences: vec![],
            config,
            host,
            luac: LuacIdentity {
                version: luac.version.clone(),
                platform: luac.platform.clone(),
                executable_sha256: luac.sha256.clone(),
                executable_bytes: luac.bytes,
            },
            limits: SourceLimits::research(),
        },
        control,
    )?;
    let prepared = prepare(admitted, control)?;
    let entries = prepared.entry_catalog(control)?.to_vec();
    let ordered = xml_program.lua_files();
    let graph =
        prepared.build_xml_plan("addon-semantic-load", xml_program.clone(), None, control)?;
    let mut diagnostics = Vec::new();
    let mut coverage = Vec::new();
    let mut gaps = graph
        .xml
        .as_ref()
        .map(|facts| facts.gaps.clone())
        .unwrap_or_default();
    for (occurrence, path) in ordered.iter().enumerate() {
        collect(
            &prepared,
            &graph,
            NativeViewSelection::Load(occurrence),
            path,
            None,
            &entries,
            files,
            inline,
            &mut diagnostics,
            &mut coverage,
            &mut gaps,
            control,
        )?;
    }
    let instances = graph
        .native_invocations
        .as_ref()
        .map(|facts| facts.closures.clone())
        .unwrap_or_default();
    let handlers = graph
        .xml
        .as_ref()
        .map(|facts| facts.handlers.clone())
        .unwrap_or_default();
    let mut handled = BTreeSet::new();
    let mut xml_entries = Vec::new();
    for handler in &handlers {
        control.check()?;
        if matches!(handler.target, crate::xml_load::HandlerTarget::Empty) {
            continue;
        }
        let Some(instance) = handler.closure.and_then(|id| instances.get(id)) else {
            gaps.push(json!({"code":"xml_handler_callable_unavailable","registration":handler}));
            continue;
        };
        let function = &graph.functions[instance.function];
        let candidate = entries
            .iter()
            .find(|candidate| candidate.closure == function.closure)
            .expect("loaded XML handler source closure");
        handled.insert(candidate.id.clone());
        let request = EntryRequest {
            candidate: candidate.id.clone(),
            load_occurrence: Some(instance.load_occurrence),
            creation_context: Some(instance.creation_context.clone()),
            arguments: EntryArguments::XmlHandler(handler.id),
        };
        let entry = match prepared.build_xml_plan(
            "addon-semantic-xml-entry",
            xml_program.clone(),
            Some(request),
            control,
        ) {
            Ok(graph) => graph,
            Err(error) => {
                control.check()?;
                gaps.push(
                    json!({"code":error.code,"detail":error.message,"xmlHandler":handler.id}),
                );
                continue;
            }
        };
        xml_entries.push(json!({"handler":handler.id,"instance":handler.instance,"snapshot":entry.snapshot_key,"entries":entry.native_invocations.as_ref().map(|facts|&facts.entries),"selfOrigin":entry.values[handler.self_value].origin}));
        let frame = entry
            .native_invocations
            .as_ref()
            .and_then(|facts| facts.entries.first())
            .and_then(|entry| entry.frame);
        if let Some(frame) = frame {
            collect(
                &prepared,
                &entry,
                NativeViewSelection::Entry(frame),
                &candidate.closure.file,
                Some(&candidate.closure),
                &entries,
                files,
                inline,
                &mut diagnostics,
                &mut coverage,
                &mut gaps,
                control,
            )?;
        } else {
            gaps.push(json!({"code":"xml_handler_frame_unavailable","xmlHandler":handler.id}));
        }
    }
    for candidate in &entries {
        control.check()?;
        if handled.contains(&candidate.id)
            || inline
                .get(&candidate.closure.file)
                .is_some_and(|script| script.kind == "handler")
        {
            continue;
        }
        let matching = instances
            .iter()
            .filter(|instance| graph.functions[instance.function].closure == candidate.closure)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            gaps.push(json!({"code":"entry_not_created_by_load","candidate":candidate}));
            continue;
        }
        for instance in matching {
            let request = EntryRequest {
                candidate: candidate.id.clone(),
                load_occurrence: Some(instance.load_occurrence),
                creation_context: Some(instance.creation_context.clone()),
                arguments: EntryArguments::Unspecified,
            };
            let entry = match prepared.build_xml_plan(
                "addon-semantic-entry",
                xml_program.clone(),
                Some(request),
                control,
            ) {
                Ok(graph) => graph,
                Err(error) => {
                    control.check()?;
                    gaps.push(
                        json!({"code":error.code,"detail":error.message,"candidate":candidate}),
                    );
                    continue;
                }
            };
            let frame = entry
                .native_invocations
                .as_ref()
                .and_then(|facts| facts.entries.first())
                .and_then(|entry| entry.frame);
            if let Some(frame) = frame {
                collect(
                    &prepared,
                    &entry,
                    NativeViewSelection::Entry(frame),
                    &candidate.closure.file,
                    Some(&candidate.closure),
                    &entries,
                    files,
                    inline,
                    &mut diagnostics,
                    &mut coverage,
                    &mut gaps,
                    control,
                )?;
            } else {
                gaps.push(json!({"code":"entry_frame_unavailable","candidate":candidate}));
            }
        }
    }
    diagnostics.sort_by_key(Value::to_string);
    diagnostics.dedup();
    let mut observed = BTreeSet::new();
    for row in &coverage {
        observed.insert((
            row["generatedSite"].to_string(),
            row["operation"].as_str().unwrap_or_default().to_owned(),
        ));
    }
    let query = prepared.source_queries(control)?;
    let mut cursor = None;
    loop {
        let page = query.calls(
            QueryTarget::All,
            PageRequest {
                limit: 200,
                bytes: 2 << 20,
                cursor,
            },
        )?;
        for call in page.rows {
            if inline
                .get(&call.site.file)
                .is_some_and(|script| script.kind == "handler")
                && !instances.iter().any(|instance| {
                    graph.functions[instance.function].closure.file == call.site.file
                })
            {
                continue;
            }
            for operation in ["argument_check", "argument_count_check", "callable_check"] {
                if !observed.contains(&(json!(call.site).to_string(), operation.to_owned())) {
                    gaps.push(json!({"code":"call_check_unavailable","operation":operation,"site":call.site}));
                }
            }
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let complete = gaps.is_empty() && !coverage.iter().any(|row| row["state"] == "unavailable");
    Ok(
        json!({"state":if complete{"checked"}else{"partial"},"passed":diagnostics.is_empty(),"complete":complete,"diagnostics":diagnostics,"coverage":coverage,"gaps":gaps,"inputIdentity":prepared.inputs().identity_sha256(),"loadPlan":LoadPlan::Known(ordered),"xmlProgram":xml_program.as_ref(),"xmlFacts":graph.xml,"xmlEntries":xml_entries,"entryPolicy":"hypothetical_after_load; XML self is the exact instance; remaining arguments unspecified","checkerFamilies":CHECKER_FAMILIES}),
    )
}

fn owns(site: &Site, file: &str, scope: Option<&Site>, entries: &[EntryCandidate]) -> bool {
    if site.file != file {
        return false;
    }
    let owner = entries
        .iter()
        .filter(|entry| {
            entry.closure.file == site.file
                && entry.closure.start <= site.start
                && entry.closure.end >= site.end
        })
        .min_by_key(|entry| entry.closure.end - entry.closure.start);
    owner.map(|entry| &entry.closure) == scope
}

#[allow(clippy::too_many_arguments)]
fn collect(
    prepared: &PreparedSnapshot,
    graph: &Graph,
    view: NativeViewSelection,
    file: &str,
    scope: Option<&Site>,
    entries: &[EntryCandidate],
    files: &BTreeMap<String, Vec<u8>>,
    inline: &BTreeMap<String, xml_inline::Script>,
    diagnostics: &mut Vec<Value>,
    coverage: &mut Vec<Value>,
    gaps: &mut Vec<Value>,
    control: &OperationControl,
) -> Result<()> {
    let provider = match prepared.freeze(graph, view, control) {
        Ok(provider) => provider,
        Err(error) => {
            control.check()?;
            gaps.push(json!({"code":error.code,"detail":error.message,"file":file,"scope":scope}));
            return Ok(());
        }
    };
    let session =
        lycheedev_emmy_adapter::session::OwnedBindingSession::new_checked(provider, || {
            control.check()
        })?;
    let mut check = || control.check();
    let mut query = lycheedev_emmy_adapter::session::OwnedQuery::new_with_contribution_order(
        prepared.analysis(),
        &session,
        prepared.files()[file],
        &mut check,
        prepared.contribution_order(),
    )
    .with_workspace_facts(prepared.workspace_facts());
    let mut run = lycheedev_emmy_adapter::session::DiagnosticRun::default();
    query
        .diagnose_into(Arc::new(CheckConfig::new(prepared.config())), &mut run)
        .map_err(|error| match error {
            lycheedev_emmy_adapter::session::OwnedQueryFailure::Control(error) => error,
            lycheedev_emmy_adapter::session::OwnedQueryFailure::Continue(missing) => {
                ToolkitError::new(
                    "source.native_internal",
                    format!("owned query admission invariant: {missing:?}"),
                    8,
                )
            }
        })?;
    let rows = run.diagnostics;
    control.check()?;
    for row in rows {
        let mapped = prepared.inputs().original_range(file, row.range)?;
        let site = Site {
            file: file.into(),
            start: u32::from(mapped.start()),
            end: u32::from(mapped.end()),
        };
        // Native callback parameter diagnostics belong to the expression that
        // supplied the expected signature, while their displayed range stays
        // on the actual excessive parameter.
        let owner = if let Some(owner) = row
            .data
            .as_ref()
            .and_then(|data| data.get("bindingCheckOwner"))
        {
            let range = rowan::TextRange::new(
                (owner["start"].as_u64().expect("native owner start") as u32).into(),
                (owner["end"].as_u64().expect("native owner end") as u32).into(),
            );
            let mapped = prepared.inputs().original_range(file, range)?;
            Site {
                file: file.into(),
                start: u32::from(mapped.start()),
                end: u32::from(mapped.end()),
            }
        } else {
            site.clone()
        };
        if !owns(&owner, file, scope, entries) {
            continue;
        }
        let mut output = position(&site, files, inline)?;
        if row.data.is_some() {
            output["nativeData"] = json!(row.data);
        }
        output["code"] = json!(row.code.get_name());
        output["message"] = json!(row.message);
        output["severity"] = serde_json::to_value(row.severity)?;
        output["contextIdentity"] = json!(session.identity());
        diagnostics.push(output);
    }
    for row in session.coverage_checked(|| control.check())? {
        let site = prepared.source_site(row.site)?;
        if !owns(&site, file, scope, entries) {
            continue;
        }
        let mut output = position(&site, files, inline)?;
        output["operation"] = json!(row.operation);
        output["state"] = json!(row.state);
        output["reason"] = json!(row.reason);
        output["proof"] = json!(row.proof);
        output["contextIdentity"] = json!(session.identity());
        if let Some(parameter) = row.parameter {
            output["parameter"] = json!(parameter);
        }
        if let Some(index) = row.parameter_index {
            output["parameterIndex"] = json!(index);
        }
        if let Some(argument) = row.argument {
            output["argument"] = position(&prepared.source_site(argument)?, files, inline)?;
        }
        coverage.push(output);
    }
    Ok(())
}

fn position(
    site: &Site,
    files: &BTreeMap<String, Vec<u8>>,
    inline: &BTreeMap<String, xml_inline::Script>,
) -> Result<Value> {
    let bytes = xml_inline::source_map::Span::new(site.start as usize, site.end as usize);
    if let Some(script) = inline.get(&site.file) {
        let mut result = script.position(bytes)?;
        result["generatedSite"] = json!(site);
        return Ok(result);
    }
    let text = std::str::from_utf8(&files[&site.file])
        .map_err(|error| ToolkitError::new("codebase.source_integrity", error.to_string(), 4))?;
    let range = xml_inline::source_map::utf16_range(text, bytes)
        .map_err(|error| ToolkitError::new("source.native_source_range", error, 4))?;
    Ok(
        json!({"path":site.file,"range":range,"bytes":bytes,"positionAccuracy":"utf16-character","generatedSite":site}),
    )
}
