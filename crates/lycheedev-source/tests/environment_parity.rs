use lycheedev_source::environment::{self, Identity, Limits};
use std::collections::BTreeMap;
#[test]
fn frozen_go_literal_catalog_and_declaration_parity() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/environment-cases.json"
    ))
    .unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/environment-go.json"
    ))
    .unwrap();
    for (case, expected) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(expected.as_array().unwrap())
    {
        let docs: BTreeMap<String, Vec<u8>> = case["documents"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().as_bytes().to_vec()))
            .collect();
        let result = environment::build(
            Identity {
                repository: "Gethe/wow-ui-source".into(),
                commit: "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b".into(),
                client: "retail".into(),
                dependency_commits: BTreeMap::new(),
                generator_version: environment::GENERATOR.into(),
            },
            &docs,
            Limits::default(),
            &Default::default(),
        )
        .unwrap();
        let mut actual = serde_json::to_value(result).unwrap();
        let spans = actual
            .as_object_mut()
            .unwrap()
            .remove("sourceSpans")
            .unwrap();
        assert_eq!(
            spans.as_array().unwrap().len(),
            actual["facts"].as_array().unwrap().len()
        );
        if case["name"] == "goto-identifier" {
            // Bundled Lua 5.1 compiler coverage separately authenticates this
            // legal identifier. The frozen gopher grammar falsely reserves it.
            assert_eq!(expected["result"]["coverage"]["gaps"][0]["code"], "parse");
            assert_eq!(actual["coverage"]["complete"], true);
            assert_eq!(actual["facts"][0]["name"], "goto");
            continue;
        }
        // The old fixtures still define every unaffected fact and declaration.
        // The new owner/callback rules intentionally change the generator identity,
        // including inputs without ScriptObject declarations, to retire old caches.
        let mut versioned_expected=expected["result"].clone();
        assert_eq!(versioned_expected["manifest"]["identity"]["generatorVersion"],"environment-ts-v1");
        versioned_expected["manifest"]["identity"]["generatorVersion"]=serde_json::json!(environment::GENERATOR);
        assert_eq!(actual, versioned_expected, "{}", case["name"]);
    }
    assert_eq!(
        cases.as_array().unwrap().len(),
        expected.as_array().unwrap().len()
    );
}
#[test]
fn bounded_catalog_and_cancel_before_parsing() {
    let identity = Identity {
        repository: "repo".into(),
        commit: "a".repeat(40),
        client: "retail".into(),
        dependency_commits: BTreeMap::new(),
        generator_version: environment::GENERATOR.into(),
    };
    let docs = BTreeMap::from([("API.lua".into(), vec![b' '; 1024])]);
    assert_eq!(
        environment::build(
            identity.clone(),
            &docs,
            Limits {
                file_bytes: 512,
                ..Default::default()
            },
            &Default::default()
        )
        .unwrap_err()
        .code,
        "environment.input_budget"
    );
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let control = lycheedev_core::control::OperationControl::new(
        std::time::Instant::now() + std::time::Duration::from_secs(1),
        cancel,
    );
    assert_eq!(
        environment::build(identity, &docs, Limits::default(), &control)
            .unwrap_err()
            .exit_code,
        7
    );
}
#[test]
fn precise_spans_keep_bom_original_hash_and_full_table_extent() {
    let raw="\u{feff}APIDocumentation:AddDocumentationTable({Functions={\n {Name='Fn',\n Type='Function'}\n}})".as_bytes().to_vec();
    let docs = BTreeMap::from([("中文/API.lua".into(), raw.clone())]);
    let env = environment::build(
        Identity {
            repository: "wow-ui-source".into(),
            commit: "a".repeat(40),
            client: "retail".into(),
            dependency_commits: BTreeMap::new(),
            generator_version: environment::GENERATOR.into(),
        },
        &docs,
        Limits::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(env.facts[0].line, 2);
    assert_eq!(env.facts[0].end_line, 2);
    let span = &env.source_spans[0];
    assert_eq!(span["firstLine"], 2);
    assert_eq!(span["lastLine"], 3);
    assert_eq!(span["contentHash"], lycheedev_core::storage::sha256(&raw));
    let extent = &raw
        [span["startByte"].as_u64().unwrap() as usize..span["endByte"].as_u64().unwrap() as usize];
    assert_eq!(extent, b"{Name='Fn',\n Type='Function'}");
}
