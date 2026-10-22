use lycheedev_source::environment::{self, Identity, Limits};
use std::collections::BTreeMap;
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
    let cancel = lycheedev_core::control::TrackedCancellation::new();
    cancel.cancel();
    let control = lycheedev_core::control::OperationControl::tracked(
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
