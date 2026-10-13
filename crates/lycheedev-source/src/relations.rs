//! Structural relations remain candidates until an independent semantic binding resolves a site.
use crate::{
    environment, git,
    index::{self, Index},
    luals,
};
use lycheedev_core::{
    Invocation, Result, ToolkitError,
    selection::{self, PinnedSet},
    storage,
};
use serde_json::{Value, json};
pub struct Reading {
    pub pin: PinnedSet,
    pub index: Index,
    pub data: Value,
    pub environment: Option<environment::Environment>,
    pub(crate) load_facts: Option<Vec<index::LoadFact>>,
}
fn invalid(code: &str, message: &str) -> ToolkitError {
    ToolkitError::new(code, message, 2)
}
pub fn structural(inv: &Invocation) -> Result<Reading> {
    with_runtime(inv, None)
}
pub fn with_runtime(inv: &Invocation, runtime: Option<&luals::Runtime>) -> Result<Reading> {
    with_services(inv, runtime, None)
}
pub fn with_services(
    inv: &Invocation,
    runtime: Option<&luals::Runtime>,
    broker: Option<&crate::session::BrokerIdentity>,
) -> Result<Reading> {
    with_engine(inv,runtime,broker,None)
}
pub fn with_native_services(inv:&Invocation,tools:&crate::SourceTools<'_>,broker:Option<&crate::session::BrokerIdentity>)->Result<Reading>{
    with_engine(inv,None,broker,tools.luac.filter(|_|!inv.flag_bool("--static-only")))
}
fn with_engine(inv:&Invocation,runtime:Option<&luals::Runtime>,broker:Option<&crate::session::BrokerIdentity>,native:Option<&crate::luac::Runtime>)->Result<Reading>{
    inv.control.check()?;
    if inv.flag("--snapshot").is_none()
        || inv.flag("--symbol-id").is_some() == inv.flag("--symbol").is_some()
    {
        return Err(ToolkitError::arguments(
            "source refs/context require --snapshot and exactly one of --symbol-id or --symbol",
        ));
    }
    if inv.flag_bool("--static-only")
        && (inv.flag("--release").is_some() || inv.flag_bool("--session-reuse"))
    {
        return Err(ToolkitError::arguments(
            "--static-only cannot be combined with --release or --session-reuse",
        ));
    }
    if inv.flag_bool("--flow") {
        return Err(ToolkitError::arguments("--flow is evaluated by source context"));
    }
    if native.is_none() && inv.flag_bool("--session-reuse") && (broker.is_none() || runtime.is_none()) {
        return Err(ToolkitError::new(
            "luals.session_unavailable",
            "verified persistent broker provider is not yet connected",
            3,
        ));
    }
    if native.is_none() && inv.flag("--release").is_some() && runtime.is_none() {
        return Err(ToolkitError::new(
            "luals.research_provider_unavailable",
            "explicit verified semantic runtime must be connected before research can use --release",
            3,
        ));
    }
    let runtime = runtime.filter(|_| !inv.flag_bool("--static-only"));
    let limit = inv.usize_flag("--limit", 50, 1, 200)?;
    let direction = inv.flag("--direction").unwrap_or("both");
    if !["incoming", "outgoing", "both"].contains(&direction) {
        return Err(ToolkitError::arguments(
            "--direction must be incoming, outgoing, or both",
        ));
    }
    let pin = selection::resolve_pinned(inv)?;
    let source = pin.source.as_ref().ok_or_else(|| {
        ToolkitError::new("selection.source_missing", "selected pin has no source", 3)
    })?;
    let (index, index_digest) = crate::research_material::measured("source.index", ||
        index::build_material_with_control(&inv.home, source, &inv.control))?;
    let declarations: Vec<_> = index
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == "declaration"
                && (inv.flag("--symbol-id").is_some_and(|id| symbol.id == id)
                    || inv.flag("--symbol").is_some_and(|name| symbol.name == name))
        })
        .collect();
    if inv.flag("--symbol-id").is_some() && declarations.is_empty() {
        return Err(invalid(
            "codebase.invalid_symbol",
            "selected symbol ID is absent from the fixed index",
        ));
    }
    let mut reasons = vec![];
    let mut data = json!({"repository":source.repository,"product":source.product,"commit":source.exact_commit,"snapshot":pin.id,"relations":[],"truncated":false,"complete":false});
    let selected_environment = inv
        .flag("--environment")
        .map(|id| selection::load_pinned(&inv.home, id))
        .transpose()?;
    if selected_environment
        .as_ref()
        .is_some_and(|pin| pin.source.is_none())
    {
        return Err(ToolkitError::new(
            "selection.source_missing",
            "selected environment pin has no source",
            3,
        ));
    }
    let environment_source = selected_environment
        .as_ref()
        .and_then(|pin| pin.source.as_ref())
        .or(if source.repository == "wow-ui-source" {
            Some(source)
        } else {
            None
        });
    let material = if declarations.len() == 1 {
        native.map(|runtime| crate::research_material::load_or_build(inv, source, environment_source, &index, &index_digest, runtime)).transpose()?
    } else { None };
    let (client_environment, mut prepared_snapshot, mut load_facts) = if let Some(material) = material {
        (material.environment, Some(material.snapshot), Some(material.load_facts))
    } else {
        (crate::research_material::measured("source.environment", || environment_source
            .map(|source| environment::from_pin(&inv.home, source, &inv.control)).transpose())?, None, None)
    };
    let environment_state = if let Some(env) = &client_environment {
        data["environmentManifest"] = serde_json::to_value(&env.manifest)?;
        if env.coverage.complete {
            "ready"
        } else {
            reasons.push("client API environment has extraction gaps");
            "partial"
        }
    } else {
        "missing-client-pin"
    };
    let structural = if index.summary["complete"] == true {
        "complete"
    } else {
        reasons.push("some source files have parser diagnostics");
        "partial"
    };
    let scanned = declarations.len();
    if declarations.len() > 1 && inv.flag("--symbol-id").is_none() {
        data["candidates"] =
            serde_json::to_value(declarations.iter().take(limit).collect::<Vec<_>>())?;
        data["truncated"] = json!(declarations.len() > limit);
        reasons.push("name has multiple declaration identities; choose a symbolId");
    } else {
        let name = if let Some(symbol) = declarations.first() {
            data["symbol"] = serde_json::to_value(symbol)?;
            symbol.name.as_str()
        } else {
            inv.required("--symbol")?
        };
        let runtime_key = if native.is_some(){crate::native_research::RESEARCH_RULES.into()}else{runtime
            .map(|runtime| {
                format!(
                    "{}:{}",
                    runtime.identity()["version"].as_str().unwrap_or(""),
                    runtime.identity()["sha256"].as_str().unwrap_or("")
                )
            })
            .unwrap_or_else(|| "static".into())};
        let environment_key = client_environment
            .as_ref()
            .map(|environment| {
                format!(
                    "{}:{}:{}:{}",
                    environment.manifest.input_sha256, environment.manifest.definitions_sha256,
                    environment.manifest.facts_sha256, environment.manifest.identity.generator_version
                )
            })
            .unwrap_or_else(|| environment_state.into());
        let identity = format!(
            "{}\0{}\0{}\0{}\0{}\0{direction}\0{limit}\0{runtime_key}\0{environment_key}\0false",
            source.repository,
            source.product,
            source.exact_commit,
            inv.flag("--symbol-id").unwrap_or(""),
            inv.flag("--symbol").unwrap_or("")
        );
        let key = &storage::sha256(identity.as_bytes())[..16];
        let offset = if let Some(cursor) = inv.flag("--cursor") {
            let Some((found, n)) = cursor.split_once(':') else {
                return Err(invalid(
                    "codebase.invalid_search_cursor",
                    "relation cursor shape differs",
                ));
            };
            if found != key {
                return Err(invalid(
                    "codebase.invalid_search_cursor",
                    "relation cursor belongs to a different query",
                ));
            }
            n.parse::<usize>()
                .ok()
                .filter(|n| *n <= 10000)
                .ok_or_else(|| {
                    invalid(
                        "codebase.invalid_search_cursor",
                        "relation cursor offset exceeds bounds",
                    )
                })?
        } else {
            0
        };
        let mut relations = Vec::new();
        let mut overflow = false;
        for edge in index
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == "relationship")
        {
            inv.control.check()?;
            if !(direction != "outgoing" && edge.target == name
                || direction != "incoming" && edge.name == name)
            {
                continue;
            }
            if relations.len() == 10000 {
                overflow = true;
                continue;
            }
            let (state, reason) = if edge.confidence == "dynamic-unresolved" {
                ("unknown", "dynamic source expression")
            } else if ["contains", "event-name", "inherits", "event-registration"]
                .contains(&edge.category.as_str())
            {
                (
                    "structural",
                    "literal or parser-derived WoW relation; runtime dispatch is not proven",
                )
            } else {
                (
                    "static_candidate",
                    "name and structural source relation only; binding is not proven",
                )
            };
            relations.push(json!({"source":edge.name,"target":edge.target,"kind":edge.category,"state":state,"path":edge.path,"line":edge.line,"basis":["static-extraction"],"reason":reason}));
        }
        if let (Some(native),Some(symbol))=(native,declarations.first()) {
            let snapshot=if let Some(snapshot)=prepared_snapshot.take(){snapshot}else{
                let (snapshot, facts)=crate::research_material::measured("source.snapshot", || crate::native_research::snapshot_material(inv,source,&index,client_environment.as_ref(),native))?;
                load_facts=Some(facts);snapshot
            };
            let request=crate::native_research::WorkerRequest{snapshot,query:crate::native_research::QueryRequest{symbol:(*symbol).clone(),direction:direction.into()}};
            let mut response=crate::research_material::measured("source.native_analyze", || if inv.flag_bool("--session-reuse"){
                crate::native_session::analyze(inv,request)
            }else{crate::native_research::one_shot(inv,request)})?;
            data["semanticRuntime"]=json!({"engine":"native-common-source","rulesVersion":crate::native_research::RESEARCH_RULES});
            data["semanticAnalysis"]=response["analysis"].clone();
            if let Some(rows)=response.as_object_mut().and_then(|value|value.remove("relations")).and_then(|value|value.as_array().cloned()){relations.extend(rows);}
            data["nativeResearch"]=response;
        } else if let (Some(runtime), Some(symbol)) = (runtime, declarations.first()) {
            let cache = SemanticCache::admit(
                inv,
                source,
                &index,
                declarations[0],
                direction,
                client_environment.as_ref(),
                SemanticProvider {
                    runtime,
                    broker,
                    environment_pin: environment_source,
                },
            )?;
            let cached = cache
                .as_ref()
                .and_then(|cache| cache.read(inv).ok().flatten());
            inv.control.check()?;
            let (semantic, analysis) = if let Some(rows) = cached {
                (
                    rows,
                    json!({"state":"complete","cacheHit":true,"queryBatches":0,"results":[],"coverage":{"state":"complete","basis":"authenticated complete semantic cache"}}),
                )
            } else {
                let mut computed = semantic_relations(
                    inv,
                    source,
                    &index,
                    symbol,
                    direction,
                    client_environment.as_ref(),
                    SemanticProvider {
                        runtime,
                        environment_pin: environment_source,
                        broker: broker.filter(|_| inv.flag_bool("--session-reuse")),
                    },
                )?;
                if computed.1["state"] == "complete"
                    && let Some(cache) = &cache
                    && let Err(error) = cache.write(inv, &computed.0)
                {
                    inv.control.check()?;
                    computed.1["cacheWriteState"] =
                        json!({"state":"not_stored","reason":error.code});
                }
                computed
            };
            data["semanticRuntime"] = runtime.identity();
            data["semanticAnalysis"] = json!({"state":analysis["state"],"cacheHit":analysis["cacheHit"]==true,"queryCount":analysis["results"].as_array().map_or(0,Vec::len),"queryBatches":analysis["queryBatches"],"workspaceCoverage":analysis["coverage"],"positionCoverage":analysis["positionCoverage"]});
            relations.extend(semantic);
        }
        relations.sort_by(|a, b| {
            a["path"]
                .as_str()
                .cmp(&b["path"].as_str())
                .then(a["line"].as_u64().cmp(&b["line"].as_u64()))
                .then(a["kind"].as_str().cmp(&b["kind"].as_str()))
                .then(a["source"].as_str().cmp(&b["source"].as_str()))
                .then(a["target"].as_str().cmp(&b["target"].as_str()))
        });
        if offset > relations.len() {
            return Err(invalid(
                "codebase.invalid_search_cursor",
                "relation cursor exceeds available results",
            ));
        }
        let end = relations.len().min(offset + limit);
        data["truncated"] = json!(end < relations.len() || overflow);
        if end < relations.len() {
            data["nextCursor"] = json!(format!("{key}:{end}"));
        }
        data["relations"] = json!(&relations[offset..end]);
        if overflow {
            reasons.push("matching structural relations exceeded 10000-entry budget");
        }
        if declarations.is_empty() && relations.is_empty() {
            reasons.push("no declaration or structural relation in indexed coverage");
        }
    }
    let semantic_state = if (runtime.is_some()||native.is_some()) && data.get("semanticAnalysis").is_some() {
        data["semanticAnalysis"]["state"]
            .as_str()
            .unwrap_or("partial")
    } else {
        "unavailable"
    }
    .to_owned();
    if semantic_state == "unavailable" {
        reasons.push("semantic source provider unavailable or static-only requested");
    }
    if environment_state == "missing-client-pin" {
        reasons.push("third-party source has no selected client API environment");
    }
    let complete = data["truncated"] != true
        && structural == "complete"
        && semantic_state == "complete"
        && environment_state == "ready";
    data["complete"] = json!(complete);
    data["coverage"] = json!({"state":if complete{"complete"}else{"partial"},"structural":structural,"semantic":semantic_state,"environment":environment_state,"scannedFacts":scanned,"reasons":reasons});
    Ok(Reading {
        pin,
        index,
        data,
        environment: client_environment,
        load_facts,
    })
}
fn position(
    inv: &Invocation,
    pin: &selection::SourcePin,
    path: &str,
    line: usize,
    name: &str,
    documents: &mut std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<Option<Value>> {
    if !documents.contains_key(path) {
        let (raw, _) =
            git::blob_with_control(&inv.home, pin, path, git::MAX_SOURCE_BYTES, &inv.control)?;
        documents.insert(path.into(), raw);
    }
    let text = std::str::from_utf8(&documents[path])
        .map_err(|_| ToolkitError::invalid("semantic source is not UTF-8"))?;
    let Some(row) = text.lines().nth(line.saturating_sub(1)) else {
        return Ok(None);
    };
    let leaf = name.rsplit(['.', ':']).next().unwrap_or(name);
    let matches: Vec<_> = row.match_indices(leaf).collect();
    if matches.len() != 1 {
        return Ok(None);
    };
    let column = row[..matches[0].0].encode_utf16().count();
    Ok(Some(
        json!({"line":line.saturating_sub(1),"character":column}),
    ))
}
struct SemanticProvider<'a> {
    runtime: &'a luals::Runtime,
    environment_pin: Option<&'a selection::SourcePin>,
    broker: Option<&'a crate::session::BrokerIdentity>,
}
struct SemanticCache {
    key: String,
    _lease: storage::ExclusiveLock,
}
impl SemanticCache {
    fn admit(
        inv: &Invocation,
        pin: &selection::SourcePin,
        index: &Index,
        symbol: &index::Symbol,
        direction: &str,
        environment: Option<&environment::Environment>,
        services: SemanticProvider<'_>,
    ) -> Result<Option<Self>> {
        inv.control.check()?;
        let runtime = services.runtime;
        let broker = services.broker;
        let Some(environment) =
            environment.filter(|env| env.coverage.complete && index.summary["complete"] == true)
        else {
            return Ok(None);
        };
        let store = lycheedev_core::metadata::Metadata::open(&inv.home, false)?;
        let manifest: Value =
            store.read(&format!("source/rust-index/{}", index::fingerprint(pin)))?;
        let key = storage::sha256(&serde_json::to_vec(
            &json!({"schema":"lycheedev.source-semantic.rust.v1","source":pin,"indexFingerprint":index::fingerprint(pin),"recordsHash":manifest["blob"]["sha256"],"environment":environment.manifest,"runtime":runtime.identity(),"executable":runtime.executable_identity()["sha256"],"release":broker.map(|id|(&id.release_manifest_sha256,&id.exe_sha256)),"configuration":luals::configuration(std::path::Path::new("@environment")),"symbolId":symbol.id,"direction":direction}),
        )?);
        let lease = match storage::ExclusiveLock::resource(
            &inv.home.join("locks"),
            &format!("source:v1:semantic:{key}"),
        ) {
            Ok(lease) => lease,
            Err(error) if error.exit_code == 6 => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(Self { key, _lease: lease }))
    }
    fn metadata_key(&self) -> String {
        format!("source/rust-semantics/v1/{}", self.key)
    }
    fn read(&self, inv: &Invocation) -> Result<Option<Vec<Value>>> {
        inv.control.check()?;
        let store = lycheedev_core::metadata::Metadata::open(&inv.home, false)?;
        let manifest: Value = match store.read(&self.metadata_key()) {
            Ok(value) => value,
            Err(error) if error.code == "vault.record_missing" => return Ok(None),
            Err(error) => return Err(error),
        };
        if manifest["schema"] != "lycheedev.source-semantic.rust.v1" || manifest["key"] != self.key
        {
            return Ok(None);
        }
        let raw = lycheedev_core::cache::read_object(
            &inv.home,
            &manifest["blob"],
            4 << 20,
            &inv.control,
        )?;
        inv.control.check()?;
        let value: Value = serde_json::from_slice(&raw)?;
        let Some(rows) = value["rows"].as_array().filter(|rows| rows.len() <= 10000) else {
            return Ok(None);
        };
        if value["schema"] != "lycheedev.source-semantic.rust.v1"
            || value["key"] != self.key
            || value["rowsHash"] != storage::sha256(&serde_json::to_vec(rows)?)
        {
            return Ok(None);
        }
        Ok(Some(rows.clone()))
    }
    fn write(&self, inv: &Invocation, rows: &[Value]) -> Result<()> {
        inv.control.check()?;
        let rows_raw = storage::bounded_json(&rows, 4 << 20)?;
        let value = json!({"schema":"lycheedev.source-semantic.rust.v1","key":self.key,"rowsHash":storage::sha256(&rows_raw),"rows":rows});
        let raw = storage::bounded_json(&value, 4 << 20)?;
        if raw.len() > 4 << 20 {
            return Ok(());
        }
        let blob = lycheedev_core::cache::publish(&inv.home, &raw, 4 << 20, None)?;
        inv.control.check()?;
        let mut store = lycheedev_core::metadata::Metadata::open(&inv.home, true)?;
        let key = self.metadata_key();
        let generation = store.generation(&key)?;
        store.put(
            &key,
            &json!({"schema":"lycheedev.source-semantic.rust.v1","key":self.key,"blob":blob}),
            generation,
        )?;
        Ok(())
    }
}
fn semantic_relations(
    inv: &Invocation,
    pin: &selection::SourcePin,
    index: &Index,
    symbol: &index::Symbol,
    direction: &str,
    environment: Option<&environment::Environment>,
    services: SemanticProvider<'_>,
) -> Result<(Vec<Value>, Value)> {
    let definitions = environment
        .map(|env| env.definitions.as_bytes())
        .unwrap_or(b"");
    let mut queries = Vec::new();
    let mut sites = Vec::new();
    let mut uncertain = false;
    let mut documents = std::collections::BTreeMap::new();
    if direction != "outgoing" {
        let reference_position = if symbol.category.starts_with("api-") {
            let mappings = environment
                .map(|env| {
                    env.mappings
                        .iter()
                        .filter(|m| {
                            m.kind == "function"
                                && m.source_path == symbol.path
                                && m.callable == symbol.name
                                && m.source_line >= symbol.line
                                && m.source_line <= symbol.end_line.max(symbol.line)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if mappings.len() == 1 {
                Some((
                    "@environment/api.d.lua".to_owned(),
                    json!({"line":mappings[0].generated_line,"character":mappings[0].generated_utf16_column}),
                ))
            } else {
                None
            }
        } else if symbol.path.to_ascii_lowercase().ends_with(".lua") {
            position(
                inv,
                pin,
                &symbol.path,
                symbol.line,
                &symbol.name,
                &mut documents,
            )?
            .map(|pos| (symbol.path.clone(), pos))
        } else {
            None
        };
        if let Some((path, position)) = reference_position {
            queries.push(json!({"kind":"references","path":path,"position":position}));
            sites.push(None);
            queries.push(json!({"kind":"hover","path":path,"position":position}));
            sites.push(None);
        } else {
            uncertain = true;
        }
    }
    if direction != "incoming" {
        let mut outgoing_count = 0usize;
        for edge in index.symbols.iter().filter(|edge| {
            edge.kind == "relationship"
                && edge.name == symbol.name
                && edge.category == "call"
                && edge.path == symbol.path
                && edge.line >= symbol.line
                && edge.line <= symbol.end_line.max(symbol.line)
        }) {
            inv.control.check()?;
            if outgoing_count >= 128 {
                uncertain = true;
                break;
            }
            if let Some(position) = position(
                inv,
                pin,
                &edge.path,
                edge.line,
                &edge.target,
                &mut documents,
            )? {
                queries.push(json!({"kind":"definition","path":edge.path,"position":position}));
                sites.push(Some(edge));
                outgoing_count += 1;
            } else {
                uncertain = true;
            }
        }
    }
    if queries.is_empty() {
        return Ok((
            vec![],
            json!({"state":"partial","reason":"no unique semantic query position","results":[]}),
        ));
    }
    let mut analysis = if let Some(identity) = services.broker {
        crate::session::analyze(
            inv,
            identity,
            pin.clone(),
            services.environment_pin.cloned(),
            queries.chunks(128).map(<[Value]>::to_vec).collect(),
        )?
    } else {
        let mut session = services.runtime.open_source_session_with_control(
            &inv.home,
            pin,
            index,
            definitions,
            &inv.control,
        )?;
        let analysis = (|| -> Result<Value> {
            let mut combined = json!({"state":"complete","results":[],"queryBatches":0});
            for batch in queries.chunks(128) {
                let mut part = session.analyze_with_control(batch, &inv.control)?;
                if part["state"] != "complete" {
                    combined["state"] = json!("partial");
                }
                combined["coverage"] = part["coverage"].clone();
                combined["queryBatches"] = json!(combined["queryBatches"].as_u64().unwrap() + 1);
                combined["results"]
                    .as_array_mut()
                    .unwrap()
                    .append(part["results"].as_array_mut().unwrap());
            }
            Ok(combined)
        })();
        session.close();
        analysis?
    };
    if uncertain {
        analysis["state"] = json!("partial");
        analysis["positionCoverage"] = json!("some structural sites were not uniquely positioned");
    }
    analysis["queryBatches"] = json!(queries.len().div_ceil(128));
    let mut result = vec![];
    let mut partial_results = false;
    for (query_result, site) in analysis["results"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .zip(sites)
    {
        if query_result["state"] != "complete" {
            partial_results = true;
            continue;
        }
        if query_result["query"]["kind"] == "hover" {
            continue;
        }
        if site.is_some() && query_result["locations"].as_array().map_or(0, Vec::len) != 1 {
            partial_results = true;
            continue;
        }
        for location in query_result["locations"].as_array().unwrap_or(&Vec::new()) {
            if result.len() >= 10000 {
                partial_results = true;
                break;
            }
            let Some(path) = location["path"].as_str() else {
                continue;
            };
            let line = location["range"]["start"]["line"].as_u64().unwrap_or(0) as usize;
            let (target_path, target_line) = if path == "@environment/api.d.lua" {
                let Some(mapping) = environment.and_then(|env| {
                    env.mappings.iter().find(|mapping| {
                        mapping.generated_line == line
                            && mapping.generated_utf16_column
                                == location["range"]["start"]["character"]
                                    .as_u64()
                                    .unwrap_or(u64::MAX) as usize
                    })
                }) else {
                    partial_results = true;
                    continue;
                };
                (mapping.source_path.clone(), mapping.source_line)
            } else {
                (path.into(), line + 1)
            };
            if let Some(site) = site {
                let identity = if path == "@environment/api.d.lua" {
                    environment
                        .map(|env| {
                            (
                                &env.manifest.identity.repository,
                                &env.manifest.identity.commit,
                            )
                        })
                        .unwrap()
                } else {
                    (&pin.repository, &pin.exact_commit)
                };
                let mut row = json!({"source":site.name,"target":site.target,"kind":"call","state":"resolved","path":site.path,"line":site.line,"basis":["LuaLS public textDocument/definition",format!("fixed worktree {}",pin.exact_commit)],"targetRepository":identity.0,"targetCommit":identity.1,"targetPath":target_path,"targetLine":target_line});
                let declarations = index
                    .symbols
                    .iter()
                    .filter(|s| {
                        s.kind == "declaration" && s.path == target_path && s.line == target_line
                    })
                    .collect::<Vec<_>>();
                if path == "@environment/api.d.lua" {
                    let mapping = environment
                        .unwrap()
                        .mappings
                        .iter()
                        .find(|m| {
                            m.generated_line == line
                                && m.generated_utf16_column
                                    == location["range"]["start"]["character"]
                                        .as_u64()
                                        .unwrap_or(u64::MAX)
                                        as usize
                        })
                        .unwrap();
                    let identity = format!(
                        "{}\0{}\0{}\0{}\0{}",
                        identity.0,
                        identity.1,
                        mapping.callable,
                        mapping.source_path,
                        mapping.source_line
                    );
                    row["targetId"] = json!(format!(
                        "API-{}",
                        &storage::sha256(identity.as_bytes())[..32]
                    ));
                    row["target"] = json!(mapping.callable);
                    row["basis"] = json!([
                        "LuaLS public textDocument/definition",
                        "generated declaration mapped to pinned API source"
                    ]);
                } else if declarations.len() == 1 {
                    row["targetId"] = json!(declarations[0].id);
                    row["target"] = json!(declarations[0].name);
                } else {
                    partial_results = true;
                    continue;
                }
                row["sourceId"] = json!(symbol.id);
                result.push(row);
            } else {
                if path == "@environment/api.d.lua" {
                    continue;
                }
                if target_path == symbol.path && target_line == symbol.line {
                    continue;
                }
                let row = json!({"source":target_path,"target":symbol.name,"kind":"reference","state":"resolved","path":target_path,"line":target_line,"endLine":location["range"]["end"]["line"].as_u64().unwrap_or(line as u64)+1,"basis":["LuaLS public textDocument/references",format!("fixed worktree {}",pin.exact_commit)],"targetId":symbol.id});
                result.push(row);
            }
        }
    }
    if partial_results {
        analysis["state"] = json!("partial");
    }
    Ok((result, analysis))
}
#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn semantic_cache_roundtrip_detects_corruption_and_bounds_serialization() {
        let home = std::env::temp_dir().join(format!(
            "lycheedev-semantic-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let inv = Invocation {
            command: "source refs".into(),
            positional: None,
            flags: Default::default(),
            home: home.clone(),
            cwd: home.clone(),
            control: Default::default(),
        };
        let cache = SemanticCache {
            key: "1".repeat(64),
            _lease: storage::ExclusiveLock::resource(
                &home.join("locks"),
                "source:v1:semantic:test",
            )
            .unwrap(),
        };
        let rows = vec![
            json!({"source":"First","target":"FixtureAPI","kind":"call","state":"resolved","path":"probe.lua","line":2}),
        ];
        cache.write(&inv, &rows).unwrap();
        assert_eq!(cache.read(&inv).unwrap(), Some(rows.clone()));
        let store = lycheedev_core::metadata::Metadata::open(&home, false).unwrap();
        let value: Value = store.read(&cache.metadata_key()).unwrap();
        let digest = value["blob"]["sha256"].as_str().unwrap();
        let path = home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]);
        let raw = std::fs::read(&path).unwrap();
        let mut wrong = raw.clone();
        wrong[0] = b'!';
        std::fs::write(&path, wrong).unwrap();
        assert_eq!(cache.read(&inv).unwrap_err().exit_code, 4);
        // Missing ordinary derived objects may be recomputed; forensic corrupt
        // bytes remain until explicit repair/prune rather than silent adoption.
        std::fs::remove_file(&path).unwrap();
        assert!(cache.read(&inv).is_err());
        cache.write(&inv, &rows).unwrap();
        assert_eq!(cache.read(&inv).unwrap(), Some(rows));
        assert!(storage::bounded_json(&json!({"large":"x".repeat((4<<20)+1)}), 4 << 20).is_err());
        drop(store);
        drop(cache);
        std::fs::remove_dir_all(&home).unwrap();
    }
}
