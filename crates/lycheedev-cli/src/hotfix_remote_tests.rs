//! Actual admitted argv fixtures for fixed Go3.2.1 Hotfix (offline, no network).
use super::*;
use lycheedev_core::{
    selection::{self, DataPin, SelectionSpec},
    workspace,
};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    file: PathBuf,
    snapshot: String,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-hotfix-cli-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let file = root.join("DBCache.bin");
        let mut raw = b"XFTH".to_vec();
        raw.extend(9u32.to_le_bytes());
        raw.extend(68974u32.to_le_bytes());
        raw.resize(44, 0);
        for (id, table) in [(1u32, 42u32), (2, 42), (2, 43)] {
            raw.extend(b"XFTH");
            raw.extend(71u32.to_le_bytes());
            raw.extend(10i32.to_le_bytes());
            raw.extend(id.to_le_bytes());
            raw.extend(table.to_le_bytes());
            raw.extend(id.to_le_bytes());
            raw.extend(0u32.to_le_bytes());
            raw.extend([1, 0, 0, 0]);
        }
        fs::write(&file, raw).unwrap();
        let snapshot = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "us".into(),
                    language: "enUS".into(),
                    full_build: "12.0.7.68974".into(),
                    build_config: "b".repeat(32),
                    cdn_config: "c".repeat(32),
                    definition_commit: "a".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        Self {
            root,
            home,
            file,
            snapshot,
        }
    }
    fn argv(&self, source: &str, scan: bool) -> Vec<String> {
        let mut a = words(&[
            "data",
            "hotfix",
            "--source",
            source,
            "--home",
            self.home.to_str().unwrap(),
            "--format",
            "json",
            "--record",
            "2",
            "--table-hash",
            "0000002A",
        ]);
        if source == "dbcache" {
            a.extend(words(&[
                "--snapshot",
                &self.snapshot,
                "--dbcache",
                self.file.to_str().unwrap(),
            ]));
            if scan {
                a.push("--scan".into());
            }
        } else {
            a.extend(words(&[
                "--raidbots",
                self.file.to_str().unwrap(),
                "--product",
                "retail",
                "--build",
                "12.0.7.68974",
                "--region",
                "us",
                "--locale",
                "enUS",
            ]));
        }
        a
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}

#[test]
fn admitted_raidbots_record_and_hex_hash_reach_typed_public_output() {
    let f = Fixture::new();
    let inv = parse(&f.argv("raidbots", false)).unwrap();
    let mut bytes = Vec::new();
    lycheedev_data::hotfix::remote::render(&inv, None, &mut bytes).unwrap();
    let public: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(public["result"]["records"].as_array().unwrap().len(), 1);
    assert_eq!(public["result"]["records"][0]["recordId"], 2);
    assert_eq!(public["result"]["records"][0]["tableHash"], 42);
    let rejected = parse(&words(&["data", "hotfix", "--record-id", "2"])).unwrap_err();
    assert_eq!(rejected.code, "command.invalid_arguments");
}
#[test]
fn admitted_dbcache_local_and_scan_use_eight_digit_hex_without_decimal_regression() {
    let f = Fixture::new();
    for scan in [false, true] {
        let inv = parse(&f.argv("dbcache", scan)).unwrap();
        let output = lycheedev_data::dispatch(&inv).unwrap();
        let value = if scan {
            &output.data["result"]["query"]["Filter"]
        } else {
            &output.data["page"]["filter"]
        };
        assert_eq!(value["tableHash"], 42);
        assert_eq!(value["recordId"], 2);
        if !scan {
            assert_eq!(output.data["page"]["entries"].as_array().unwrap().len(), 1);
        }
    }
}
#[test]
fn admitted_hotfix_values_fail_before_any_workspace_io_for_every_source() {
    let home = std::env::temp_dir().join(format!(
        "lychee-hotfix-no-io-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for source in ["dbcache", "wago", "raidbots"] {
        for (flag, values) in [
            ("--max-pages", &["0", "-1", "2049", "x"][..]),
            (
                "--max-requests",
                &["0", "-1", "9223372036854775808", "x"][..],
            ),
            ("--max-bytes", &["0", "-1", "536870913", "x"][..]),
            ("--table-hash", &["42", "0x00002A", "GGGGGGGG"][..]),
            ("--record", &["-1", "+2", "4294967296", "x"][..]),
        ] {
            for value in values {
                let inv = parse(&words(&[
                    "data",
                    "hotfix",
                    "--source",
                    source,
                    "--home",
                    home.to_str().unwrap(),
                    flag,
                    value,
                ]));
                assert_eq!(inv.unwrap_err().exit_code, 2, "{source} {flag} {value}");
                assert!(!home.exists());
            }
        }
    }
}
#[test]
fn hotfix_clap_duplicates_precede_admitted_value_checks() {
    // Once the command shape is admitted, numeric validation still follows
    // argv order and retains its specific business diagnostic.
    for (tail, message) in [
        (vec!["--max-pages", "0", "--table-hash", "bad"], "--max-pages must be between 1 and 2048"),
        (vec!["--table-hash", "bad", "--max-pages", "0"], "--table-hash requires eight hexadecimal digits"),
    ] {
        let mut argv = words(&["data", "hotfix", "--source", "dbcache"]);
        argv.extend(words(&tail));
        assert_eq!(parse(&argv).unwrap_err().message, message);
    }
    for source in ["dbcache", "wago", "raidbots"] {
        for scan in [false, true] {
            let mut a = words(&["data", "hotfix", "--source", source]);
            if scan {
                a.push("--scan".into());
            }
            a.extend(words(&["--table-hash", "bad", "--table-hash", "0000002A"]));
            let error = parse(&a).unwrap_err();
            assert_hotfix_clap_error(&error, "ArgumentConflict", "--table-hash");
            a.truncate(a.len() - 2);
            assert_eq!(parse(&a).unwrap_err().message, "--table-hash requires eight hexadecimal digits");
            let mut a = words(&[
                "data",
                "hotfix",
                "--source",
                source,
                "--max-pages",
                "0",
                "--max-pages",
                "1",
            ]);
            assert_hotfix_clap_error(&parse(&a).unwrap_err(), "ArgumentConflict", "--max-pages");
            a[5] = "1".into();
            assert_hotfix_clap_error(&parse(&a).unwrap_err(), "ArgumentConflict", "--max-pages");
            a[5] = "0".into();
            a.truncate(a.len() - 2);
            assert_eq!(parse(&a).unwrap_err().message, "--max-pages must be between 1 and 2048");
        }
    }
}
#[test]
fn admitted_decimal_looking_hash_is_hex_and_maximum_budgets_are_accepted() {
    let inv = parse(&words(&[
        "data",
        "hotfix",
        "--source",
        "wago",
        "--product",
        "retail",
        "--build",
        "12.0.7.68974",
        "--region",
        "us",
        "--locale",
        "enUS",
        "--table-hash",
        "00000042",
        "--max-pages",
        "2048",
        "--max-requests",
        "9223372036854775807",
        "--max-bytes",
        "536870912",
    ]))
    .unwrap();
    assert_eq!(inv.flag("--max-bytes"), Some("536870912"));
    let f = Fixture::new();
    let mut args = f.argv("dbcache", false);
    let at = args.iter().position(|a| a == "--table-hash").unwrap();
    args[at + 1] = "00000042".into();
    let output = lycheedev_data::dispatch(&parse(&args).unwrap()).unwrap();
    assert_eq!(output.data["page"]["filter"]["tableHash"], 0x42);
    assert!(
        output.data["page"]["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn clap_source_and_encoding_fail_in_argv_order_without_io() {
    let home = std::env::temp_dir().join(format!(
        "lychee-hotfix-enum-no-io-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (tail, flag) in [
        (
            vec!["--source", "bogus", "--max-pages", "0"],
            "--source",
        ),
        (
            vec!["--source", "bogus", "--source", "wago"],
            "--source",
        ),
        (
            vec!["--encoding", "bogus", "--source", "bogus"],
            "--encoding",
        ),
        (
            vec!["--source", "bogus", "--encoding", "bogus"],
            "--source",
        ),
        (
            vec!["--encoding", "bogus", "--encoding", "csv"],
            "--encoding",
        ),
    ] {
        let mut a = words(&["data", "hotfix", "--home", home.to_str().unwrap()]);
        a.extend(words(&tail));
        let error = parse(&a).unwrap_err();
        assert_hotfix_clap_error(&error, "InvalidValue", flag);
        assert!(error.details["diagnostic"].as_str().unwrap().contains("bogus"));
        assert!(!home.exists());
    }
    for source in ["dbcache", "wago", "raidbots"] {
        // The formal Hotfix command advertises CSV; raw/png/webp belong to
        // asset export and are not accepted by this command's clap enum.
        assert!(parse(&words(&["data", "hotfix", "--source", source, "--encoding", "csv"])).is_ok());
        for encoding in ["raw", "png", "webp"] {
            let error = parse(&words(&[
                    "data",
                    "hotfix",
                    "--source",
                    source,
                    "--encoding",
                    encoding
                ])).unwrap_err();
            assert_hotfix_clap_error(&error, "InvalidValue", "--encoding");
            assert!(error.details["diagnostic"].as_str().unwrap().contains(encoding));
        }
        for encoding in ["json", "CSV", "bogus"] {
            let error = parse(&words(&[
                    "data",
                    "hotfix",
                    "--source",
                    source,
                    "--encoding",
                    encoding
                ])).unwrap_err();
            assert_hotfix_clap_error(&error, "InvalidValue", "--encoding");
            assert!(error.details["diagnostic"].as_str().unwrap().contains(encoding));
        }
    }
}

fn assert_hotfix_clap_error(error: &ToolkitError, kind: &str, flag: &str) {
    assert_eq!(error.code, "command.invalid_arguments");
    assert_eq!(error.exit_code, 2);
    assert_eq!(error.details["parser"], "clap");
    assert_eq!(error.details["kind"], kind);
    assert!(error.details["diagnostic"].as_str().unwrap().contains(flag), "{error:?}");
}

#[test]
fn admitted_wago_cli_default_and_explicit_byte_budgets_reach_coverage() {
    use lycheedev_core::{evidence, metadata::DataCacheMetadata, storage};
    let f = Fixture::new();
    let base = words(&[
        "data",
        "hotfix",
        "--source",
        "wago",
        "--home",
        f.home.to_str().unwrap(),
        "--product",
        "retail",
        "--build",
        "12.0.7.68974",
        "--region",
        "us",
        "--locale",
        "enUS",
        "--offline",
        "--format",
        "json",
    ]);
    let inv = parse(&base).unwrap();
    // Go 3.2.2 hotfix_wago.go:535-546 narrows an otherwise unfiltered
    // request by build number. The receipt key and request URL must agree.
    let identity_bytes = br#"{"provider":"wago","parser":"lycheedev-wago-inertia-v1","product":"retail","fullBuild":"12.0.7.68974","region":"us","locale":"enUS","search":"68974","page":1}"#;
    let identity: Value = serde_json::from_slice(identity_bytes).unwrap();
    let key = format!("hotfix-wago-page/{}", storage::sha256(identity_bytes));
    let page = br#"<html><div id="app" data-page='{"component":"Hotfixes","props":{"hotfixes":{"current_page":1,"last_page":1,"per_page":25,"total":0,"data":[],"next_page_url":null}}}'></div></html>"#;
    let blob = evidence::publish_blob(&f.home, page, 16 << 20).unwrap();
    let receipt = json!({"schema":"lycheedev.wago-page.v1","identity":identity,"requestUrl":"https://wago.tools/hotfixes?page=1&search=68974","response":blob,"capturedAt":"2026-08-05T22:09:07Z","currentPage":1,"lastPage":1,"perPage":25,"total":0});

    // Preserve the original failing seed as a negative case: another scope
    // must miss offline even when its response bytes are present and valid.
    let unscoped_bytes = br#"{"provider":"wago","parser":"lycheedev-wago-inertia-v1","product":"retail","fullBuild":"12.0.7.68974","region":"us","locale":"enUS","search":"","page":1}"#;
    let unscoped_key = format!("hotfix-wago-page/{}", storage::sha256(unscoped_bytes));
    assert_ne!(key, unscoped_key);
    let mut unscoped_receipt = receipt.clone();
    unscoped_receipt["identity"] = serde_json::from_slice(unscoped_bytes).unwrap();
    unscoped_receipt["requestUrl"] = json!("https://wago.tools/hotfixes?page=1");
    DataCacheMetadata::put_controlled_with_check(
        &f.home,
        &unscoped_key,
        &unscoped_receipt,
        None,
        &Default::default(),
        || Ok(()),
    )
    .unwrap();
    let mut bytes = Vec::new();
    let error = lycheedev_data::hotfix::remote::render(&inv, None, &mut bytes).unwrap_err();
    assert_eq!(error.code, "records.hotfix_offline");
    assert_eq!(error.exit_code, 3);
    assert!(bytes.is_empty());

    DataCacheMetadata::put_controlled_with_check(
        &f.home,
        &key,
        &receipt,
        None,
        &Default::default(),
        || Ok(()),
    )
    .unwrap();
    for max_bytes in [None, Some("268435456")] {
        let mut a = base.clone();
        if let Some(value) = max_bytes {
            a.extend(words(&["--max-bytes", value]));
        }
        let inv = parse(&a).unwrap();
        let mut bytes = Vec::new();
        lycheedev_data::hotfix::remote::render(&inv, None, &mut bytes).unwrap();
        let public: Value = serde_json::from_slice(&bytes).unwrap();
        let fetch = &public["result"]["coverage"]["fetch"];
        assert_eq!(
            fetch["maxBytes"],
            if max_bytes.is_none() {
                128 << 20
            } else {
                256 << 20
            }
        );
        assert_eq!(fetch["requests"], 0);
        assert_eq!(fetch["bytes"], 0);
        assert_eq!(public["result"]["coverage"]["fullQueryComplete"], true);
    }

    // A hit is still required to verify the receipt's embedded identity.
    DataCacheMetadata::put_controlled_with_check(
        &f.home,
        &key,
        &unscoped_receipt,
        Some(1),
        &Default::default(),
        || Ok(()),
    )
    .unwrap();
    let mut bytes = Vec::new();
    let error = lycheedev_data::hotfix::remote::render(&inv, None, &mut bytes).unwrap_err();
    assert_eq!(error.code, "records.hotfix_cache_corrupt");
    assert_eq!(error.exit_code, 4);
    assert!(bytes.is_empty());
}
