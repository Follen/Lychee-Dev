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
        "records.metadata_invalid"
    );
    assert_eq!(
        installed::parse_configuration_budgeted(
            invalid_text.clone(),
            &"a".repeat(32),
            Some(&budget)
        )
        .unwrap_err()
        .code,
        "records.configuration_integrity"
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
        installed::installed_rows_budgeted(too_many_headers.as_bytes(), Some(&budget))
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
    assert_eq!(
        installed::resolve_target(&legacy_cn).unwrap_err().code,
        "records.client_identity",
        "an old CN selection named forever is not silently reinterpreted as Asia"
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
        "records.build_unavailable"
    );
    std::fs::write(
        root.join(".build.info"),
        format!("{header}{}", row("wow_cn_beta", "5.5.4.69934")),
    )
    .unwrap();
    assert_eq!(
        installed::resolve_target(&request).unwrap_err().code,
        "records.client_identity"
    );
    std::fs::write(
        root.join(".build.info"),
        format!("{header}{}", row("wow_cn_beta", "1.60.1.70338")),
    )
    .unwrap();
    std::fs::write(client.join("version.txt"), "1.60.1.70124").unwrap();
    assert_eq!(
        installed::resolve_target(&request).unwrap_err().code,
        "records.client_identity"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires explicitly named read-only actual client and report directory"]
fn real_local_authenticated_encoding_root() {
    let installation = std::env::var("LYCHEEDEV_RUST_DATA_CLIENT").expect("explicit client path");
    let build = std::env::var("LYCHEEDEV_RUST_DATA_BUILD").expect("exact build");
    let output = std::env::var("LYCHEEDEV_RUST_DATA_REPORT").expect("task-owned report path");
    let ids = std::env::var("LYCHEEDEV_RUST_DATA_IDS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().unwrap())
        .collect::<Vec<_>>();
    let start = Instant::now();
    let mut stages = BTreeMap::new();
    let (root, product) = installed::installation_root(Path::new(&installation)).unwrap();
    let metadata = installed::resolve_local(&root, &product, &build).unwrap();
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
    let entries = if ids.is_empty() {
        vec![]
    } else {
        metadata_format::lookup_root(
            &root_content,
            &ids,
            RootLimits {
                operation: limits,
                ..Default::default()
            },
            None,
        )
        .unwrap()
    };
    stages.insert("rootLookup", start.elapsed().as_micros());
    let definition_dir = std::env::var("LYCHEEDEV_RUST_DATA_DEFINITIONS").ok();
    let mut tables = vec![];
    if let Some(directory) = definition_dir {
        let directory = Path::new(&directory);
        let identity: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("input-identity.json")).unwrap())
                .unwrap();
        let manifest = std::fs::read(directory.join("manifest.json")).unwrap();
        assert_eq!(
            lycheedev_core::storage::sha256(&manifest),
            identity["manifest"]["sha256"]
        );
        let table_identity = &identity["identity"];
        let id = table_identity["db2FileDataID"].as_u64().unwrap() as u32;
        let name = table_identity["tableName"].as_str().unwrap();
        let dbd = std::fs::read(directory.join(format!("{name}.dbd"))).unwrap();
        assert_eq!(
            lycheedev_core::storage::sha256(&dbd),
            identity["definition"]["sha256"]
        );
        let matches = entries
            .iter()
            .filter(|entry| {
                entry.file_data_id == id
                    && entry.locale_mask & 0x40 != 0
                    && entry.content_flags & 0x80 == 0
            })
            .collect::<Vec<_>>();
        assert_eq!(
            matches.len(),
            1,
            "explicit zhCN standard identity must be unambiguous"
        );
        let file_record = index.find(&matches[0].content_key, limits, None).unwrap();
        let (file_object, file) =
            installed::read_content(&root, &index, &file_record, limits, &keys, None).unwrap();
        stages.insert("tableDecoded", start.elapsed().as_micros());
        let layout = lycheedev_data::table::inspect(&file.bytes, Default::default()).unwrap();
        assert_eq!(
            format!("{:08X}", layout.table_hash),
            table_identity["tableHash"]
        );
        let definition = lycheedev_data::schema::Document::parse(&dbd)
            .unwrap()
            .select(&build, &format!("{:08X}", layout.layout_hash))
            .unwrap();
        let view =
            lycheedev_data::table::open(&file.bytes, &definition, Default::default()).unwrap();
        stages.insert("tableBound", start.elapsed().as_micros());
        let row = view.row(0, 1 << 20).unwrap();
        let all_rows = view
            .ids()
            .map(|id| view.row(id, 1 << 20).unwrap())
            .collect::<Vec<_>>();
        let source =
            lycheedev_data::relational::Source::from_wdc(&view, Default::default(), None).unwrap();
        let sql = format!("SELECT ID, Directory, MapName_lang FROM {name} WHERE ID=0");
        let sql_result = lycheedev_data::relational::Program::compile(&sql)
            .unwrap()
            .execute(
                &BTreeMap::from([(name.into(), source)]),
                &BTreeMap::new(),
                Default::default(),
                None,
            )
            .unwrap();
        let mut queries = vec![];
        let provider =
            lycheedev_data::relational::Source::from_wdc(&view, Default::default(), None).unwrap();
        let providers = BTreeMap::from([(name.to_owned(), provider)]);
        for sql in [
            "SELECT ID FROM Map WHERE ID>=0 ORDER BY ID",
            "SELECT ID, Directory FROM Map WHERE ID BETWEEN 0 AND 20 ORDER BY ID DESC LIMIT 5 OFFSET 1",
            "SELECT ID, \"Flags[0]\" FROM Map WHERE ID=0",
            "SELECT ID+1 AS NextID FROM Map WHERE ID IN (0,1,2) ORDER BY NextID DESC",
            "SELECT DISTINCT InstanceType FROM Map ORDER BY InstanceType",
            "SELECT ID, MapName_lang FROM Map WHERE ID=0 AND LENGTH(MapName_lang)>0",
        ] {
            let result = lycheedev_data::relational::Program::compile(sql)
                .unwrap()
                .execute(&providers, &BTreeMap::new(), Default::default(), None)
                .unwrap();
            queries.push(serde_json::json!({"sql":sql,"columns":result.columns,"rows":result.rows,"complete":result.complete}));
        }
        let decoded = directory.join(format!("{name}.decoded.db2"));
        std::fs::write(&decoded, &file.bytes).unwrap();
        tables.push(serde_json::json!({"identity":identity,"selectedRoot":matches[0],"encoding":file_record,"archive":{"index":file_object.index,"span":file_object.span},"decoded":{"bytes":file.bytes.len(),"sha256":lycheedev_core::storage::sha256(&file.bytes),"path":decoded,"wholeCKey":true},"layout":layout,"coverage":view.coverage(),"row":row,"allRows":all_rows,"queries":queries,"sql":sql,"sqlResult":sql_result}));
    }
    stages.insert("queriesFinished", start.elapsed().as_micros());
    let again = installed::resolve_local(&root, &product, &build).unwrap();
    assert_eq!(metadata.catalog_sha256, again.catalog_sha256);
    assert_eq!(metadata.build_document.sha256, again.build_document.sha256);
    assert_eq!(metadata.cdn_document.sha256, again.cdn_document.sha256);
    let result = serde_json::json!({"schema":"lycheedev.rust-local-data-proof.v1","installation":installation,"commonRoot":root,"identity":metadata,"encoding":{"bytes":ranges.size(),"headerSHA256":encoding_object.header_sha256,"io":encoding_object.io_stats(),"verification":{"framingEKey":true,"requestedPagesMD5":true,"wholeDecodedCKey":false}},"root":{"content":root_record,"physical":{"index":object.index,"span":object.span,"encodingKey":object.encoding_key},"bytes":root_content.bytes.len(),"sha256":lycheedev_core::storage::sha256(&root_content.bytes),"wholeDecodedCKey":true},"requestedIDs":ids,"entries":entries,"tables":tables,"stagesCumulativeMicros":stages,"elapsedMs":start.elapsed().as_millis(),"complete":true,"network":false});
    let out = Path::new(&output);
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(out, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
}
