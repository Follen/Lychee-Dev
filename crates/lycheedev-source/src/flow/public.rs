//! Public pinned-source flow uses the common graph in an owned CLI worker.
use crate::{SourceTools, environment, git, index, native::*, relations};
use lycheedev_core::{Invocation, Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

const INPUT_BYTES: usize = 32 << 20;
const FILE_BYTES: usize = 1 << 20;
const FILES: usize = 32;
const WORKER_INPUT_BYTES: usize = 64 << 20;
const WORKER_BYTES: usize = 2 * 1024 * 1024;
const WORKER_MILLIS: u64 = 2000;

#[derive(Serialize, Deserialize)]
struct Request {
    files: BTreeMap<String, String>,
    ordered: Option<Vec<String>>,
    source: Value,
    target_path: String,
    target_line: usize,
    luac: LuacIdentity,
    api: Vec<environment::ApiRecord>,
    environment_manifest: Option<environment::Manifest>,
    rules: BTreeMap<String, super::CallRule>,
    input_gaps: Vec<Value>,
    load_evidence: Value,
}

fn gap(code: &str, detail: impl Into<String>) -> Value {
    json!({"code":code,"detail":detail.into()})
}
fn incomplete(gaps: Vec<Value>) -> Value {
    json!({"rules_version":FLOW_RULES,"findings":{"rows":[],"has_more":false},"guarded":[],"gaps":gaps,"complete":false,"input_complete":false,"analysis_complete":false,"output_complete":true,"truncated":false})
}
mod load_plan;

/// Collect immutable pin bytes and actual compiler results before worker admission.
pub fn context_flow(
    inv: &Invocation,
    reading: &relations::Reading,
    tools: &SourceTools<'_>,
    facts: &[index::LoadFact],
) -> Result<Value> {
    inv.control.check()?;
    let runtime = tools.luac.ok_or_else(|| {
        ToolkitError::new(
            "flow.compiler_unavailable",
            "source context --flow requires the verified bundled Lua 5.1 compiler",
            3,
        )
    })?;
    let Some(path) = reading.data["symbol"]["path"].as_str() else {
        return Ok(incomplete(vec![gap(
            "selector_required",
            "select one source declaration with --symbol-id",
        )]));
    };
    let source = reading.pin.source.as_ref().expect("resolved source pin");
    if !path.to_ascii_lowercase().ends_with(".lua") {
        return Ok(incomplete(vec![gap(
            "source_entry_unavailable",
            "flow requires a Lua declaration; API metadata remains rule evidence",
        )]));
    }
    let source_identity = json!({"snapshot":reading.pin.id,"repository":source.repository,"product":source.product,"commit":source.exact_commit});
    let load_plan::Selection { mut ordered, mut input_gaps, input_bytes, load_evidence } = load_plan::collect(
        path,
        &reading.index,
        facts,
        &source_identity,
        &inv.control,
    )?;
    let mut names = ordered.clone().unwrap_or_else(|| vec![path.into()]);
    names.sort();
    names.dedup();
    if names.len() > FILES {
        input_gaps.push(gap(
            "input_file_budget",
            format!("{} load files exceed {FILES}", names.len()),
        ));
        names = vec![path.into()];
        ordered = None;
    }
    let mut files = BTreeMap::new();
    let mut total = input_bytes;
    for name in names {
        inv.control.check()?;
        let (raw, _) = git::blob_with_control(
            &inv.home,
            source,
            &name,
            git::MAX_SOURCE_BYTES,
            &inv.control,
        )?;
        total += raw.len();
        if raw.len() > FILE_BYTES || total > INPUT_BYTES {
            return Ok(incomplete(vec![gap(
                "input_byte_budget",
                format!(
                    "selected load closure exceeds {FILE_BYTES} bytes per file or {INPUT_BYTES} total bytes"
                ),
            )]));
        }
        let text = String::from_utf8(raw)
            .map_err(|_| ToolkitError::new("flow.invalid_input", "flow source must be UTF8", 4))?;
        files.insert(name, text);
    }
    let compiler_files = files
        .iter()
        .map(|(name, text)| (name.clone(), text.as_bytes().to_vec()))
        .collect();
    let syntax = runtime.check_with_control(
        &inv.home.join("cache/source/flow"),
        &compiler_files,
        &inv.control,
    )?;
    if syntax["passed"] != true {
        let mut result = incomplete(vec![gap(
            "parse_error",
            "bundled Lua 5.1 syntax check failed",
        )]);
        result["syntaxCheck"] = syntax;
        return Ok(result);
    }
    let words = files
        .values()
        .flat_map(|text| {
            text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .filter(|word| !word.is_empty())
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    let mut api = Vec::new();
    if let Some(environment) = &reading.environment {
        for record in &environment.facts {
            inv.control.check()?;
            if record.kind == "function" && record.owner.is_none() && record.name.split('.').all(|part| words.contains(part))
            {
                if api.len() == 32 {
                    input_gaps.push(gap(
                        "host_binding_budget",
                        "selected source references more than 32 API declarations",
                    ));
                    break;
                }
                api.push(record.clone());
            }
        }
    } else {
        input_gaps.push(gap(
            "environment_missing",
            "select a pinned client API environment with --environment",
        ));
    }
    let rules = if let Some(environment) = &reading.environment {
        let identity = &environment.manifest;
        super::from_facts(
            super::RuleIdentity {
                client: identity.identity.client.clone(),
                api_commit: identity.identity.commit.clone(),
                metadata_digest: identity.input_sha256.clone(),
                version: "pinned-client-api-flow-v1".into(),
            },
            environment,
            &inv.control,
        )?
        .calls
        .into_iter()
        .filter(|(name, _)| api.iter().any(|record| &record.name == name))
        .collect()
    } else {
        BTreeMap::new()
    };
    let manifest = runtime.manifest();
    let request = Request {
        files,
        ordered,
        source: source_identity,
        target_path: path.into(),
        target_line: reading.data["symbol"]["line"].as_u64().unwrap_or(1) as usize,
        luac: LuacIdentity {
            version: manifest.version.clone(),
            platform: manifest.platform.clone(),
            executable_sha256: manifest.sha256.clone(),
            executable_bytes: manifest.bytes,
        },
        api,
        rules,
        environment_manifest: reading
            .environment
            .as_ref()
            .map(|environment| environment.manifest.clone()),
        input_gaps,
        load_evidence,
    };
    let started = Instant::now();
    let deadline = (started + Duration::from_millis(WORKER_MILLIS)).min(inv.control.deadline());
    let bytes = serde_json::to_vec(&request)?;
    inv.control.check()?;
    if bytes.len() > WORKER_INPUT_BYTES {
        return Err(ToolkitError::new(
            "flow.budget",
            "serialized source flow input exceeds 64 MiB",
            3,
        ));
    }
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--source-flow-worker");
    let output = crate::process::run_controlled(
        command,
        WORKER_BYTES,
        deadline.saturating_duration_since(Instant::now()),
        Some(bytes),
        Some(inv.control.token()),
    )
    .map_err(|mut error| {
        error.code = error.code.replace("luals.", "flow.");
        error
    })?;
    inv.control.check()?;
    if output.status != 0 {
        return Err(ToolkitError::new(
            "flow.worker_failed",
            String::from_utf8_lossy(&output.stderr).into_owned(),
            5,
        ));
    }
    let mut response: Value = serde_json::from_slice(&output.stdout)?;
    if response["schema"] != "lycheedev.source-flow-worker.v1" {
        return Err(ToolkitError::new(
            "flow.worker_protocol",
            "invalid flow worker result",
            4,
        ));
    }
    let elapsed = started.elapsed().as_millis();
    if elapsed > u128::from(WORKER_MILLIS) {
        return Err(ToolkitError::new(
            "flow.budget",
            "flow worker retirement exceeded total two-second budget",
            3,
        ));
    }
    response["syntaxCheck"] = syntax;
    response["executionMode"] = json!("owned_one_shot");
    response["sessionReuseRequested"] = json!(inv.flag_bool("--session-reuse"));
    response["sessionReused"] = json!(false);
    response["lifecycle"] = json!({"owner":"source_owned_process","elapsedMillisecondsThroughRetirement":elapsed,"budgetMilliseconds":WORKER_MILLIS,"retired":true});
    let mut flow = response
        .as_object_mut()
        .expect("worker object")
        .remove("flow")
        .ok_or_else(|| {
            ToolkitError::new("flow.worker_protocol", "worker omitted flow result", 4)
        })?;
    flow.as_object_mut()
        .ok_or_else(|| {
            ToolkitError::new(
                "flow.worker_protocol",
                "worker flow result is not an object",
                4,
            )
        })?
        .extend(response.as_object().expect("worker object").clone());
    flow["schema"] = json!("lycheedev.source-flow.v1");
    Ok(flow)
}

fn environment_materials(
    request: &Request,
    control: &OperationControl,
) -> Result<(TrustedHostEnvironment, Vec<Value>)> {
    let mut duplicates = BTreeSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for record in &request.api {
        *counts.entry(record.name.clone()).or_default() += 1;
    }
    for (name, count) in counts {
        if count > 1 {
            duplicates.insert(name);
        }
    }
    let mut gaps = duplicates
        .iter()
        .map(|name| gap("ambiguous_api_declaration", name))
        .collect::<Vec<_>>();
    let mut declarations = Vec::new();
    let mut predicates = Vec::new();
    let mut calls = serde_json::Map::new();
    let mut bindings = Vec::new();
    for record in request
        .api
        .iter()
        .filter(|record| !duplicates.contains(&record.name))
    {
        control.check()?;
        let identity = format!("{}:{}:{}", record.path, record.line, record.name);
        if record.name == "issecretvalue" {
            let index = predicates.len();
            predicates.push(json!({"name":record.name,"identity":identity,"argument":1,"returnArity":1,"effect":"no_lua_heap_write","retainsArguments":false,"evidence":record}));
            bindings.push(HostBindingInput::Predicate {
                declaration: HostMaterialRef {
                    material: "api-declarations".into(),
                    pointer: format!("/predicates/{index}"),
                },
            });
            continue;
        }
        let rule = request
            .rules
            .get(&record.name)
            .cloned()
            .unwrap_or_else(|| super::CallRule {
                name: record.name.clone(),
                path: record.path.clone(),
                line: record.line,
                ..Default::default()
            });
        if record.returns.len() > 32 || record.parameters.len() > 32 {
            gaps.push(gap("host_slot_budget", &record.name));
            continue;
        }
        calls.insert(record.name.clone(), serde_json::to_value(rule)?);
        let mut declaration = json!({"name":record.name,"identity":identity,"effect":"unknown","sourceExecution":false,"evidence":record});
        if record.raw.get("MayReturnNothing") != Some(&Value::Bool(true)) {
            declaration["returnArity"] = json!(record.returns.len());
        }
        let index = declarations.len();
        declarations.push(declaration);
        bindings.push(HostBindingInput::Callable {
            declaration: HostMaterialRef {
                material: "api-declarations".into(),
                pointer: format!("/callables/{index}"),
            },
            rules: Some(HostMaterialRef {
                material: "api-rules".into(),
                pointer: format!("/calls/{}", record.name),
            }),
        });
    }
    let rules = serialize_json(
        &json!({"sourceExecution":false,"configExecution":false,"calls":calls,"environment":request.environment_manifest}),
        control,
        65536,
    )?;
    let rules_hash = storage::sha256(&rules);
    let declarations = serialize_json(
        &json!({"sourceExecution":false,"configExecution":false,"callables":declarations,"predicates":predicates,"rules":{"sha256":rules_hash},"environment":request.environment_manifest}),
        control,
        65536,
    )?;
    let materials = vec![
        HostMaterialInput {
            name: "api-declarations".into(),
            expected_sha256: storage::sha256(&declarations),
            bytes: declarations,
        },
        HostMaterialInput {
            name: "api-rules".into(),
            expected_sha256: rules_hash,
            bytes: rules,
        },
    ];
    let host = TrustedHostEnvironment::from_host_material(
        "pinned-client-api-flow-v1".into(),
        false,
        materials,
        vec![],
        control,
    )?
    .with_bindings(bindings, control)?;
    Ok((host, gaps))
}

fn execute(request: Request, control: &OperationControl) -> Result<Value> {
    let (host, mut input_gaps) = environment_materials(&request, control)?;
    input_gaps.extend(request.input_gaps.clone());
    let files = request
        .files
        .iter()
        .map(|(path, text)| SourceFileInput {
            path: path.clone(),
            original_utf8: text.clone(),
            content_sha256: storage::sha256(text.as_bytes()),
            analysis_utf8: text.strip_prefix('\u{feff}').unwrap_or(text).into(),
            map: if text.starts_with('\u{feff}') {
                SourceMap::Utf8BomPrefix
            } else {
                SourceMap::Identity
            },
        })
        .collect();
    let flow_budget = FlowBudget {
        max_files: FILES,
        ..Default::default()
    };
    let limits = SourceLimits {
        flow: Some(flow_budget),
        files: FILES,
        input_bytes: INPUT_BYTES,
        ast_nodes: 2000000,
        construction_steps: 100000,
        total_milliseconds: WORKER_MILLIS,
        ..Default::default()
    };
    let config = crate::engine_config::research();
    let plan = match &request.ordered {
        Some(order) => LoadPlan::Known(order.clone()),
        None => LoadPlan::Unknown(request.files.keys().cloned().collect()),
    };
    let inputs = SourceInputs::admit(
        SourceInputSpec {
            files,
            workspace_root: PathBuf::from("C:/lycheedev/pinned-source-flow"),
            source_identity: request.source,
            phase: "after_load".into(),
            ordered_occurrences: request.ordered.clone().unwrap_or_default(),
            config,
            host,
            luac: request.luac,
            limits,
        },
        control,
    )?;
    let prepared = prepare(inputs, control)?;
    let resolution =
        prepared.entry_candidates(&request.target_path, request.target_line, control)?;
    let entry = match &resolution {
        EntryResolution::Unique { candidate } => Some(EntryRequest {
            candidate: candidate.id.clone(),
            load_occurrence: None,
            creation_context: None,
            arguments: EntryArguments::Unspecified,
        }),
        EntryResolution::Absent => None,
        EntryResolution::SelectorRequired { .. } => {
            input_gaps.push(gap(
                "selector_required",
                "original source line intersects multiple function closures",
            ));
            return Ok(
                json!({"schema":"lycheedev.source-flow-worker.v1","flow":incomplete(input_gaps),"entryResolution":resolution,"loadEvidence":request.load_evidence,"inputIdentity":prepared.inputs().identity()}),
            );
        }
    };
    let graph = prepared.build_plan(
        "source-context-flow",
        "after_load",
        plan,
        entry,
        Some(FLOW_RULES),
        control,
    )?;
    let query = prepared.source_queries(control)?.with_execution(
        &graph,
        QuerySelector::AllOccurrences,
        control,
    )?;
    let flow = query.flow(
        FlowOptions::default(),
        PageRequest {
            limit: 200,
            ..Default::default()
        },
    )?;
    let mut flow = serde_json::to_value(flow)?;
    if !input_gaps.is_empty() {
        flow["complete"] = json!(false);
        flow["input_complete"] = json!(false);
        flow["gaps"]
            .as_array_mut()
            .expect("flow gaps")
            .extend(input_gaps);
    }
    let mut common = serde_json::to_value(&graph)?;
    common.as_object_mut().expect("graph").remove("raw_emmy");
    common
        .as_object_mut()
        .expect("graph")
        .remove("snapshot_inputs_json");
    Ok(
        json!({"schema":"lycheedev.source-flow-worker.v1","flow":flow,"commonFacts":common,"entryResolution":resolution,"inputIdentity":graph.inputs().identity(),"loadEvidence":request.load_evidence}),
    )
}

/// Private same-binary worker; input is owned data from the public pinned-source command.
pub fn worker_stdio() -> Result<()> {
    let started = Instant::now();
    let control = OperationControl::new(
        started + Duration::from_millis(WORKER_MILLIS),
        Arc::new(AtomicBool::new(false)),
    );
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(WORKER_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    control.check()?;
    if bytes.len() > WORKER_INPUT_BYTES {
        return Err(ToolkitError::new(
            "flow.budget",
            "worker input exceeds byte budget",
            3,
        ));
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    drop(bytes);
    let response = execute(request, &control)?;
    let bytes = serialize_json(&response, &control, WORKER_BYTES)?;
    drop(response);
    std::io::stdout().lock().write_all(&bytes)?;
    control.check()?;
    Ok(())
}
