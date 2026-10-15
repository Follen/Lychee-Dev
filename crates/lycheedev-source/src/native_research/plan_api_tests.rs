use super::*;
use emmylua_parser::{
    LuaAstNode, LuaDocTagReturn, LuaLanguageLevel, LuaParseErrorKind, LuaParser, ParserConfig,
};
use lycheedev_core::{control::OperationControl, storage};
use std::collections::BTreeMap;

const RETAIL_COMMIT: &str = "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b";

fn identity() -> environment::Identity {
    environment::Identity {
        repository: "wow-ui-source".into(),
        commit: RETAIL_COMMIT.into(),
        client: "retail".into(),
        dependency_commits: BTreeMap::new(),
        generator_version: environment::GENERATOR.into(),
    }
}

fn retail_environment() -> (environment::Environment, BTreeMap<String, String>) {
    // Complete, unchanged documents from the same authenticated Retail archive
    // as the Windows full-corpus failure; no studied source is executed.
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tests/rust-source-data/retail-api-return-labels.json"
    ))
    .unwrap();
    assert_eq!(fixture["commit"], RETAIL_COMMIT);
    let files: BTreeMap<String, String> = fixture["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let text = item["text"].as_str().unwrap();
            assert_eq!(item["bytes"], text.len());
            assert_eq!(item["sha256"], storage::sha256(text.as_bytes()));
            (item["path"].as_str().unwrap().into(), text.into())
        })
        .collect();
    assert_eq!(files.len(), 2);
    let documents = files
        .iter()
        .map(|(path, text)| (path.clone(), text.as_bytes().to_vec()))
        .collect();
    let api = environment::build(
        identity(),
        &documents,
        environment::Limits::default(),
        &OperationControl::default(),
    )
    .unwrap();
    (api, files)
}

fn symbol(record: &environment::ApiRecord) -> index::Symbol {
    index::Symbol {
        id: String::new(),
        kind: "declaration".into(),
        name: record.name.clone(),
        target: String::new(),
        category: "api-function".into(),
        confidence: String::new(),
        path: record.path.clone(),
        line: record.line,
        end_line: record.end_line,
        signature: String::new(),
        scope: String::new(),
        ordinal: 0,
    }
}

#[test]
fn retail_get_net_stats_return_name_reproduces_emmy_doc_error() {
    let legacy = "---@return number in\n---@return number out\n---@return number latencyList\nfunction GetNetStats() end\n";
    let tree = LuaParser::parse(legacy, ParserConfig::with_level(LuaLanguageLevel::Lua51));
    assert!(
        tree.get_errors()
            .iter()
            .any(|error| error.kind == LuaParseErrorKind::DocError),
        "the original full-corpus failure must remain represented"
    );
    let repaired = legacy.replace("number in\n", "number # in\n");
    let tree = LuaParser::parse(&repaired, ParserConfig::with_level(LuaLanguageLevel::Lua51));
    assert!(tree.get_errors().is_empty(), "{:?}", tree.get_errors());
    let types: Vec<_> = tree
        .get_red_root()
        .descendants()
        .filter_map(LuaDocTagReturn::cast)
        .flat_map(|tag| tag.get_types())
        .map(|ty| ty.syntax().text().to_string())
        .collect();
    assert_eq!(types, ["number", "number", "number"]);
}

#[test]
fn retail_api_rebuild_preserves_original_return_names_and_authenticates_mappings() {
    let (api, files) = retail_environment();
    let net = api.facts.iter().find(|r| r.name == "GetNetStats").unwrap();
    assert_eq!(net.line, 39);
    assert_eq!(net.returns[0].name, "in");
    assert_eq!(net.returns[0].line, 45);
    assert_eq!(net.returns[0].raw["Name"], "in");
    assert_eq!(net.returns[2].raw["StrideIndex"], 1);
    assert!(api.definitions.contains("---@return number # in\n"));
    assert!(api.definitions.contains("---@return number out\n"));
    let control = OperationControl::default();
    let verified =
        VerifiedApiDefinitions::rebuild(&files, &api.facts, Some(&api.manifest), &control)
            .unwrap()
            .unwrap();
    for name in ["GetNetStats", "C_Timer.After"] {
        let record = api.facts.iter().find(|r| r.name == name).unwrap();
        let target = verified.target(&symbol(record)).unwrap().unwrap();
        assert_eq!(target.file_sha256, api.manifest.definitions_sha256);
        assert_eq!(target.material["mapping"]["sourcePath"], record.path);
        assert_eq!(target.material["mapping"]["sourceLine"], record.line);
        assert_eq!(
            &verified.definitions[target.site.start as usize..target.site.end as usize],
            name.rsplit('.').next().unwrap()
        );
    }
    let mut tampered = files.clone();
    let path = net.path.clone();
    tampered.insert(path.clone(), files[&path].replace("GetNetStats", "Changed"));
    assert_eq!(
        VerifiedApiDefinitions::rebuild(&tampered, &api.facts, Some(&api.manifest), &control)
            .err()
            .unwrap()
            .code,
        "source.native_plan_environment"
    );
}

#[test]
fn retail_reserved_return_label_does_not_block_unrelated_api_semantic_type() {
    let (api, files) = retail_environment();
    let control = OperationControl::default();
    let verified =
        VerifiedApiDefinitions::rebuild(&files, &api.facts, Some(&api.manifest), &control)
            .unwrap()
            .unwrap();
    let record = api
        .facts
        .iter()
        .find(|r| r.name == "C_Timer.After")
        .unwrap();
    let target = verified.target(&symbol(record)).unwrap().unwrap();
    let host =
        crate::native_research::api_environment(&api.facts, Some(&api.manifest), &control).unwrap();
    let result = verified
        .semantic_type(&target, host.bindings(), &control, || Ok(()))
        .unwrap();
    assert_eq!(result["executed"], true, "{result}");
    assert_eq!(result["state"], "known", "{result}");
    assert_eq!(result["result"]["sourceExecution"], false);
    assert!(!result["result"]["coverage"].as_array().unwrap().is_empty());
}

#[test]
fn retail_return_labels_allow_ordinary_research_preparation_and_query() {
    use crate::native_research::{QueryRequest, ResearchSession, SnapshotRequest};
    let (api, files) = retail_environment();
    let control = OperationControl::default();
    let mut session = ResearchSession::prepare(
        SnapshotRequest {
            files: BTreeMap::from([("logic.lua".into(), "function Probe() end\nProbe()\n".into())]),
            load_documents: BTreeMap::new(),
            environment_documents: files,
            source: serde_json::json!({"repository":"return-label-regression","product":"retail"}),
            // This in-process analysis test does not invoke a Lua compiler.
            luac: crate::native::LuacIdentity {
                version: "5.1.5".into(),
                platform: "windows-amd64".into(),
                executable_sha256: "a".repeat(64),
                executable_bytes: 1,
            },
            api: api.facts.clone(),
            environment_manifest: Some(api.manifest),
            load_plans: vec![],
            input_gaps: vec![],
        },
        &control,
    )
    .unwrap();
    let mut target = symbol(&api.facts[0]);
    target.name = "Probe".into();
    target.path = "logic.lua".into();
    target.category = "function".into();
    target.line = 1;
    target.end_line = 1;
    let query = QueryRequest {
        symbol: target,
        direction: "incoming".into(),
    };
    let result = session.query(query.clone(), &control).unwrap();
    // No load order was supplied: retain the honest partial/static result.
    assert_eq!(result["semanticPlan"]["state"], "execution_incomplete");
    let repeated = session.query(query, &control).unwrap();
    assert_eq!(repeated["semanticPlan"]["state"], "execution_incomplete");
}

#[test]
fn unparseable_api_model_stays_rejected_with_bounded_error_coordinates() {
    let path = "Interface/AddOns/Blizzard_APIDocumentationGenerated/BadDocumentation.lua";
    let text = "APIDocumentation:AddDocumentationTable({Tables={{Name='keyof',Type='Structure'}}})";
    let control = OperationControl::default();
    let api = environment::build(
        identity(),
        &BTreeMap::from([(path.into(), text.as_bytes().to_vec())]),
        environment::Limits::default(),
        &control,
    )
    .unwrap();
    let error = VerifiedApiDefinitions::rebuild(
        &BTreeMap::from([(path.into(), text.into())]),
        &api.facts,
        Some(&api.manifest),
        &control,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "source.native_plan_environment");
    assert_eq!(error.exit_code, 4);
    assert!(error.message.contains("parse errors"));
    assert_eq!(
        error.details["definitionsSHA256"],
        api.manifest.definitions_sha256
    );
    assert!(error.details["parseErrorCount"].as_u64().unwrap() > 0);
    assert!(
        error.details["firstError"]["endByte"].as_u64().unwrap() <= api.definitions.len() as u64
    );
    assert!(serde_json::to_string(&error).unwrap().len() < 1024);
}

#[test]
fn return_labels_cannot_become_type_expressions_or_new_declarations() {
    for name in [
        "in",
        "extends",
        "keyof",
        "as",
        "and",
        "or",
        "else",
        "true",
        "false",
        "nil",
        "end",
        "",
        "a, string",
        "x|nil",
        "foo.bar",
        "[string]",
        "a?",
        "value-name",
        "line\r\nfunction Injected() end",
        "中文",
        "goto",
        "out",
        "private",
        "async",
    ] {
        let text = format!(
            "APIDocumentation:AddDocumentationTable({{Functions={{{{Name='Probe',Type='Function',Returns={{{{Name={},Type='number'}},{{Name='nextResult',Type='string'}}}}}}}}}})",
            serde_json::to_string(name).unwrap()
        );
        let api = environment::build(
            identity(),
            &BTreeMap::from([("API.lua".into(), text.into_bytes())]),
            environment::Limits::default(),
            &OperationControl::default(),
        )
        .unwrap();
        assert_eq!(api.facts[0].returns[0].name, name);
        assert_eq!(api.facts[0].returns[0].raw["Name"], name);
        let tree = LuaParser::parse(
            &api.definitions,
            ParserConfig::with_level(LuaLanguageLevel::Lua51),
        );
        assert!(
            tree.get_errors().is_empty(),
            "{name:?}: {:?}",
            tree.get_errors()
        );
        let types: Vec<_> = tree
            .get_red_root()
            .descendants()
            .filter_map(LuaDocTagReturn::cast)
            .flat_map(|tag| tag.get_types())
            .map(|ty| ty.syntax().text().to_string())
            .collect();
        assert_eq!(types, ["number", "string"], "{name:?}");
        assert_eq!(
            api.definitions
                .lines()
                .filter(|line| line.starts_with("function "))
                .collect::<Vec<_>>(),
            ["function Probe() end"]
        );
    }
}
