#[path = "support/current_local_data.rs"]
mod current_local_data;
use blte::ByteSource;
use lycheedev_data::{
    blte, casc, installed,
    metadata_format::{self, Encoding, RootLimits},
    resource::{Budget, Cost},
};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
fn md5(raw: &[u8]) -> String {
    use md5::{Digest, Md5};
    casc::hex(&Md5::digest(raw))
}
fn parser_budget(limits: Cost) -> Budget {
    Budget::new(limits, Instant::now() + Duration::from_secs(30), None)
}
#[test]
fn current_data_json_numeric_equivalence_preserves_exact_boundaries() {
    let parse = |text: &str| current_local_data::parse_json(text.as_bytes());
    // The fixture reader must preserve the lexical sign before comparison.
    let negative_zero = serde_json::Value::Number(serde_json::Number::from_f64(-0.0).unwrap());
    assert!(current_local_data::json_equal(&parse("-0"), &negative_zero));
    assert!(!current_local_data::json_equal(&parse("0"), &negative_zero));
    let nested = parse(r#"{"-0":[-0,"-0"]}"#);
    assert!(current_local_data::json_equal(
        &nested["-0"][0],
        &negative_zero
    ));
    assert_eq!(nested["-0"][1], serde_json::json!("-0"));
    for (a, b) in [
        ("0", "0.0"),
        ("1", "1.0"),
        ("2e0", "2.0"),
        ("-0", "-0.0"),
        ("[-0,{\"-0\":\"-0\"}]", "[-0.0,{\"-0\":\"-0\"}]"),
        ("[1,{\"x\":2.0}]", "[1.0,{\"x\":2e0}]"),
    ] {
        assert!(
            current_local_data::json_equal(&parse(a), &parse(b)),
            "{a} = {b}"
        );
    }
    for (a, b) in [
        ("9007199254740992", "9007199254740993"),
        ("18446744073709551614", "18446744073709551615"),
        ("-9223372036854775808", "-9223372036854775807"),
        ("0", "-0"),
        ("0.0", "-0.0"),
        ("0.1", "0.10000000000000001"),
        ("1571.57", "1571.5699462890625"),
        ("-230.989", "-230.98899841308594"),
    ] {
        assert!(
            !current_local_data::json_equal(&parse(a), &parse(b)),
            "{a} != {b}"
        );
    }
}
#[test]
fn parser_admits_token_storage_before_copying_and_releases_borrowed_scratch() {
    let raw = format!("dense = {}\n", "x ".repeat(4096)).into_bytes();
    let key = md5(&raw);
    let tight = parser_budget(Cost {
        metadata_bytes: raw.len() as u64,
        ..Cost::defaults()
    });
    let error =
        installed::parse_configuration_budgeted(raw.clone(), &key, Some(&tight)).unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    // The first field could not be admitted; its temporary retained charge was
    // released and no owned token array/name/map node was committed.
    assert_eq!(tight.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(tight.snapshot().unwrap().metadata_bytes, 0);
    let budget = parser_budget(Cost::defaults());
    let actual = installed::parse_configuration_budgeted(raw.clone(), &key, Some(&budget)).unwrap();
    let expected = installed::parse_configuration(raw, &key).unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(budget.snapshot().unwrap().metadata_bytes >= (4096 * size_of::<String>()) as u64);

    let raw = format!(
        "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.0.69933|{key}|{key}|1\n"
    );
    let budget = parser_budget(Cost::defaults());
    let actual = installed::installed_rows_budgeted(raw.as_bytes(), Some(&budget)).unwrap();
    assert_eq!(actual, installed::installed_rows(raw.as_bytes()).unwrap());
    let used = budget.snapshot().unwrap();
    assert!(used.retained_bytes > 0, "owned row strings and Vec escape");
    assert!(
        used.metadata_bytes > used.retained_bytes,
        "headers, columns, values and version components are temporary"
    );
}
#[test]
fn budgeted_parsers_preserve_authentication_and_field_limit_error_order() {
    let budget = parser_budget(Cost::defaults());
    let invalid_text = vec![0xff, 0];
    assert_eq!(
        installed::parse_configuration_budgeted(invalid_text.clone(), "bad", Some(&budget))
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    assert_eq!(
        installed::parse_configuration_budgeted(
            invalid_text.clone(),
            &"a".repeat(32),
            Some(&budget)
        )
        .unwrap_err()
        .code,
        "records.invalid_metadata"
    );
    assert_eq!(
        installed::parse_configuration_budgeted(
            invalid_text.clone(),
            &md5(&invalid_text),
            Some(&budget)
        )
        .unwrap_err()
        .code,
        "records.invalid_metadata"
    );
    let mut fields = String::new();
    for index in 0..4096 {
        fields.push_str(&format!("field{index} =\n"));
    }
    let exact = installed::parse_configuration_budgeted(
        fields.clone().into_bytes(),
        &md5(fields.as_bytes()),
        Some(&budget),
    )
    .unwrap();
    assert_eq!(exact.fields.len(), 4096);
    for (last, expected) in [
        ("field0 =\n", "records.invalid_metadata"),
        ("extra =\n", "records.metadata_limit"),
    ] {
        let raw = format!("{fields}{last}").into_bytes();
        assert_eq!(
            installed::parse_configuration_budgeted(raw.clone(), &md5(&raw), Some(&budget))
                .unwrap_err()
                .code,
            expected
        );
    }
    let too_many_headers = std::iter::repeat_n("x", 129).collect::<Vec<_>>().join("|");
    assert_eq!(
        installed::installed_rows_budgeted(
            format!("{too_many_headers}\n").as_bytes(),
            Some(&budget)
        )
        .unwrap_err()
        .code,
        "records.metadata_limit",
        "column cap precedes duplicate/missing-column errors"
    );
}
#[test]
fn strict_catalog_and_content_addressed_configuration() {
    let header = "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n";
    let row = format!("wow|12.1.0.69933|{}|{}|1\n", "a".repeat(32), "b".repeat(32));
    let rows = installed::installed_rows(format!("{header}{row}").as_bytes()).unwrap();
    assert_eq!(rows[0].product, "wow");
    assert!(rows[0].active);
    assert!(installed::installed_rows(format!("{header}{row}x|1|2\n").as_bytes()).is_err());
    assert!(installed::installed_rows(header.replace("Product!", "Version!").as_bytes()).is_err());
    let raw =
        b"# config\nroot = 01234567890123456789012345678901\nencoding-size = 20 30\n".to_vec();
    let doc = installed::parse_configuration(raw.clone(), &md5(&raw)).unwrap();
    assert_eq!(doc.fields["encoding-size"], vec!["20", "30"]);
    let mut changed = raw.clone();
    changed.push(b' ');
    assert_eq!(
        installed::parse_configuration(changed, &md5(&raw))
            .unwrap_err()
            .code,
        "records.configuration_integrity"
    );
    let duplicate = b"root = a\nroot = b\n".to_vec();
    assert_eq!(
        installed::parse_configuration(duplicate.clone(), &md5(&duplicate))
            .unwrap_err()
            .exit_code,
        4
    );
    let content = blte::Content {
        bytes: vec![0; 16],
        missing: vec![blte::Missing {
            offset: usize::MAX,
            bytes: 2,
            key_id: "x".into(),
            chunk: 0,
        }],
    };
    assert_eq!(
        content.checked_span(0, 1).unwrap_err().code,
        "container.invalid_format"
    );
}
#[test]
fn target_preserves_observed_cn_slot_and_rejects_cross_slot_borrowing() {
    let root = std::env::temp_dir().join(format!(
        "lychee-slot-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let client = root.join("renamed-client");
    std::fs::create_dir_all(&client).unwrap();
    std::fs::write(
        client.join(".flavor.info"),
        b"Product Flavor!STRING:0\nwow_cn_beta\n",
    )
    .unwrap();
    let config=b"root = 01234567890123456789012345678901\nencoding = 01234567890123456789012345678901 abcdef01234567890123456789012345\nencoding-size = 20 30\n";
    let key = md5(config);
    let parent = root.join(format!("Data/config/{}/{}", &key[..2], &key[2..4]));
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::write(parent.join(&key), config).unwrap();
    let header = "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1|Branch!STRING:0\n";
    let row = |product: &str, build: &str| {
        format!(
            "{product}|{build}|{key}|{key}|1|{}\n",
            if product == "wow_cn_beta" { "cn" } else { "us" }
        )
    };
    std::fs::write(
        root.join(".build.info"),
        format!(
            "{header}{}{}",
            row("wow_cn_beta", "1.60.1.70338"),
            row("wow_classic_beta", "1.60.1.70338")
        ),
    )
    .unwrap();
    let request = installed::LocalTargetRequest {
        installation: client.clone(),
        product: Some("forever-cn-beta".into()),
        full_build: None,
        region: "cn".into(),
        language: "zhCN".into(),
        definition_commit: "a".repeat(40),
    };
    let resolved = installed::resolve_target(&request).unwrap();
    assert_eq!(resolved.data.product, "forever-cn-beta");
    assert_eq!(resolved.data.full_build, "1.60.1.70338");
    assert_eq!(resolved.metadata.installed.product, "wow_cn_beta");
    assert_eq!(resolved.metadata.installed.metadata_branch, "cn");
    assert_eq!(resolved.metadata.installed.channel, "cn-beta");
    assert_eq!(resolved.metadata.installed.display_name, "Forever CN Beta");
    let budget = parser_budget(Cost::defaults());
    let metered = installed::resolve_target_budgeted(&request, Some(&budget)).unwrap();
    assert_eq!(
        serde_json::to_value(&metered).unwrap(),
        serde_json::to_value(&resolved).unwrap()
    );
    let initial = budget.snapshot().unwrap();
    let recheck = || {
        installed::resolve_local_budgeted(
            &metered.common_root,
            &metered.metadata.installed.product,
            &metered.data.full_build,
            Some(&budget),
        )
        .unwrap()
    };
    assert_eq!(
        serde_json::to_value(recheck()).unwrap(),
        serde_json::to_value(&metered.metadata).unwrap()
    );
    let once = budget.snapshot().unwrap();
    recheck();
    let twice = budget.snapshot().unwrap();
    assert!(once.metadata_bytes > initial.metadata_bytes);
    assert_eq!(
        twice.metadata_bytes - once.metadata_bytes,
        once.metadata_bytes - initial.metadata_bytes,
        "revalidation reparses the same files; it must not use charge_once"
    );
    assert_eq!(
        twice.decode_work - once.decode_work,
        once.decode_work - initial.decode_work
    );
    let tight = parser_budget(Cost {
        metadata_bytes: once.metadata_bytes - 1,
        ..Cost::defaults()
    });
    let metered = installed::resolve_target_budgeted(&request, Some(&tight)).unwrap();
    assert_eq!(
        installed::resolve_local_budgeted(
            &metered.common_root,
            &metered.metadata.installed.product,
            &metered.data.full_build,
            Some(&tight)
        )
        .unwrap_err()
        .code,
        "records.query_resource_budget"
    );
    let control = lycheedev_core::control::OperationControl::default();
    let controlled = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    installed::resolve_target_budgeted(&request, Some(&controlled)).unwrap();
    control
        .cancel_handle()
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(
        installed::resolve_local_budgeted(
            &resolved.common_root,
            &resolved.metadata.installed.product,
            &resolved.data.full_build,
            Some(&controlled)
        )
        .unwrap_err()
        .exit_code,
        7
    );
    let legacy_cn = installed::LocalTargetRequest {
        product: Some("forever".into()),
        ..request.clone()
    };
    // Go 3.2's legacy family selector is valid for an exact regional client;
    // the returned Data product remains the observed CN delivery track.
    assert_eq!(
        installed::resolve_target(&legacy_cn).unwrap().data,
        resolved.data
    );
    let asia_client = root.join("another-renamed-client");
    std::fs::create_dir_all(&asia_client).unwrap();
    std::fs::write(
        asia_client.join(".flavor.info"),
        b"Product Flavor!STRING:0\nwow_classic_beta\n",
    )
    .unwrap();
    let asia_request = installed::LocalTargetRequest {
        installation: asia_client,
        product: Some("forever-asia-beta".into()),
        region: "tw".into(),
        ..request.clone()
    };
    let asia = installed::resolve_target(&asia_request).unwrap();
    assert_eq!(asia.data.product, "forever-asia-beta");
    assert_eq!(asia.data.full_build, resolved.data.full_build);
    assert_ne!(asia.data, resolved.data);
    assert_eq!(asia.metadata.installed.product, "wow_classic_beta");
    assert_eq!(asia.metadata.installed.metadata_branch, "us");
    assert_eq!(
        asia.data.region, "tw",
        "publishing branch does not overwrite the caller's region"
    );
    let common_cn = installed::resolve_target(&installed::LocalTargetRequest {
        installation: root.clone(),
        product: Some("forever-cn-beta".into()),
        ..request.clone()
    })
    .unwrap();
    assert_eq!(common_cn.data, resolved.data);
    let common_metered = installed::resolve_target_budgeted(
        &installed::LocalTargetRequest {
            installation: root.clone(),
            product: Some("forever-cn-beta".into()),
            ..request.clone()
        },
        Some(&parser_budget(Cost::defaults())),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(common_metered).unwrap(),
        serde_json::to_value(&common_cn).unwrap()
    );
    let common_asia = installed::resolve_target(&installed::LocalTargetRequest {
        installation: root.clone(),
        product: Some("forever-asia-beta".into()),
        region: "tw".into(),
        ..request.clone()
    })
    .unwrap();
    assert_eq!(common_asia.data, asia.data);
    std::fs::write(
        root.join(".build.info"),
        format!(
            "{header}{}{}",
            row("wow_cn_beta", "1.60.1.70338"),
            row("wow_cn_beta", "1.60.1.70338")
        ),
    )
    .unwrap();
    assert_eq!(
        installed::resolve_target(&request).unwrap_err().code,
        "records.build_ambiguous"
    );
    assert_eq!(
        installed::resolve_target(&installed::LocalTargetRequest {
            full_build: Some("1.60.1.70338".into()),
            ..request.clone()
        })
        .unwrap_err()
        .code,
        "records.build_ambiguous"
    );
    std::fs::write(
        root.join(".build.info"),
        format!("{header}{}", row("wow_classic_beta", "1.60.1.70338")),
    )
    .unwrap();
    assert_eq!(
        installed::resolve_target(&request).unwrap_err().code,
        "selection.installation_metadata_conflict"
    );
    std::fs::write(
        root.join(".build.info"),
        format!("{header}{}", row("wow_cn_beta", "5.5.4.69934")),
    )
    .unwrap();
    assert_eq!(
        installed::resolve_target(&request).unwrap_err().code,
        "selection.installation_metadata_conflict"
    );
    std::fs::write(
        root.join(".build.info"),
        format!("{header}{}", row("wow_cn_beta", "1.60.1.70338")),
    )
    .unwrap();
    std::fs::write(client.join("version.txt"), "1.60.1.70124").unwrap();
    // Fixed Go inspectExactClient returns the successfully resolved canonical
    // wow_forever identity with this later catalog conflict. The explicit
    // regional selector associates only wow_cn_beta issues, so it reports no
    // installation; the unfiltered selector and pinned reader still reject
    // the conflicting client rather than borrowing the sibling's archives.
    let regional_error = installed::resolve_target(&request).unwrap_err();
    assert_eq!(regional_error.code, "selection.installation_missing");
    let issue = &regional_error.details["__commandContext"]["installationSelection"]["issues"][0];
    assert_eq!(issue["productCode"], "wow_forever");
    assert_eq!(issue["reason"], "selection.installation_metadata_conflict");
    assert_eq!(
        installed::resolve_target(&installed::LocalTargetRequest {
            product: None,
            ..request.clone()
        })
        .unwrap_err()
        .code,
        "selection.installation_metadata_conflict"
    );
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &resolved.data, None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires explicitly named read-only actual client and report directory"]
fn real_local_authenticated_encoding_root() {
    let fixture = current_local_data::Fixture::load();
    let installation = fixture.installation.clone();
    let build = &fixture.pin.full_build;
    let output = std::env::var("LYCHEEDEV_RUST_DATA_REPORT").expect("task-owned report path");
    let table_identity = fixture.table_identity();
    let ids = vec![table_identity.db2_file_data_id];
    let start = Instant::now();
    let mut stages = BTreeMap::new();
    let resolved = fixture.resolve();
    let root = resolved.common_root;
    let product = resolved.metadata.installed.product.clone();
    let metadata = resolved.metadata;
    stages.insert("identity", start.elapsed().as_micros());
    let limits = casc::Limits::default();
    let keys = BTreeMap::new();
    let encoding_object = casc::open_local_reader(
        &root,
        &metadata.encoding_key,
        metadata.encoding_bytes,
        limits,
        None,
    )
    .unwrap();
    stages.insert("encodingOpen", start.elapsed().as_micros());
    let ranges = blte::Ranges::open_reader(
        &encoding_object,
        blte::Limits {
            decoded_bytes: metadata.content_bytes,
            ..Default::default()
        },
        &keys,
    )
    .unwrap();
    assert_eq!(ranges.size(), metadata.content_bytes);
    let index = Encoding::open(&ranges, limits, None).unwrap();
    stages.insert("encodingDirectory", start.elapsed().as_micros());
    let root_record = index
        .find(&metadata.root_content_key, limits, None)
        .unwrap();
    let (object, root_content) =
        installed::read_content(&root, &index, &root_record, limits, &keys, None).unwrap();
    stages.insert("rootDecoded", start.elapsed().as_micros());
    let entries = metadata_format::lookup_root(
        &root_content,
        &ids,
        RootLimits {
            operation: limits,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    stages.insert("rootLookup", start.elapsed().as_micros());
    // Full Map and its exact DBD are mandatory: Root authentication alone is
    // not the installed table/SQL acceptance path.
    let name = &table_identity.name;
    let dbd = fixture.bytes(&fixture.document["definitions"]["map"]);
    let matches = entries
        .iter()
        .filter(|entry| {
            entry.file_data_id == table_identity.db2_file_data_id
                && entry.locale_mask & fixture.locale_mask() != 0
                && entry.content_flags & 0x80 == 0
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "selected locale standard identity must be unambiguous"
    );
    let file_record = index.find(&matches[0].content_key, limits, None).unwrap();
    let (file_object, file) =
        installed::read_content(&root, &index, &file_record, limits, &keys, None).unwrap();
    assert_eq!(
        file.bytes,
        fixture.bytes(&fixture.document["map"]["raw"]),
        "independent Go whole-CKey decoded Map"
    );
    stages.insert("tableDecoded", start.elapsed().as_micros());
    let layout = lycheedev_data::table::inspect(&file.bytes, Default::default()).unwrap();
    assert_eq!(layout.table_hash, table_identity.hash);
    let definition = lycheedev_data::schema::Document::parse(&dbd)
        .unwrap()
        .select(build, &format!("{:08X}", layout.layout_hash))
        .unwrap();
    let view = lycheedev_data::table::open(&file.bytes, &definition, Default::default()).unwrap();
    stages.insert("tableBound", start.elapsed().as_micros());
    let expected_rows = fixture.rows();
    let row = view
        .row(fixture.record_id().parse().unwrap(), 1 << 20)
        .unwrap();
    let all_ids = view.ids().collect::<Vec<_>>();
    let all_rows = all_ids
        .iter()
        .map(|id| view.row(*id, 1 << 20).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(&all_ids).unwrap(),
        expected_rows["ids"]
    );
    let expected_records = expected_rows["rows"].as_array().unwrap();
    assert_eq!(all_rows.len(), expected_records.len(), "Map row count");
    for (index, (actual, expected)) in all_rows.iter().zip(expected_records).enumerate() {
        fixture.assert_json_equal(
            &serde_json::to_value(actual).unwrap(),
            expected,
            &format!("Map row index {index}, ID {}", expected["ID"]),
        );
    }
    assert_eq!(
        all_rows.len() as u64,
        fixture.json(&fixture.document["map"]["schema"])["result"]["rowCount"]
            .as_u64()
            .unwrap()
    );
    let provider =
        lycheedev_data::relational::Source::from_wdc(&view, Default::default(), None).unwrap();
    let providers = BTreeMap::from([(name.to_owned(), provider)]);
    let mut queries = vec![];
    let oracle_queries = fixture.document["expected"]["installedSqlQueries"]
        .as_array()
        .unwrap();
    assert_eq!(
        oracle_queries.len(),
        7,
        "six syntax scenarios and the selected actual record query"
    );
    for query in oracle_queries {
        let sql = query["sql"].as_str().unwrap();
        let expected = fixture.json(&query["response"]);
        let result = lycheedev_data::relational::Program::compile(sql)
            .unwrap()
            .execute(&providers, &BTreeMap::new(), Default::default(), None)
            .unwrap();
        let public = serde_json::to_value(&result).unwrap();
        assert_eq!(
            public["columns"], expected["result"]["result"]["columns"],
            "{sql} columns"
        );
        fixture.assert_json_equal(
            &public["rows"],
            &expected["result"]["result"]["rows"],
            &format!("{sql} rows"),
        );
        assert_eq!(
            result.complete,
            expected["result"]["complete"].as_bool().unwrap(),
            "{sql} completeness"
        );
        queries.push(serde_json::json!({"sql":sql,"columns":result.columns,"rows":result.rows,"complete":result.complete}));
    }
    let decoded = Path::new(&output).parent().unwrap().join("Map.decoded.db2");
    std::fs::create_dir_all(decoded.parent().unwrap()).unwrap();
    std::fs::write(&decoded, &file.bytes).unwrap();
    let tables = vec![
        serde_json::json!({"identity":table_identity,"selectedRoot":matches[0],"encoding":file_record,"archive":{"index":file_object.index,"span":file_object.span},"decoded":{"bytes":file.bytes.len(),"sha256":lycheedev_core::storage::sha256(&file.bytes),"path":decoded,"wholeCKey":true},"layout":layout,"coverage":view.coverage(),"row":row,"allRows":all_rows,"queries":queries}),
    ];
    stages.insert("queriesFinished", start.elapsed().as_micros());
    let again = installed::resolve_local(&root, &product, build).unwrap();
    assert_eq!(metadata.catalog_sha256, again.catalog_sha256);
    assert_eq!(metadata.build_document.sha256, again.build_document.sha256);
    assert_eq!(metadata.cdn_document.sha256, again.cdn_document.sha256);
    let result = serde_json::json!({"schema":"lycheedev.rust-local-data-proof.v1","installation":installation,"commonRoot":root,"identity":metadata,"encoding":{"bytes":ranges.size(),"headerSHA256":encoding_object.header_sha256,"io":encoding_object.io_stats(),"verification":{"framingEKey":true,"requestedPagesMD5":true,"wholeDecodedCKey":false}},"root":{"content":root_record,"physical":{"index":object.index,"span":object.span,"encodingKey":object.encoding_key},"bytes":root_content.bytes.len(),"sha256":lycheedev_core::storage::sha256(&root_content.bytes),"wholeDecodedCKey":true},"requestedIDs":ids,"entries":entries,"tables":tables,"stagesCumulativeMicros":stages,"elapsedMs":start.elapsed().as_millis(),"complete":true,"network":false});
    let out = Path::new(&output);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(out, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}

// Go oracle (3057a966): records/installation_test.go, product_tracks_test.go,
// target_test.go and catalog_test.go. These are offline synthetic files only.
struct InstallationFixture {
    root: std::path::PathBuf,
}
impl InstallationFixture {
    fn new() -> Self {
        // Windows wall-clock ticks can be identical across parallel fixtures.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "lychee-data-installation-{}-{}-{sequence}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        // Windows TEMP may contain an 8.3 alias (RUNNER~1). Selection follows
        // Go's EvalSymlinks contract and exposes the canonical directory, so
        // fixture identity expectations must use the public canonical spelling.
        let root = std::path::PathBuf::from(lycheedev_core::client::display_path(
            &std::fs::canonicalize(&root).unwrap(),
        ));
        Self { root }
    }
    fn client(&self, name: &str, flavor: Option<&str>, build: Option<&str>) -> std::path::PathBuf {
        let path = self.root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        if let Some(flavor) = flavor {
            std::fs::write(path.join(".flavor.info"), flavor).unwrap();
        }
        if let Some(build) = build {
            std::fs::write(path.join("version.txt"), build).unwrap();
        }
        path
    }
    fn catalog(&self, rows: &[(&str, &str, bool)]) {
        let mut catalog = String::from(
            "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n",
        );
        for &(slot, build, active) in rows {
            let config = format!(
                "build-uid = {slot}\nroot = 01234567890123456789012345678901\nencoding = 01234567890123456789012345678901 abcdef01234567890123456789012345\nencoding-size = 20 30\n"
            );
            let key = md5(config.as_bytes());
            let parent = self
                .root
                .join(format!("Data/config/{}/{}", &key[..2], &key[2..4]));
            std::fs::create_dir_all(&parent).unwrap();
            std::fs::write(parent.join(&key), config).unwrap();
            catalog.push_str(&format!(
                "{slot}|{build}|{key}|{key}|{}\n",
                u8::from(active)
            ));
        }
        std::fs::write(self.root.join(".build.info"), catalog).unwrap();
    }
    fn request(&self, path: &Path, product: Option<&str>) -> installed::LocalTargetRequest {
        installed::LocalTargetRequest {
            installation: path.to_owned(),
            product: product.map(str::to_owned),
            full_build: None,
            region: "cn".into(),
            language: "zhCN".into(),
            definition_commit: "a".repeat(40),
        }
    }
}
impl Drop for InstallationFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn target_root_and_client_share_go_metadata_selection() {
    // TestInstallationRootAndClientResolveSameTitan and discovery's
    // missing-flavor/missing-version/known-folder cases.
    for (name, flavor, version, slot, build, product) in [
        (
            "renamed",
            Some("wow_classic_titan"),
            Some("3.80.2.69874"),
            "wow_classic_titan",
            "3.80.2.69874",
            "titan",
        ),
        (
            "version-only",
            None,
            Some("12.1.0.69933"),
            "wow",
            "12.1.0.69933",
            "retail",
        ),
        (
            "_classic_titan_",
            None,
            None,
            "wow_classic_titan",
            "3.80.2.69874",
            "titan",
        ),
        (
            "renamed-beta",
            Some("wow_cn_beta"),
            None,
            "wow_cn_beta",
            "1.60.1.70338",
            "forever-cn-beta",
        ),
    ] {
        let fixture = InstallationFixture::new();
        let client = fixture.client(name, flavor, version);
        if flavor.is_none() && version.is_none() {
            std::fs::write(client.join("WowClassic.exe"), []).unwrap();
        }
        fixture.catalog(&[(slot, build, true)]);
        let exact = installed::resolve_target(&fixture.request(&client, None)).unwrap();
        let root =
            installed::resolve_target(&fixture.request(&fixture.root, Some(product))).unwrap();
        assert_eq!(exact.data, root.data, "{name}");
        assert_eq!(exact.installation, client);
        assert_eq!(root.installation, client);
        assert_eq!(exact.metadata.installed.product, slot);
        let selected = installed::installation_root(&fixture.root).unwrap();
        assert_eq!(selected, (fixture.root.clone(), slot.to_owned()));
    }
}

#[test]
fn pin_reader_casc_root_precedes_client_markers_but_target_needs_client() {
    let fixture = InstallationFixture::new();
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    assert_eq!(
        installed::resolve_target(&fixture.request(&fixture.root, Some("retail")))
            .unwrap_err()
            .code,
        "selection.installation_missing"
    );
    let metadata = installed::resolve_local(&fixture.root, "wow", "12.1.0.69933").unwrap();
    let pin = lycheedev_core::selection::DataPin {
        product: "retail".into(),
        region: "cn".into(),
        full_build: "12.1.0.69933".into(),
        build_config: metadata.installed.build_config,
        cdn_config: metadata.installed.cdn_config,
        language: "zhCN".into(),
        definition_commit: "a".repeat(40),
    };
    // Go dataInstallationRoot's .build.info early return ignores these markers.
    std::fs::write(fixture.root.join(".flavor.info"), "unsupported").unwrap();
    std::fs::write(fixture.root.join("version.txt"), "3.80.2.69874").unwrap();
    let target = installed::resolve_pinned_target_budgeted(&fixture.root, &pin, None).unwrap();
    assert_eq!(target.data, pin);
    assert_eq!(target.common_root, fixture.root);
    let mut changed = pin.clone();
    changed.build_config = "b".repeat(32);
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&fixture.root, &changed, None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&fixture.root, &pin, None)
            .unwrap()
            .data,
        pin
    );
}

#[test]
fn pin_reader_preserves_canonical_family_and_exact_regional_slot() {
    let fixture = InstallationFixture::new();
    let build = "1.60.1.70338";
    let cn = fixture.client("_cn_beta_", Some("wow_cn_beta"), Some(build));
    let asia = fixture.client("_classic_beta_", Some("wow_classic_beta"), Some(build));
    fixture.catalog(&[
        ("wow_cn_beta", build, true),
        ("wow_classic_beta", build, true),
    ]);
    for (path, sibling, slot, product) in [
        (&cn, &asia, "wow_cn_beta", "forever-cn-beta"),
        (&asia, &cn, "wow_classic_beta", "forever-asia-beta"),
    ] {
        let target = installed::resolve_target(&fixture.request(path, Some(product))).unwrap();
        assert_eq!(
            installed::data_installation_root(path, "wow_forever", build).unwrap(),
            (fixture.root.clone(), slot.to_owned())
        );
        assert_eq!(
            installed::resolve_local(path, slot, build)
                .unwrap()
                .installed
                .product,
            slot
        );
        assert_eq!(
            installed::resolve_pinned_target_budgeted(path, &target.data, None)
                .unwrap()
                .data,
            target.data
        );
        assert_eq!(
            installed::resolve_pinned_target_budgeted(sibling, &target.data, None)
                .unwrap_err()
                .code,
            "records.pinned_build_changed"
        );
        assert_eq!(
            installed::data_installation_root(path, slot, "1.60.1.70000")
                .unwrap_err()
                .code,
            "records.pinned_build_changed"
        );
    }
    assert_eq!(
        installed::resolve_target(&fixture.request(&fixture.root, Some("forever")))
            .unwrap_err()
            .code,
        "selection.installation_ambiguous"
    );
    assert_eq!(
        installed::resolve_local(&fixture.root, "wow_forever", build)
            .unwrap_err()
            .code,
        "records.build_ambiguous"
    );
    fixture.catalog(&[("wow_cn_beta", build, true)]);
    assert_eq!(
        installed::resolve_local(&fixture.root, "wow_forever", build)
            .unwrap()
            .installed
            .product,
        "wow_cn_beta"
    );
    assert_eq!(
        installed::resolve_local(&fixture.root, "wow_classic_beta", build)
            .unwrap_err()
            .code,
        "records.build_unavailable"
    );
}

#[test]
fn pin_reader_conflict_inactive_and_flavor_recovery_preserve_error_layers() {
    let fixture = InstallationFixture::new();
    let client = fixture.client("renamed", None, Some("12.1.0.69933"));
    std::fs::write(client.join("flavor.info"), "launcher flavor: wow").unwrap();
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    let request = fixture.request(&client, Some("retail"));
    let pin = installed::resolve_target(&request).unwrap().data;
    assert_eq!(
        installed::resolve_target(&installed::LocalTargetRequest {
            full_build: Some("12.1.0.69934".into()),
            ..request.clone()
        })
        .unwrap_err()
        .code,
        "selection.target_build_unavailable"
    );
    for rows in [
        vec![("wow", "12.1.0.69934", true)],
        vec![("wow", "12.1.0.69933", true), ("wow", "12.1.0.69934", true)],
    ] {
        fixture.catalog(&rows);
        assert_eq!(
            installed::resolve_target(&request).unwrap_err().code,
            "selection.installation_metadata_conflict"
        );
        assert_eq!(
            installed::resolve_pinned_target_budgeted(&client, &pin, None)
                .unwrap_err()
                .code,
            "records.pinned_build_changed"
        );
    }
    fixture.catalog(&[("wow", "12.1.0.69933", false)]);
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, None)
            .unwrap_err()
            .code,
        "records.build_unavailable"
    );
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    // First nonempty flavor wins; an invalid first file cannot borrow the second.
    std::fs::write(client.join(".flavor.info"), "invalid").unwrap();
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    std::fs::write(client.join(".flavor.info"), " \r\n").unwrap();
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, None)
            .unwrap()
            .data,
        pin
    );
    // An inactive malformed catalog row is still rejected before selection.
    std::fs::write(
        fixture.root.join(".build.info"),
        "Product|Version|Build Key|CDN Key|Active\nwow|12.1.0.69933|bad|bad|0\n",
    )
    .unwrap();
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, None)
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, None)
            .unwrap()
            .data,
        pin
    );
}

#[test]
fn installation_selection_and_pinned_reads_share_cancellation_and_budget() {
    let fixture = InstallationFixture::new();
    let client = fixture.client("version-only", None, Some("12.1.0.69933"));
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    let request = fixture.request(&client, Some("retail"));
    let pin = installed::resolve_target(&request).unwrap().data;
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancelled = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(cancel),
    );
    assert_eq!(
        installed::resolve_target_budgeted(&request, Some(&cancelled))
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, Some(&cancelled))
            .unwrap_err()
            .exit_code,
        7
    );
    let tight = parser_budget(Cost {
        metadata_bytes: 1,
        ..Cost::defaults()
    });
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&fixture.root, &pin, Some(&tight))
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    let expired = Budget::new(
        Cost::defaults(),
        Instant::now() - Duration::from_secs(1),
        None,
    );
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&client, &pin, Some(&expired))
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    let budget = parser_budget(Cost::defaults());
    installed::resolve_pinned_target_budgeted(&client, &pin, Some(&budget)).unwrap();
    let first = budget.snapshot().unwrap();
    installed::resolve_pinned_target_budgeted(&client, &pin, Some(&budget)).unwrap();
    let second = budget.snapshot().unwrap();
    assert_eq!(second.metadata_bytes, first.metadata_bytes * 2);
    assert_eq!(second.decode_work, first.decode_work * 2);
}

#[test]
fn data_root_missing_and_catalog_errors_keep_go_sentinels() {
    let fixture = InstallationFixture::new();
    assert_eq!(
        installed::data_installation_root(&fixture.root.join("absent"), "wow", "12.1.0.69933")
            .unwrap_err()
            .code,
        "selection.installation_missing"
    );
    let header = "Product|Version|Build Key|CDN Key|Active\n";
    assert!(
        installed::installed_rows(header.as_bytes())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        installed::installed_rows(header.trim_end().as_bytes())
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    for row in [
        format!("wow|bad|{}|{}|0\n", "a".repeat(32), "b".repeat(32)),
        format!("wow|12.1.0.69933|bad|{}|0\n", "b".repeat(32)),
    ] {
        std::fs::write(fixture.root.join(".build.info"), format!("{header}{row}")).unwrap();
        assert_eq!(
            installed::resolve_local(&fixture.root, "wow", "12.1.0.69933")
                .unwrap_err()
                .code,
            "records.invalid_metadata"
        );
    }
    fixture.catalog(&[("wow_classic_beta", "5.5.4.69934", true)]);
    let metadata =
        installed::resolve_local(&fixture.root, "wow_classic_beta", "5.5.4.69934").unwrap();
    let pin = lycheedev_core::selection::DataPin {
        product: "forever-asia-beta".into(),
        full_build: "5.5.4.69934".into(),
        build_config: metadata.installed.build_config,
        cdn_config: metadata.installed.cdn_config,
        region: "cn".into(),
        language: "zhCN".into(),
        definition_commit: "a".repeat(40),
    };
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&fixture.root, &pin, None)
            .unwrap_err()
            .code,
        "selection.data_product_build_mismatch"
    );
}

#[test]
fn installation_root_many_directories_uses_temporary_memory_not_metadata_bytes() {
    let fixture = InstallationFixture::new();
    let client = fixture.client("selected", Some("wow"), Some("12.1.0.69933"));
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    for index in 0..64 {
        std::fs::create_dir(fixture.root.join(format!("ordinary-{index}"))).unwrap();
    }
    let request = fixture.request(&fixture.root, Some("retail"));
    let budget = parser_budget(Cost::defaults());
    let selected = installed::resolve_target_budgeted(&request, Some(&budget)).unwrap();
    assert_eq!(selected.installation, client);
    let used = budget.snapshot().unwrap();
    assert!(
        used.metadata_bytes < 8 << 20,
        "path capacity is not metadata read: {used:?}"
    );
    assert!(
        used.retained_bytes < 8 << 20,
        "inspection scratch must be released: {used:?}"
    );
    let tight = parser_budget(Cost {
        retained_bytes: 1,
        ..Cost::defaults()
    });
    assert_eq!(
        installed::resolve_target_budgeted(&request, Some(&tight))
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    std::fs::write(fixture.root.join(".build.info"), vec![b' '; (1 << 20) + 1]).unwrap();
    assert_eq!(
        installed::resolve_target_budgeted(&request, Some(&parser_budget(Cost::defaults())))
            .unwrap_err()
            .code,
        "records.metadata_limit"
    );
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancelled = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(cancel),
    );
    assert_eq!(
        installed::resolve_target_budgeted(&request, Some(&cancelled))
            .unwrap_err()
            .exit_code,
        7
    );
}

#[test]
fn catalog_branch_is_internal_and_does_not_change_go_admission_or_wire_shape() {
    let fixture = InstallationFixture::new();
    let client = fixture.client("selected", Some("wow"), Some("12.1.0.69933"));
    fixture.catalog(&[("wow", "12.1.0.69933", true)]);
    let original = std::fs::read_to_string(fixture.root.join(".build.info")).unwrap();
    for branch in ["cn beta".to_owned(), "1.2".to_owned(), "分支".repeat(80)] {
        let with_branch = original
            .lines()
            .enumerate()
            .map(|(index, line)| {
                format!(
                    "{line}|{}\n",
                    if index == 0 {
                        "Branch!STRING:0"
                    } else {
                        &branch
                    }
                )
            })
            .collect::<String>();
        std::fs::write(fixture.root.join(".build.info"), with_branch).unwrap();
        let result = installed::resolve_target(&fixture.request(&client, Some("retail"))).unwrap();
        assert_eq!(result.metadata.installed.metadata_branch, branch);
        let value = serde_json::to_value(&result.metadata.installed).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 5);
        for key in ["product", "fullBuild", "buildConfig", "cdnConfig", "active"] {
            assert!(object.contains_key(key));
        }
        for key in ["metadataBranch", "channel", "displayName"] {
            assert!(!object.contains_key(key));
        }
    }
    let key = "a".repeat(32);
    let inactive = format!(
        "Product|Version|Build Key|CDN Key|Active|Branch\nunknown|1.2.3.4|{key}|{key}|0|cn beta\n"
    );
    assert_eq!(
        installed::installed_rows(inactive.as_bytes()).unwrap()[0].metadata_branch,
        "cn beta"
    );
}

#[test]
fn configuration_validation_precedes_integrity_and_preserves_go_size_bounds() {
    for raw in [vec![0xff], vec![0], vec![b'a'; (4 << 20) + 1]] {
        let expected = if raw.len() > 4 << 20 {
            "records.metadata_limit"
        } else {
            "records.invalid_metadata"
        };
        for key in ["bad".to_owned(), "a".repeat(32), md5(&raw)] {
            assert_eq!(
                installed::parse_configuration(raw.clone(), &key)
                    .unwrap_err()
                    .code,
                expected
            );
        }
    }
    let raw = b"valid = text\n".to_vec();
    assert_eq!(
        installed::parse_configuration(raw, &"a".repeat(32))
            .unwrap_err()
            .code,
        "records.configuration_integrity"
    );
    let fixture = InstallationFixture::new();
    for size in [(1u64 << 40) + 1, 1 << 50, (1 << 50) + 1, 0] {
        let config = format!(
            "root = {}\nencoding = {} {}\nencoding-size = {size} {size}\n",
            "a".repeat(32),
            "b".repeat(32),
            "c".repeat(32)
        );
        let key = md5(config.as_bytes());
        let parent = fixture
            .root
            .join(format!("Data/config/{}/{}", &key[..2], &key[2..4]));
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(parent.join(&key), config).unwrap();
        std::fs::write(
            fixture.root.join(".build.info"),
            format!("Product|Version|Build Key|CDN Key|Active\nwow|12.1.0.69933|{key}|{key}|1\n"),
        )
        .unwrap();
        let result = installed::resolve_local(&fixture.root, "wow", "12.1.0.69933");
        if size > 0 && size <= 1 << 50 {
            let metadata = result.unwrap();
            assert_eq!(metadata.content_bytes as u64, size);
            assert_eq!(metadata.encoding_bytes as u64, size);
        } else {
            assert_eq!(result.unwrap_err().code, "records.invalid_metadata");
        }
    }
}
