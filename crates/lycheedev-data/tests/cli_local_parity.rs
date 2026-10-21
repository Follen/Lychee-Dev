#[path = "support/current_local_data.rs"]
mod current_local_data;
#[path = "support/data_cache.rs"]
mod data_cache;
use lycheedev_core::{Invocation, evidence, storage, workspace};
use lycheedev_data::definitions::{self, Manifest};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};
#[test]
fn manifest_rejects_ambiguous_and_malformed_identities() {
    let raw = br#"[{"tableName":"Map","tableHash":"BD84CD62","db2FileDataID":1349477}]"#;
    let manifest = Manifest::parse(raw).unwrap();
    assert_eq!(manifest.lookup("mAp").unwrap().db2_file_data_id, 1349477);
    assert!(manifest.resolve("Map", 1349477, 0).is_err());
    for bad in [
        r#"[{"tableName":"Map","tableName":"Other","tableHash":"BD84CD62"}]"#,
        r#"[{"tableName":"Map","tableHash":"BD84CD62","db2FileDataID":null}]"#,
        r#"[{"tableName":"Map","tableHash":"BD84CD62","db2FileDataID":0}]"#,
        r#"[{"tableName":"Map","tableHash":"BD84CD62","unknown":1}]"#,
        r#"[{"tableName":"Map","tableHash":"BD84CD62"},{"tableName":"map","tableHash":"00000000"}]"#,
        r#"[{"tableName":"../Map","tableHash":"BD84CD62"}]"#,
    ] {
        assert!(Manifest::parse(bad.as_bytes()).is_err(), "{bad}");
    }
}
#[test]
fn definition_revision_exact_and_offline_observation_contract() {
    let home = std::env::temp_dir().join(format!(
        "lychee-revision-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let commit = "a".repeat(40);
    assert_eq!(
        definitions::resolve_revision(&home, &commit, true).unwrap(),
        commit
    );
    assert!(!home.exists());
    workspace::initialize(&home).unwrap();
    assert_eq!(
        definitions::resolve_revision(&home, "", true)
            .unwrap_err()
            .code,
        "records.definition_unavailable_offline"
    );
    let metadata = data_cache::Cache::new(&home);
    let key = format!("definition-ref/{}", storage::sha256(b"refs/heads/master"));
    metadata
        .put(
            &key,
            &json!({"reference":"refs/heads/master","commit":commit}),
            None,
        )
        .unwrap();
    assert_eq!(
        definitions::resolve_revision(&home, "", true).unwrap(),
        commit
    );
    for bad in [
        "master",
        "refs/heads/../x",
        "refs/heads/a.lock",
        "refs/heads/a//b",
        "refs/heads/a?b",
        "refs/tags/",
    ] {
        assert_eq!(
            definitions::resolve_revision(&home, bad, true)
                .unwrap_err()
                .code,
            "records.definition_identity"
        );
    }
    drop(metadata);
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
#[ignore = "explicit task-owned workspace and real upstream HTTPS required"]
fn real_definition_transport_and_revision_observation() {
    let home = PathBuf::from(
        std::env::var("LYCHEEDEV_RUST_DEFINITION_NETWORK_HOME")
            .expect("explicit isolated network proof home"),
    );
    workspace::initialize(&home).unwrap();
    let (bundle, _) = definitions::prepare(
        &home,
        "83057bdc0cbe13062850ebf8ad530031e128a1cd",
        "Map",
        false,
    )
    .unwrap();
    assert_eq!(
        bundle.manifest["sha256"],
        "4314266fe055e30207e1c39498a7a31058a1aa2ab6cd674726491b313d135265"
    );
    assert_eq!(
        bundle.definition["sha256"],
        "972819af8ae37fcdcf77c1e6fa0b349852199d96f5b991651587ff68d57575c0"
    );
    let latest = definitions::resolve_revision(&home, "refs/heads/master", false).unwrap();
    assert_eq!(
        definitions::resolve_revision(&home, "", true).unwrap(),
        latest
    );
    let (cached, _) = definitions::prepare(&home, &bundle.commit, "Map", true).unwrap();
    assert_eq!(bundle.manifest, cached.manifest);
    std::fs::write(home.parent().unwrap().join("definition-transport-report.json"),serde_json::to_vec_pretty(&json!({"bundle":bundle,"observedReference":"refs/heads/master","observedCommit":latest,"offlineAliasMatches":true,"fixedResourceHashesMatch":true})).unwrap()).unwrap();
}
#[test]
fn offline_definition_bytes_are_rehashed_and_not_replaced() {
    let home = std::env::temp_dir().join(format!(
        "lychee-definition-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    workspace::initialize(&home).unwrap();
    let commit = "a".repeat(40);
    let base = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{commit}/");
    assert_eq!(
        definitions::prepare(&home, &commit, "Map", true)
            .unwrap_err()
            .code,
        "records.definition_unavailable_offline"
    );
    let metadata = data_cache::Cache::new(&home);
    for (path, bytes) in [
        (
            "manifest.json",
            br#"[{"tableName":"Map","tableHash":"BD84CD62","db2FileDataID":1349477}]"#.as_slice(),
        ),
        (
            "definitions/Map.dbd",
            b"COLUMNS\nint ID\n\nBUILD 12.1.0.69933\nLAYOUT 12345678\n$id$ID<32>\n".as_slice(),
        ),
    ] {
        let url = format!("{base}{path}");
        let blob = evidence::publish_blob(&home, bytes, 4 << 20).unwrap();
        metadata
            .put(
                &format!("definition/{}", storage::sha256(url.as_bytes())),
                &json!({"url":url,"blob":blob}),
                None,
            )
            .unwrap();
    }
    let (bundle, _) = definitions::prepare(&home, &commit, "map", true).unwrap();
    assert_eq!(bundle.identity.name, "Map");
    let url = format!("{base}manifest.json");
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let generation = metadata.generation(&key).unwrap();
    metadata
        .put(
            &key,
            &json!({"url":"https://invalid.example/","blob":bundle.manifest}),
            generation,
        )
        .unwrap();
    assert_eq!(
        definitions::prepare(&home, &commit, "Map", true)
            .unwrap_err()
            .code,
        "records.definition_identity"
    );
    drop(metadata);
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
#[ignore = "explicit read-only actual client input and task-owned proof workspace required"]
fn real_pinned_public_sql_and_asset_dispatch() {
    let fixture = current_local_data::Fixture::load();
    let home = PathBuf::from(
        std::env::var("LYCHEEDEV_RUST_DATA_CLI_HOME").expect("fresh isolated proof home"),
    );
    let pin = fixture.initialize(&home);
    let client = &fixture.installation;
    let record_id = fixture.record_id();
    let flags = BTreeMap::from([
        ("--snapshot".into(), pin.clone()),
        (
            "--installation".into(),
            client.to_string_lossy().into_owned(),
        ),
        ("--offline".into(), "true".into()),
        (
            "--sql".into(),
            "SELECT ID, Directory, MapName_lang FROM Map WHERE ID=:id".into(),
        ),
        ("--param".into(), format!("id={record_id}")),
    ]);
    let inv = Invocation {
        command: "data sql".into(),
        control: Default::default(),
        positional: None,
        flags,
        home: home.clone(),
        cwd: home.clone(),
    };
    let sql = lycheedev_data::migration_oracle::dispatch(&inv).unwrap();
    fixture.assert_outcome("sql", &sql, &["result"]);
    assert!(sql.complete);
    let mut asset_inv = inv.clone();
    asset_inv.command = "asset inspect".into();
    asset_inv.flags.remove("--sql");
    asset_inv.flags.remove("--param");
    asset_inv.flags.insert(
        "--file-id".into(),
        fixture.table_identity().db2_file_data_id.to_string(),
    );
    let asset = lycheedev_data::migration_oracle::dispatch(&asset_inv).unwrap();
    let raw = fixture.bytes(&fixture.document["map"]["raw"]);
    assert_eq!(asset.data["content"]["sha256"], storage::sha256(&raw));
    assert_eq!(
        asset.data["content"]["sha256"],
        fixture.oracle("asset")["result"]["content"]["sha256"]
    );
    assert_eq!(asset.data["content"]["bytes"], raw.len());
    assert_eq!(asset.data["contentVerified"], true);
    for capture in sql.provenance.iter().chain(asset.provenance.iter()) {
        let archived = evidence::read_capture(&home, capture["id"].as_str().unwrap()).unwrap();
        assert_eq!(archived["complete"], true);
        evidence::read_blob(&home, &archived["blob"], 16 << 20).unwrap();
    }
    std::fs::write(
        home.parent().unwrap().join("public-dispatch-report.json"),
        serde_json::to_vec_pretty(&json!({"pin":pin,"sql":sql,"asset":asset})).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "current Go-authenticated CASC/DBD fixture and fresh DB2 proof home required"]
fn real_db2_record_page_schema_search_foreign_key_profiles() {
    let fixture = current_local_data::Fixture::load();
    let home = PathBuf::from(
        std::env::var("LYCHEEDEV_RUST_DATA_CLI_HOME").expect("fresh isolated proof home"),
    );
    let pin = fixture.initialize(&home);
    let client = &fixture.installation;
    let record_id = fixture.record_id();
    let run = |command: &str, extra: &[(&str, &str)]| {
        let mut flags = BTreeMap::from([
            ("--snapshot".into(), pin.clone()),
            (
                "--installation".into(),
                client.to_string_lossy().into_owned(),
            ),
            ("--offline".into(), "true".into()),
        ]);
        for (name, value) in extra {
            flags.insert(name.to_string(), value.to_string());
        }
        lycheedev_data::db2::dispatch(&Invocation {
            command: command.into(),
            positional: if command == "data db2" {
                None
            } else {
                Some("Map".into())
            },
            flags,
            home: home.clone(),
            cwd: home.clone(),
            control: Default::default(),
        })
    };
    let record = run("data db2", &[("--table", "Map"), ("--id", &record_id)]).unwrap();
    fixture.assert_outcome("record", &record, &["recordID", "row"]);
    assert!(record.complete);
    let page = run("data db2", &[("--table", "Map"), ("--limit", "3")]).unwrap();
    assert_eq!(page.data["page"]["rows"].as_array().unwrap().len(), 3);
    fixture.assert_outcome("page", &page, &["page", "truncated"]);
    assert_eq!(page.data["page"]["more"], true);
    assert!(!page.complete);
    assert_eq!(page.warnings.len(), 1);
    let next = page.data["page"]["next"].as_u64().unwrap().to_string();
    let following = run(
        "data db2",
        &[("--table", "Map"), ("--limit", "3"), ("--after-id", &next)],
    )
    .unwrap();
    fixture.assert_outcome("following", &following, &["page", "truncated"]);
    assert!(following.data["page"]["ids"][0].as_u64().unwrap() > next.parse::<u64>().unwrap());
    let terminal = run(
        "data db2",
        &[("--table", "Map"), ("--after-id", "4294967295")],
    )
    .unwrap();
    fixture.assert_outcome("terminal", &terminal, &["page", "truncated"]);
    assert_eq!(terminal.data["page"]["ids"], json!([]));
    assert!(!terminal.complete);
    assert_eq!(terminal.data["page"]["more"], false);
    let schema = run("data db2 schema", &[]).unwrap();
    fixture.assert_outcome(
        "schema",
        &schema,
        &[
            "rowCount",
            "fields",
            "keys",
            "relationshipFields",
            "arrayFields",
        ],
    );
    assert_eq!(
        schema.data["rowCount"],
        fixture.document["map"]["rows"]["count"]
    );
    assert!(
        schema.data["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "ID" && f["type"] == "dbFieldNonInlineID")
    );
    let search = run(
        "data db2 search",
        &[
            ("--field", "Directory"),
            (
                "--query",
                fixture.document["probes"]["searchQuery"].as_str().unwrap(),
            ),
            ("--limit", "1"),
        ],
    )
    .unwrap();
    fixture.assert_outcome("search", &search, &["rows", "count", "truncated"]);
    let truncated = run(
        "data db2 search",
        &[
            ("--field", "Directory"),
            (
                "--query",
                fixture.document["probes"]["truncatedSearchQuery"]
                    .as_str()
                    .unwrap(),
            ),
            ("--limit", "2"),
        ],
    )
    .unwrap();
    fixture.assert_outcome(
        "truncatedSearch",
        &truncated,
        &["rows", "count", "truncated"],
    );
    assert_eq!(truncated.data["count"], 2);
    assert_eq!(truncated.data["truncated"], true);
    assert!(!truncated.complete);
    let foreign = run(
        "data db2 foreign-key",
        &[("--field", "ID"), ("--value", &record_id), ("--limit", "1")],
    )
    .unwrap();
    fixture.assert_outcome("foreignKey", &foreign, &["rows", "count", "truncated"]);
    assert_eq!(foreign.data["rows"][0], record.data["row"]);
    assert!(foreign.complete);
    let invalid = run(
        "data db2 foreign-key",
        &[("--field", "Directory"), ("--value", "0"), ("--limit", "1")],
    )
    .unwrap_err();
    assert_eq!(invalid.exit_code, 2);
    assert_eq!(
        invalid.code,
        fixture.oracle("invalidForeignKey")["error"]["code"]
    );
    for out in [
        &record, &page, &following, &terminal, &schema, &search, &truncated, &foreign,
    ] {
        for cap in &out.provenance {
            let read = evidence::read_capture(&home, cap["id"].as_str().unwrap()).unwrap();
            evidence::read_blob(&home, &read["blob"], 16 << 20).unwrap();
        }
    }
    std::fs::write(home.parent().unwrap().join("db2-dispatch-report.json"),serde_json::to_vec_pretty(&json!({"record":record,"page":page,"following":following,"terminal":terminal,"schema":schema,"search":search,"truncated":truncated,"foreignKey":foreign,"pin":pin})).unwrap()).unwrap();
}
