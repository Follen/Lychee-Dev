//! Structural relations remain candidates until an independent semantic binding resolves a site.
use crate::{
    environment,
    index::{self, Index},
    request::{RelationRequest, ResearchMode, ResearchSelection, SourceContext},
};
use lycheedev_core::{
    Result, ToolkitError,
    selection::{self, PinnedSet},
    storage,
};
use serde_json::{Value, json};
use std::sync::Arc;
pub struct Reading {
    pub pin: PinnedSet,
    pub index: Arc<Index>,
    pub data: Value,
    pub environment: Option<Arc<environment::Environment>>,
    pub(crate) load_facts: Option<Arc<Vec<index::LoadFact>>>,
}
/// One authenticated preparation shared by every item in a public batch.
/// Immutable handles avoid rebuilding or copying the full index per item.
pub(crate) struct PreparedResearch {
    pub selection: ResearchSelection,
    pub index: Arc<Index>,
    pub environment: Option<Arc<environment::Environment>>,
    pub load_facts: Arc<Vec<index::LoadFact>>,
    pub native_unavailable: Option<String>,
}
pub(crate) fn with_prepared(
    ctx: &SourceContext,
    request: &RelationRequest,
    mode: ResearchMode,
    tools: &crate::SourceTools<'_>,
    prepared: &PreparedResearch,
    native_response: Option<Value>,
) -> Result<Reading> {
    with_engine(
        ctx,
        &prepared.selection,
        request,
        mode,
        tools.luac.filter(|_| mode.uses_native()),
        Some(prepared),
        native_response,
    )
}
fn invalid(code: &str, message: &str) -> ToolkitError {
    ToolkitError::new(code, message, 2)
}
pub fn with_tools(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: &RelationRequest,
    mode: ResearchMode,
    tools: &crate::SourceTools<'_>,
) -> Result<Reading> {
    with_engine(
        ctx,
        selection,
        request,
        mode,
        tools.luac.filter(|_| mode.uses_native()),
        None,
        None,
    )
}
fn with_engine(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: &RelationRequest,
    mode: ResearchMode,
    native: Option<&crate::luac::Runtime>,
    prepared: Option<&PreparedResearch>,
    native_response: Option<Value>,
) -> Result<Reading> {
    ctx.control.check()?;
    if native.is_none()
        && mode.uses_native()
        && prepared.is_none_or(|shared| shared.native_unavailable.is_none())
    {
        return Err(ToolkitError::new(
            "source.native_unavailable",
            "Source research requires the verified bundled Lua compiler and native semantic provider",
            3,
        ));
    }
    let limit = request.limit;
    let direction = request.direction.as_str();
    let pin = selection.pin.clone();
    let source = pin.source.as_ref().ok_or_else(|| {
        ToolkitError::new("selection.source_missing", "selected pin has no source", 3)
    })?;
    let (index, index_digest) = if let Some(shared) = prepared {
        (Arc::clone(&shared.index), String::new())
    } else {
        let (index, digest) = crate::research_material::measured("source.index", || {
            index::build_material_with_control(&ctx.home, source, &ctx.control)
        })?;
        (Arc::new(index), digest)
    };
    let declarations: Vec<_> = index
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == "declaration" && request.selector.matches(symbol))
        .collect();
    if request.selector.id().is_some() && declarations.is_empty() {
        return Err(invalid(
            "codebase.invalid_symbol",
            "selected symbol ID is absent from the fixed index",
        ));
    }
    let mut reasons = vec![];
    let mut data = json!({"repository":source.repository,"product":source.product,"commit":source.exact_commit,"snapshot":pin.id,"relations":[],"truncated":false,"complete":false});
    let environment_source = selection.environment_source()?;
    let material = if prepared.is_none() && declarations.len() == 1 {
        native
            .map(|runtime| {
                crate::research_material::load_or_build(
                    ctx,
                    source,
                    environment_source,
                    &index,
                    &index_digest,
                    runtime,
                )
            })
            .transpose()?
    } else {
        None
    };
    let (client_environment, mut prepared_snapshot, mut load_facts) = if let Some(shared) = prepared
    {
        (
            shared.environment.clone(),
            None,
            Some(Arc::clone(&shared.load_facts)),
        )
    } else if let Some(material) = material {
        (
            material.environment.map(Arc::new),
            Some(material.snapshot),
            Some(Arc::new(material.load_facts)),
        )
    } else {
        (
            crate::research_material::measured("source.environment", || {
                environment_source
                    .map(|source| environment::from_pin(&ctx.home, source, &ctx.control))
                    .transpose()
            })?
            .map(Arc::new),
            None,
            None,
        )
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
    let structural = if index.summary.complete {
        "complete"
    } else {
        reasons.push("some source files have parser diagnostics");
        "partial"
    };
    let scanned = declarations.len();
    if declarations.len() > 1 && request.selector.id().is_none() {
        data["candidates"] =
            serde_json::to_value(declarations.iter().take(limit).collect::<Vec<_>>())?;
        data["truncated"] = json!(declarations.len() > limit);
        reasons.push("name has multiple declaration identities; choose a symbolId");
    } else {
        let name = if let Some(symbol) = declarations.first() {
            data["symbol"] = serde_json::to_value(symbol)?;
            symbol.name.as_str()
        } else {
            request.selector.value()
        };
        let runtime_key = if native.is_some() {
            crate::native_research::RESEARCH_RULES
        } else {
            "static"
        };
        let (offset, key) = relation_cursor(
            request,
            source,
            limit,
            direction,
            &runtime_key,
            client_environment.as_deref(),
        )?;
        let mut relations = Vec::new();
        let mut overflow = false;
        for edge in index
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == "relationship")
        {
            ctx.control.check()?;
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
        if let (Some(native), Some(symbol)) = (native, declarations.first()) {
            let mut response = if let Some(response) = native_response {
                response
            } else if prepared.is_some() {
                return Err(ToolkitError::new(
                    "source.native_worker_protocol",
                    "Batch native result is missing for the selected declaration",
                    4,
                ));
            } else {
                let snapshot = if let Some(snapshot) = prepared_snapshot.take() {
                    snapshot
                } else {
                    let (snapshot, facts) =
                        crate::research_material::measured("source.snapshot", || {
                            crate::native_research::snapshot_material(
                                ctx,
                                source,
                                &index,
                                client_environment.as_deref(),
                                native,
                            )
                        })?;
                    load_facts = Some(Arc::new(facts));
                    snapshot
                };
                let request = crate::native_research::WorkerRequest {
                    snapshot,
                    query: crate::native_research::QueryRequest {
                        symbol: (*symbol).clone(),
                        direction: direction.into(),
                    },
                };
                crate::research_material::measured("source.native_analyze", || {
                    if mode.reuses_session() {
                        crate::native_session::analyze(ctx, request)
                    } else {
                        crate::native_research::one_shot(ctx, request)
                    }
                })?
            };
            data["semanticRuntime"] = json!({"engine":"native-common-source","rulesVersion":crate::native_research::RESEARCH_RULES});
            data["semanticAnalysis"] = response["analysis"].clone();
            if let Some(rows) = take_native_relation_rows(&mut response) {
                relations.extend(rows);
            }
            data["nativeResearch"] = response;
        }
        crate::research_material::measured("source.relation_page", || {
            project_relation_page(&mut data, &mut relations, offset, &key, limit, overflow)
        })?;
        if overflow {
            reasons.push("matching structural relations exceeded 10000-entry budget");
        }
        if declarations.is_empty() && relations.is_empty() {
            reasons.push("no declaration or structural relation in indexed coverage");
        }
    }
    let semantic_state = if native.is_some() && data.get("semanticAnalysis").is_some() {
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
// The worker reply is already owned. Move its row array into pagination rather
// than cloning it immediately before discarding the removed original array.
fn take_native_relation_rows(response: &mut Value) -> Option<Vec<Value>> {
    match response.as_object_mut()?.remove("relations")? {
        Value::Array(rows) => Some(rows),
        _ => None,
    }
}
// Keep proof rows independent, sorted before the unchanged public page cut.
// Factored only so the real pagination path is exercised by the caller tests.
fn project_relation_page(
    data: &mut Value,
    relations: &mut Vec<Value>,
    offset: usize,
    key: &str,
    limit: usize,
    overflow: bool,
) -> Result<()> {
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
    Ok(())
}
fn relation_cursor(
    request: &RelationRequest,
    source: &selection::SourcePin,
    limit: usize,
    direction: &str,
    runtime_key: &str,
    environment: Option<&environment::Environment>,
) -> Result<(usize, String)> {
    let environment_key = environment
        .map(|environment| {
            format!(
                "{}:{}:{}:{}",
                environment.manifest.input_sha256,
                environment.manifest.definitions_sha256,
                environment.manifest.facts_sha256,
                environment.manifest.identity.generator_version
            )
        })
        .unwrap_or_else(|| "missing-client-pin".into());
    let identity = format!(
        "{}\0{}\0{}\0{}\0{}\0{direction}\0{limit}\0{runtime_key}\0{environment_key}\0false",
        source.repository,
        source.product,
        source.exact_commit,
        request.selector.id().unwrap_or(""),
        request.selector.name().unwrap_or("")
    );
    let key = storage::sha256(identity.as_bytes())[..16].to_owned();
    let offset = if !request.cursor.is_empty() {
        let cursor = &request.cursor;
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
    Ok((offset, key))
}
pub(crate) fn validate_prepared_cursor(
    request: &RelationRequest,
    mode: ResearchMode,
    tools: &crate::SourceTools<'_>,
    prepared: &PreparedResearch,
) -> Result<()> {
    let runtime_key = if tools.luac.is_some() && mode.uses_native() {
        crate::native_research::RESEARCH_RULES
    } else {
        "static"
    };
    relation_cursor(
        request,
        prepared.selection.source()?,
        request.limit,
        request.direction.as_str(),
        runtime_key,
        prepared.environment.as_deref(),
    )
    .map(|_| ())
}
#[cfg(test)]
mod incoming_caller_cursor_tests {
    use super::*;
    fn fixture() -> (RelationRequest, selection::SourcePin) {
        (
            RelationRequest {
                selector: crate::request::SymbolSelector::Name("GetActionButtonForID".into()),
                direction: crate::request::RelationDirection::Incoming,
                limit: 2,
                cursor: String::new(),
            },
            selection::SourcePin {
                repository: "wow-ui-source".into(),
                product: "retail".into(),
                requested_ref: "fixed".into(),
                exact_commit: "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b".into(),
                parser_revision: "source-v1".into(),
            },
        )
    }
    #[test]
    fn incoming_caller_v7_cursor_rejected_v8_pages_keep_every_proof_row() {
        let (mut request, source) = fixture();
        let rules = crate::native_research::RESEARCH_RULES;
        let (_, old) = relation_cursor(
            &request,
            &source,
            2,
            "incoming",
            "source-native-research-v7-xml-script-ancestry",
            None,
        )
        .unwrap();
        let (_, key) = relation_cursor(&request, &source, 2, "incoming", rules, None).unwrap();
        assert_ne!(old, key);
        request.cursor = format!("{old}:2");
        assert_eq!(
            relation_cursor(&request, &source, 2, "incoming", rules, None)
                .unwrap_err()
                .code,
            "codebase.invalid_search_cursor"
        );
        request.cursor.clear();
        // Same corrected caller across two independent scopes is intentionally
        // still two native rows plus its structural proof. Source ties preserve
        // append order under Rust's stable sort; no deduplication is permitted.
        let mut input = Vec::new();
        for (path, line, name) in [
            ("ActionButton.lua", 141, "ActionButtonDown"),
            ("ActionButton.lua", 156, "ActionButtonUp"),
            (
                "Keybindings.lua",
                318,
                "KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame",
            ),
        ] {
            for basis in ["static", "classic", "mainline"] {
                input.push(json!({"path":path,"line":line,"kind":"call","source":name,"target":"GetActionButtonForID","proof":format!("{name}:{basis}"),"basis":[basis]}));
            }
        }
        // A source-name tie-breaker really participates in the production sort.
        input.push(json!({"path":"ActionButton.lua","line":141,"kind":"call","source":"AFirst","target":"GetActionButtonForID","proof":"ordered-first"}));
        input.insert(0,json!({"path":"ActionButton.lua","line":141,"kind":"call","source":"ZLast","target":"GetActionButtonForID","proof":"ordered-last"}));
        let expected = [
            "ordered-first",
            "ActionButtonDown:static",
            "ActionButtonDown:classic",
            "ActionButtonDown:mainline",
            "ordered-last",
            "ActionButtonUp:static",
            "ActionButtonUp:classic",
            "ActionButtonUp:mainline",
            "KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame:static",
            "KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame:classic",
            "KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame:mainline",
        ];
        let mut collected = Vec::new();
        let mut pages = 0;
        loop {
            let (offset, current) =
                relation_cursor(&request, &source, 2, "incoming", rules, None).unwrap();
            assert_eq!(current, key);
            let mut rows = input.clone();
            let mut data = json!({"coverage":{"semantic":"partial"},"complete":false});
            project_relation_page(&mut data, &mut rows, offset, &key, 2, false).unwrap();
            pages += 1;
            assert_eq!(data["coverage"]["semantic"], "partial");
            assert_eq!(data["complete"], false);
            let page = data["relations"].as_array().unwrap();
            assert!(page.len() <= 2);
            collected.extend(
                page.iter()
                    .map(|row| row["proof"].as_str().unwrap().to_owned()),
            );
            if let Some(next) = data["nextCursor"].as_str() {
                assert_eq!(data["truncated"], true);
                request.cursor = next.into();
            } else {
                assert_eq!(data["truncated"], false);
                break;
            }
        }
        assert_eq!(pages, 6);
        assert_eq!(collected, expected);
        let mut rows = input;
        let mut data = json!({});
        project_relation_page(&mut data, &mut rows, 0, &key, 200, true).unwrap();
        assert_eq!(data["relations"].as_array().unwrap().len(), 11);
        assert_eq!(data["truncated"], true);
        assert_eq!(
            project_relation_page(&mut data, &mut rows, 12, &key, 2, false)
                .unwrap_err()
                .code,
            "codebase.invalid_search_cursor"
        );
    }

    #[test]
    fn removed_relation_rows_keep_exact_reply_and_owned_string_storage() {
        let mut response = json!({"relations":[{"path":"Original.xml", "source":"调用", "proof":{"items":[1,2,3]}}],
            "analysis":{"state":"partial"},"sourceMaps":[{"path":"Original.xml"}],"semanticExecution":{"state":"executed"}});
        let pointer = response["relations"][0]["source"]
            .as_str()
            .unwrap()
            .as_ptr();
        let mut expected = response.clone();
        let expected_rows = expected
            .as_object_mut()
            .and_then(|value| value.remove("relations"))
            .and_then(|value| value.as_array().cloned());
        let actual = take_native_relation_rows(&mut response).unwrap();
        assert_eq!(actual[0]["source"].as_str().unwrap().as_ptr(), pointer);
        assert_eq!(Some(&actual), expected_rows.as_ref());
        assert_eq!(
            serde_json::to_vec(&response).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
    }

    #[test]
    fn absent_empty_or_invalid_relation_member_keeps_previous_projection() {
        for mut response in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"relations":null}),
            json!({"relations":{"invalid":true},"analysis":{}}),
            json!({"relations":[]}),
        ] {
            let mut expected = response.clone();
            let expected_rows = expected
                .as_object_mut()
                .and_then(|value| value.remove("relations"))
                .and_then(|value| value.as_array().cloned());
            assert_eq!(take_native_relation_rows(&mut response), expected_rows);
            assert_eq!(response, expected);
        }
    }
}
