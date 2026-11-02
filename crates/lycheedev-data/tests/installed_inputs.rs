#[path = "support/current_local_data.rs"]
mod current_local_data;
use lycheedev_data::{
    blte, casc, installed,
    resource::{Budget, Cost},
};
use std::{
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
    assert_eq!(actual.key, key);
    assert_eq!(actual.raw, raw);
    assert_eq!(actual.fields["dense"], vec!["x"; 4096]);
    assert!(budget.snapshot().unwrap().metadata_bytes >= (4096 * size_of::<String>()) as u64);

    let raw = format!(
        "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.0.69933|{key}|{key}|1\n"
    );
    let budget = parser_budget(Cost::defaults());
    let actual = installed::installed_rows_budgeted(raw.as_bytes(), Some(&budget)).unwrap();
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].product, "wow");
    assert_eq!(actual[0].full_build, "12.1.0.69933");
    assert_eq!(actual[0].build_config, key);
    assert_eq!(actual[0].cdn_config, key);
    assert!(actual[0].active);
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
    let rows =
        installed::installed_rows_budgeted(format!("{header}{row}").as_bytes(), None).unwrap();
    assert_eq!(rows[0].product, "wow");
    assert!(rows[0].active);
    assert!(
        installed::installed_rows_budgeted(format!("{header}{row}x|1|2\n").as_bytes(), None)
            .is_err()
    );
    assert!(
        installed::installed_rows_budgeted(header.replace("Product!", "Version!").as_bytes(), None)
            .is_err()
    );
    let raw =
        b"# config\nroot = 01234567890123456789012345678901\nencoding-size = 20 30\n".to_vec();
    let doc = installed::parse_configuration_budgeted(raw.clone(), &md5(&raw), None).unwrap();
    assert_eq!(doc.fields["encoding-size"], vec!["20", "30"]);
    let mut changed = raw.clone();
    changed.push(b' ');
    assert_eq!(
        installed::parse_configuration_budgeted(changed, &md5(&raw), None)
            .unwrap_err()
            .code,
        "records.configuration_integrity"
    );
    let duplicate = b"root = a\nroot = b\n".to_vec();
    assert_eq!(
        installed::parse_configuration_budgeted(duplicate.clone(), &md5(&duplicate), None)
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
fn formal_forever_target_preserves_wowf_identity_and_rejects_retired_slots() {
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
        b"Product Flavor!STRING:0\nwowf\n",
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
            if product == "wowf" { "cn" } else { "us" }
        )
    };
    std::fs::write(
        root.join(".build.info"),
        format!(
            "{header}{}{}",
            row("wowf", "1.60.1.70658"),
            row("wow_classic_beta", "1.60.1.70658")
        ),
    )
    .unwrap();
    let request = installed::LocalTargetRequest {
        installation: client.clone(),
        product: Some("forever".into()),
        full_build: None,
        region: "cn".into(),
        language: "zhCN".into(),
        definition_commit: "a".repeat(40),
    };
    let resolved = installed::resolve_target(&request).unwrap();
    assert_eq!(resolved.data.product, "forever");
    assert_eq!(resolved.data.full_build, "1.60.1.70658");
    assert_eq!(resolved.metadata.installed.product, "wowf");
    assert_eq!(resolved.metadata.installed.metadata_branch, "cn");
    assert!(resolved.metadata.installed.channel.is_empty());
    assert!(resolved.metadata.installed.display_name.is_empty());
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
    control.tracked_cancel_handle().unwrap().cancel();
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
    assert!(!client.join("version.txt").exists());
    let regional = installed::resolve_target(&installed::LocalTargetRequest {
        region: "tw".into(),
        ..request.clone()
    })
    .unwrap();
    assert_eq!(regional.data.product, "forever");
    assert_eq!(
        regional.data.region, "tw",
        "publishing branch never replaces the requested region"
    );
    assert_eq!(regional.metadata.installed.metadata_branch, "cn");
    let common = installed::resolve_target(&installed::LocalTargetRequest {
        installation: root.clone(),
        ..request.clone()
    })
    .unwrap();
    assert_eq!(common.data, resolved.data);
    for product in ["forever-asia-beta", "forever-cn-beta"] {
        assert_eq!(
            installed::resolve_target(&installed::LocalTargetRequest {
                product: Some(product.into()),
                ..request.clone()
            })
            .unwrap_err()
            .code,
            "selection.unsupported_data_product"
        );
    }
    std::fs::write(
        root.join(".build.info"),
        format!(
            "{header}{}{}",
            row("wowf", "1.60.1.70658"),
            row("wowf", "1.60.1.70658")
        ),
    )
    .unwrap();
    assert_eq!(
        installed::resolve_target(&request).unwrap_err().code,
        "records.build_ambiguous"
    );
    assert_eq!(
        installed::resolve_target(&installed::LocalTargetRequest {
            full_build: Some("1.60.1.70658".into()),
            ..request.clone()
        })
        .unwrap_err()
        .code,
        "records.build_ambiguous"
    );
    for rows in [
        row("wow_classic_beta", "1.60.1.70658"),
        row("wow_cn_beta", "1.60.1.70658"),
        row("wowf", "5.5.4.69934"),
    ] {
        std::fs::write(root.join(".build.info"), format!("{header}{rows}")).unwrap();
        assert_eq!(
            installed::resolve_target(&request).unwrap_err().code,
            "selection.installation_metadata_conflict"
        );
    }
    std::fs::write(
        root.join(".build.info"),
        format!("{header}{}", row("wowf", "1.60.1.70658")),
    )
    .unwrap();
    std::fs::write(client.join("version.txt"), "1.60.1.70707").unwrap();
    let conflict = installed::resolve_target(&request).unwrap_err();
    assert_eq!(conflict.code, "selection.installation_metadata_conflict");
    let issue = &conflict.details["__commandContext"]["installationSelection"]["issues"][0];
    assert_eq!(issue["productCode"], "wowf");
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
fn target_root_and_client_share_metadata_selection() {
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
            "renamed-formal",
            Some("wowf"),
            None,
            "wowf",
            "1.60.1.70658",
            "forever",
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
    let metadata =
        installed::resolve_local_budgeted(&fixture.root, "wow", "12.1.0.69933", None).unwrap();
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
fn formal_pin_reader_never_borrows_retired_slots_or_relabels_legacy_configs() {
    let fixture = InstallationFixture::new();
    let build = "1.60.1.70658";
    let formal = fixture.client("_forever_", Some("wowf"), None);
    let sibling = fixture.client("_retail_", Some("wow"), Some("12.1.0.69933"));
    fixture.catalog(&[
        ("wowf", build, true),
        ("wow", "12.1.0.69933", true),
        ("wow_cn_beta", build, true),
    ]);
    let target = installed::resolve_target(&fixture.request(&formal, Some("forever"))).unwrap();
    assert_eq!(target.metadata.installed.product, "wowf");
    assert_eq!(
        installed::data_installation_root_budgeted(&formal, "wowf", build, None).unwrap(),
        (fixture.root.clone(), "wowf".into())
    );
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&formal, &target.data, None)
            .unwrap()
            .data,
        target.data
    );
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&sibling, &target.data, None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    assert_eq!(
        installed::data_installation_root_budgeted(&formal, "wow_forever", build, None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    assert_eq!(
        installed::data_installation_root_budgeted(&formal, "wowf", "1.60.1.70707", None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    assert_eq!(
        installed::resolve_target(&fixture.request(&fixture.root, Some("forever")))
            .unwrap()
            .data,
        target.data
    );
    assert_eq!(
        installed::resolve_local_budgeted(&fixture.root, "wow_forever", build, None)
            .unwrap_err()
            .code,
        "records.build_unavailable"
    );
    let retired =
        installed::resolve_local_budgeted(&fixture.root, "wow_cn_beta", build, None).unwrap();
    let legacy = lycheedev_core::selection::DataPin {
        build_config: retired.installed.build_config,
        cdn_config: retired.installed.cdn_config,
        ..target.data.clone()
    };
    let original = legacy.clone();
    assert_eq!(
        installed::resolve_pinned_target_budgeted(&fixture.root, &legacy, None)
            .unwrap_err()
            .code,
        "records.pinned_build_changed"
    );
    assert_eq!(
        legacy, original,
        "legacy family-wide pins keep their original config identity"
    );
    for product in ["forever-asia-beta", "forever-cn-beta"] {
        let pin = lycheedev_core::selection::DataPin {
            product: product.into(),
            ..legacy.clone()
        };
        assert_eq!(
            installed::resolve_pinned_target_budgeted(&fixture.root, &pin, None)
                .unwrap_err()
                .code,
            "selection.unsupported_data_product"
        );
    }
    fixture.catalog(&[("wow_cn_beta", build, true)]);
    assert_eq!(
        installed::resolve_local_budgeted(&fixture.root, "wowf", build, None)
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
    let cancel = lycheedev_core::control::TrackedCancellation::new();
    cancel.cancel();
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
fn data_root_missing_and_catalog_errors_keep_wire_sentinels() {
    let fixture = InstallationFixture::new();
    assert_eq!(
        installed::data_installation_root_budgeted(
            &fixture.root.join("absent"),
            "wow",
            "12.1.0.69933",
            None
        )
        .unwrap_err()
        .code,
        "selection.installation_missing"
    );
    let header = "Product|Version|Build Key|CDN Key|Active\n";
    assert!(
        installed::installed_rows_budgeted(header.as_bytes(), None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        installed::installed_rows_budgeted(header.trim_end().as_bytes(), None)
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    for row in [
        format!("wow|bad|{}|{}|0\n", "a".repeat(32), "b".repeat(32)),
        format!("wow|12.1.0.69933|bad|{}|0\n", "b".repeat(32)),
    ] {
        std::fs::write(fixture.root.join(".build.info"), format!("{header}{row}")).unwrap();
        let error = installed::resolve_local_budgeted(&fixture.root, "wow", "12.1.0.69933", None)
            .unwrap_err();
        assert_eq!(error.code, "records.invalid_metadata");
        assert_eq!(error.exit_code, 4);
        assert_eq!(
            error.details["sourcePath"],
            serde_json::json!(fixture.root.join(".build.info"))
        );
        assert!(
            error.details["formatHint"]
                .as_str()
                .unwrap()
                .contains("Pipe-delimited")
        );
    }
    fixture.catalog(&[("wow_classic_beta", "5.5.4.69934", true)]);
    let metadata =
        installed::resolve_local_budgeted(&fixture.root, "wow_classic_beta", "5.5.4.69934", None)
            .unwrap();
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
        "selection.unsupported_data_product"
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
    let cancel = lycheedev_core::control::TrackedCancellation::new();
    cancel.cancel();
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
fn catalog_branch_is_internal_and_does_not_change_admission_or_wire_shape() {
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
        installed::installed_rows_budgeted(inactive.as_bytes(), None).unwrap()[0].metadata_branch,
        "cn beta"
    );
}

#[test]
fn configuration_validation_precedes_integrity_and_preserves_wire_size_bounds() {
    for raw in [vec![0xff], vec![0], vec![b'a'; (4 << 20) + 1]] {
        let expected = if raw.len() > 4 << 20 {
            "records.metadata_limit"
        } else {
            "records.invalid_metadata"
        };
        for key in ["bad".to_owned(), "a".repeat(32), md5(&raw)] {
            assert_eq!(
                installed::parse_configuration_budgeted(raw.clone(), &key, None)
                    .unwrap_err()
                    .code,
                expected
            );
        }
    }
    let raw = b"valid = text\n".to_vec();
    assert_eq!(
        installed::parse_configuration_budgeted(raw, &"a".repeat(32), None)
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
        let result = installed::resolve_local_budgeted(&fixture.root, "wow", "12.1.0.69933", None);
        if size > 0 && size <= 1 << 50 {
            let metadata = result.unwrap();
            assert_eq!(metadata.content_bytes as u64, size);
            assert_eq!(metadata.encoding_bytes as u64, size);
        } else {
            assert_eq!(result.unwrap_err().code, "records.invalid_metadata");
        }
    }
}
