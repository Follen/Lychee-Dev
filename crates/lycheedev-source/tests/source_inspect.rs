//! Public Source inspection/query consumers, pinned to the Go 3.2.3 contract.
use lycheedev_core::{
    Invocation, Outcome,
    control::OperationControl,
    evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec, SourcePin},
    storage, workspace,
};
use lycheedev_source::{dispatch, git, index};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

struct Fixture {
    home: PathBuf,
    source: SourcePin,
    snapshot: String,
}
impl Fixture {
    fn new(files: BTreeMap<String, Vec<u8>>) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lycheedev-inspect-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        let mirror = git::mirror(&home, "elvui");
        fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--bare"])
                .arg(&mirror)
                .output()
                .unwrap()
                .status
                .success()
        );
        let run = |input: &[u8], args: &[&str]| {
            let mut child = Command::new("git")
                .arg("--git-dir")
                .arg(&mirror)
                .args(args)
                .env("GIT_AUTHOR_NAME", "Inspect Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Inspect Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        fn tree(
            blobs: &BTreeMap<String, String>,
            prefix: &str,
            run: &impl Fn(&[u8], &[&str]) -> String,
        ) -> String {
            let mut entries = String::new();
            let mut dirs = BTreeSet::new();
            for (path, blob) in blobs {
                if let Some(relative) = path.strip_prefix(prefix) {
                    if let Some((dir, _)) = relative.split_once('/') {
                        dirs.insert(dir);
                    } else {
                        entries.push_str(&format!("100644 blob {blob}\t{relative}\0"));
                    }
                }
            }
            for dir in dirs {
                let child = tree(blobs, &format!("{prefix}{dir}/"), run);
                entries.push_str(&format!("040000 tree {child}\t{dir}\0"));
            }
            // NUL framing preserves quotes and Unicode as original Git bytes.
            run(entries.as_bytes(), &["mktree", "-z"])
        }
        let blobs = files
            .into_iter()
            .map(|(path, raw)| (path, run(&raw, &["hash-object", "-w", "--stdin"])))
            .collect();
        let tree = tree(&blobs, "", &run);
        let source = SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: run(b"inspect", &["commit-tree", &tree]),
            parser_revision: git::PARSER.into(),
        };
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                source: Some(source.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            home,
            source,
            snapshot: pin.id,
        }
    }
    fn invoke(&self, command: &str, flags: &[(&str, &str)]) -> Invocation {
        let mut flags = flags
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>();
        flags.insert("--snapshot".into(), self.snapshot.clone());
        Invocation {
            home: self.home.clone(),
            cwd: self.home.clone(),
            command: command.into(),
            positional: None,
            flags,
            control: Default::default(),
        }
    }
    fn inspect(&self, flag: &str, target: &str) -> Outcome {
        let outcome = dispatch(&self.invoke("source inspect", &[(flag, target)])).unwrap();
        assert_eq!(
            outcome.provenance[0]["provenance"]["kind"],
            "source-inspect"
        );
        outcome
    }
    fn query(&self, target: &str) -> Outcome {
        let mut inv = self.invoke("source query", &[("--topic", "asset")]);
        inv.positional = Some(target.into());
        let outcome = dispatch(&inv).unwrap();
        assert_eq!(outcome.provenance[0]["provenance"]["kind"], "source-query");
        outcome
    }
    fn assert_capture(&self, outcome: &Outcome, target: &str) {
        assert_eq!(outcome.data["sourceId"], self.source.repository);
        assert_eq!(outcome.data["product"], self.source.product);
        assert_eq!(outcome.data["requestedRef"], self.source.requested_ref);
        assert_eq!(outcome.data["resolvedCommit"], self.source.exact_commit);
        assert_eq!(outcome.data["snapshotId"], self.snapshot);
        assert_eq!(outcome.data.get("matchedTag"), Some(&Value::Null));
        assert_eq!(outcome.selection["id"], self.snapshot);
        assert_eq!(outcome.provenance.len(), 1);
        let capture = &outcome.provenance[0];
        assert_eq!(
            capture["provenance"]["sourceCommit"],
            self.source.exact_commit
        );
        assert_eq!(capture["provenance"]["snapshot"], self.snapshot);
        assert_eq!(capture["provenance"]["locator"], target);
        assert_eq!(capture["complete"], outcome.complete);
        assert_eq!(capture["truncated"], outcome.data["truncated"]);
        assert_eq!(outcome.coverage, outcome.data["coverage"]);
        assert_eq!(
            serde_json::from_slice::<Value>(
                &evidence::read_blob(&self.home, &capture["blob"], 1 << 20).unwrap()
            )
            .unwrap(),
            outcome.data
        );
    }
    fn clear_derived(&self) {
        for name in ["cache/objects", "cache/source-originals-rust"] {
            let path = self.home.join(name);
            if path.exists() {
                fs::remove_dir_all(path).unwrap();
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
fn files(rows: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    rows.iter()
        .map(|(p, r)| (p.to_string(), r.to_vec()))
        .collect()
}
fn png() -> Vec<u8> {
    let mut raw = vec![0; 24];
    raw[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    raw[16..20].copy_from_slice(&123u32.to_be_bytes());
    raw[20..24].copy_from_slice(&456u32.to_be_bytes());
    raw
}
fn blp() -> Vec<u8> {
    let mut raw = vec![0; 20];
    raw[..4].copy_from_slice(b"BLP2");
    raw[12..16].copy_from_slice(&64u32.to_le_bytes());
    raw[16..20].copy_from_slice(&32u32.to_le_bytes());
    raw
}

#[test]
fn inspect_exact_document_symbol_routes_to_six_line_path_inspection_cold_and_warm() {
    let raw = b"-- first\r\nfunction Main() end\n-- 3\n-- 4\n-- 5\n-- 6\n-- 7\n";
    let f = Fixture::new(files(&[("Main.lua", raw)]));
    let symbol = f.inspect("--symbol", "Main.lua");
    let path = f.inspect("--target-path", "Main.lua");
    assert_eq!(symbol.data["results"], path.data["results"]);
    assert_eq!(symbol.data["results"][0]["kind"], "file");
    assert_eq!(
        symbol.data["results"][0]["excerpt"],
        "1: -- first\n2: function Main() end\n3: -- 3\n4: -- 4\n5: -- 5\n6: -- 6"
    );
    assert_eq!(
        symbol.data["results"][0]["contentHash"],
        storage::sha256(raw)
    );
    // Full-index cold IO has no cachedIndex field; its actual work counters
    // prove the cold build instead of treating an absent field as false.
    assert_eq!(symbol.coverage["io"].get("cachedIndex"), None);
    assert_eq!(symbol.coverage["io"]["gitTreeProcesses"], 1);
    assert_eq!(symbol.coverage["io"]["gitBlobProcesses"], 1);
    assert_eq!(symbol.coverage["io"]["cachedFacts"], 0);
    assert_eq!(symbol.coverage["io"]["parsedDocuments"], 1);
    assert_eq!(symbol.coverage["io"]["readBytes"], raw.len());
    assert_eq!(path.coverage["io"]["cachedIndex"], true);
    assert_eq!(path.coverage["io"]["gitTreeProcesses"], 0);
    assert_eq!(path.coverage["io"]["gitBlobProcesses"], 0);
    assert_eq!(path.coverage["io"]["cachedFacts"], 1);
    assert_eq!(path.coverage["io"]["parsedDocuments"], 0);
    assert!(path.coverage["io"]["indexBytesRead"].as_u64().unwrap() > 0);
    for outcome in [&symbol, &path] {
        assert_eq!(outcome.coverage["io"]["sourceDocumentsRead"], 1);
        assert_eq!(outcome.coverage["io"]["sourceBytesRead"], raw.len());
    }
    f.assert_capture(&symbol, "Main.lua");
    f.assert_capture(&path, "Main.lua");
}
#[test]
fn inspect_exact_png_and_blp_symbols_return_verified_metadata_and_local_originals() {
    let p = png();
    let b = blp();
    let f = Fixture::new(files(&[
        ("Image.PNG", &p),
        ("Assets/ÜBER.blp", &b),
        ("Assets/quo\"te ÜBER.blp", &b),
    ]));
    for (name, raw, format, mime, width, height) in [
        ("Image.PNG", &p, "png", "image/png", 123, 456),
        ("Assets/ÜBER.blp", &b, "blp", "image/x-blp", 64, 32),
        ("Assets/quo\"te ÜBER.blp", &b, "blp", "image/x-blp", 64, 32),
    ] {
        for flag in ["--symbol", "--target-path"] {
            let result = f.inspect(flag, name);
            let hash = storage::sha256(raw);
            let local = evidence::blob_path(&f.home, &hash).unwrap();
            assert_eq!(
                result.data["results"],
                json!([{"kind":"asset","name":name.rsplit('/').next().unwrap(),"path":name,"line":1,"matchedBy":"path","role":"project","score":100,"contentHash":hash,"excerpt":format!("format={format} mime={mime} bytes={} width={width} height={height} local={}",raw.len(),local.display())}])
            );
            assert_eq!(fs::read(local).unwrap(), *raw);
            f.assert_capture(&result, name);
        }
    }
}
#[test]
fn inspect_fuzzy_documents_and_assets_share_one_twenty_five_row_limit() {
    let mut rows = BTreeMap::new();
    for n in 0..20 {
        rows.insert(format!("Needle{n:02}.lua"), b"-- document\n".to_vec());
    }
    for n in 0..10 {
        rows.insert(format!("Needle{n:02}.png"), png());
    }
    let f = Fixture::new(rows);
    let result = f.inspect("--target-path", "Needle");
    let rows = result.data["results"].as_array().unwrap();
    assert_eq!(rows.len(), 25);
    assert!(rows[..20].iter().all(|row| row["kind"] == "file"));
    assert!(rows[20..].iter().all(|row| row["kind"] == "asset"));
    assert_eq!(result.data["truncated"], true);
    assert!(!result.complete);
    f.assert_capture(&result, "Needle");
}
#[test]
fn inspect_asset_exact_path_sorts_before_lexically_earlier_substring_match() {
    let raw = png();
    let f = Fixture::new(files(&[("A/Image.png", &raw), ("Image.png", &raw)]));
    let result = f.inspect("--target-path", "Image.png");
    assert_eq!(result.data["results"][0]["path"], "Image.png");
    assert_eq!(result.data["results"][1]["path"], "A/Image.png");
    f.assert_capture(&result, "Image.png");
}
#[test]
fn inspect_non_path_symbol_uses_existing_precise_query_and_no_match_suggests_assets() {
    let f = Fixture::new(files(&[("Main.lua", b"function Main() end\n")]));
    let result = f.inspect("--symbol", "Main");
    let full = index::build(&f.home, &f.source)
        .unwrap()
        .search(&f.source, &f.snapshot, "Main", "precise", "", 25, "")
        .unwrap();
    assert_eq!(result.data["results"], full["results"]);
    assert!(!result.data["results"].as_array().unwrap().is_empty());
    f.assert_capture(&result, "Main");
    let missing = f.inspect("--target-path", "missing.asset");
    assert_eq!(missing.data["results"], json!([]));
    assert_eq!(
        missing.data["suggestions"],
        json!(["use query --topic asset with a filename or a shorter path"])
    );
    f.assert_capture(&missing, "missing.asset");
}
#[test]
fn inspect_partial_index_coverage_and_capture_remain_incomplete_without_truncation() {
    let f = Fixture::new(files(&[
        ("Main.lua", b"-- good\n"),
        ("Broken.lua", b"function broken(\n"),
    ]));
    let result = f.inspect("--symbol", "Main.lua");
    assert!(!result.complete);
    assert_eq!(result.data["complete"], false);
    assert_eq!(result.data["truncated"], false);
    assert_eq!(result.provenance[0]["complete"], false);
    f.assert_capture(&result, "Main.lua");
}
#[test]
fn inspect_warm_document_authenticates_and_recovers_corrupt_or_missing_original_cache() {
    let raw = b"-- authentic\nfunction Main() end\n";
    let f = Fixture::new(files(&[("Main.lua", raw)]));
    let first = f.inspect("--symbol", "Main.lua");
    let original = f
        .home
        .join("cache/source-originals-rust")
        .join(storage::sha256(raw));
    assert_eq!(fs::read(&original).unwrap(), raw);
    fs::write(&original, vec![b'x'; raw.len()]).unwrap();
    let recovered = f.inspect("--symbol", "Main.lua");
    assert_eq!(recovered.data["results"], first.data["results"]);
    assert_eq!(fs::read(&original).unwrap(), raw);
    fs::remove_file(&original).unwrap();
    let recovered = f.inspect("--target-path", "Main.lua");
    assert_eq!(recovered.data["results"], first.data["results"]);
    assert_eq!(fs::read(original).unwrap(), raw);
}
#[test]
fn inspect_asset_missing_original_recovers_fixed_git_but_corruption_is_an_error() {
    let raw = png();
    for name in ["Image.png", "Assets/quo\"te ÜBER.png"] {
        let f = Fixture::new(files(&[(name, &raw)]));
        let first = f.inspect("--symbol", name);
        let original = evidence::blob_path(&f.home, &storage::sha256(&raw)).unwrap();
        fs::remove_file(&original).unwrap();
        let recovered = f.inspect("--target-path", name);
        assert_eq!(recovered.data["results"], first.data["results"]);
        assert_eq!(fs::read(&original).unwrap(), raw);
        f.assert_capture(&first, name);
        f.assert_capture(&recovered, name);
        let corrupt = vec![1; raw.len()];
        fs::write(&original, &corrupt).unwrap();
        assert_eq!(
            dispatch(&f.invoke("source inspect", &[("--symbol", name)]))
                .unwrap_err()
                .code,
            "vault.blob_integrity"
        );
        assert_eq!(fs::read(&original).unwrap(), corrupt);
    }
}
#[test]
fn inspect_raw_selector_admission_precedes_pin_and_index_access() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    fs::remove_dir_all(git::mirror(&f.home, "elvui")).unwrap();
    for flags in [
        vec![],
        vec![("--symbol", "")],
        vec![("--symbol", "Main"), ("--target-path", "Main")],
        vec![("--symbol", " "), ("--target-path", "Main.lua")],
        vec![("--path", "Main.lua"), ("--symbol", "Main")],
    ] {
        let mut inv = f.invoke("source inspect", &flags);
        inv.flags.insert("--snapshot".into(), "missing".into());
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.message,
            "source sync requires --source/--product; source inspect requires --snapshot and exactly one of --path, --symbol, --target-path"
        );
    }
}
#[test]
fn inspect_injected_empty_selectors_preserve_target_and_excerpt_routes() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    // Actual argv rejects empty values; this tests the internal Invocation seam.
    for flags in [
        vec![("--symbol", ""), ("--target-path", "Main.lua")],
        vec![("--path", ""), ("--target-path", "Main.lua")],
        vec![("--path", ""), ("--symbol", "Main.lua")],
    ] {
        let out = dispatch(&f.invoke("source inspect", &flags)).unwrap();
        assert_eq!(out.data["results"][0]["path"], "Main.lua");
        assert_eq!(out.provenance[0]["provenance"]["kind"], "source-inspect");
        assert_eq!(out.provenance[0]["provenance"]["locator"], "Main.lua");
    }
    let out = dispatch(&f.invoke(
        "source inspect",
        &[
            ("--path", "Main.lua"),
            ("--symbol", ""),
            ("--target-path", ""),
            ("--line", "1"),
            ("--count", "1"),
        ],
    ))
    .unwrap();
    assert_eq!(out.data["text"], "-- main\n");
    assert_eq!(out.provenance[0]["provenance"]["kind"], "source-excerpt");
}
#[test]
fn inspect_trimmed_target_errors_keep_go_plain_failure_after_ready_index() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    index::build(&f.home, &f.source).unwrap();
    let too_long = "x".repeat(4097);
    for (flag, target, message) in [
        ("--target-path", too_long.as_str(), "codebase.invalid_span"),
        ("--symbol", " ", "codebase.inspect_target_required"),
        ("--target-path", " \t", "codebase.inspect_target_required"),
    ] {
        let error = dispatch(&f.invoke("source inspect", &[(flag, target)])).unwrap_err();
        assert_eq!(error.code, "command.failed");
        assert_eq!(error.exit_code, 5);
        assert_eq!(error.message, message);
        assert_eq!(error.details, Value::Null);
    }
}
#[test]
fn inspect_index_failure_precedes_target_validation_and_keeps_original_stop_control() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    index::build(&f.home, &f.source).unwrap();
    let metadata = Metadata::open(&f.home, false).unwrap();
    let manifests = metadata.list::<Value>("source/rust-index/", 10).unwrap();
    metadata.close().unwrap();
    assert_eq!(manifests.len(), 1);
    let digest = manifests[0]["blob"]["sha256"].as_str().unwrap();
    fs::write(
        f.home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]),
        b"corrupt",
    )
    .unwrap();
    let baseline = dispatch(&f.invoke("source inspect", &[("--symbol", "Main.lua")])).unwrap_err();
    assert_eq!(baseline.code, "codebase.source_integrity");
    let too_long = "x".repeat(4097);
    for (flag, target) in [("--symbol", " "), ("--target-path", too_long.as_str())] {
        let error = dispatch(&f.invoke("source inspect", &[(flag, target)])).unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    for (control, code) in [
        (
            OperationControl::new(
                Instant::now() + Duration::from_secs(60),
                Arc::new(AtomicBool::new(true)),
            ),
            "command.cancelled",
        ),
        (
            OperationControl::uncancelable(Instant::now()),
            "command.deadline",
        ),
    ] {
        let mut inv = f.invoke("source inspect", &[("--symbol", "Main.lua")]);
        inv.control = control;
        assert_eq!(dispatch(&inv).unwrap_err().code, code);
    }
}
#[test]
fn unicode_asset_queries_match_full_index_cold_warm_and_rebuilt_file_projection() {
    let raw = blp();
    let p = png();
    let f = Fixture::new(files(&[
        ("Assets/ÜBER.blp", &raw),
        ("Assets/ASCII.PNG", &p),
        ("Other.blp", &raw),
    ]));
    let mut expected = None;
    for phase in ["cold", "warm", "rebuilt"] {
        if phase == "rebuilt" {
            f.clear_derived();
        }
        let first = f.query("über.blp");
        assert_eq!(first.data["results"].as_array().unwrap().len(), 1);
        assert_eq!(first.data["results"][0]["path"], "Assets/ÜBER.blp");
        assert_eq!(
            first.data["results"][0]["contentHash"],
            storage::sha256(&raw)
        );
        assert_eq!(first.coverage["io"]["cachedIndex"], phase == "warm");
        if let Some(expected) = &expected {
            assert_eq!(&first.data["results"], expected);
        } else {
            expected = Some(first.data["results"].clone());
        }
        for (query, path) in [
            ("assets/über.blp", "Assets/ÜBER.blp"),
            ("ASSETS\\ÜBER.BLP", "Assets/ÜBER.blp"),
            ("assets/ascii.png", "Assets/ASCII.PNG"),
            ("ASSETS\\ASCII.PNG", "Assets/ASCII.PNG"),
        ] {
            let projected = f.query(query);
            let full = index::build(&f.home, &f.source)
                .unwrap()
                .search(&f.source, &f.snapshot, query, "", "asset", 50, "")
                .unwrap();
            assert_eq!(projected.data["results"], full["results"]);
            assert_eq!(projected.data["results"][0]["path"], path);
            assert_eq!(projected.data["results"][0]["score"], 100);
            f.assert_capture(&projected, query);
        }
    }
}
#[test]
fn inspect_rebuilds_v14_asset_metadata_without_rewriting_the_historical_record() {
    let raw = png();
    let f = Fixture::new(files(&[("Image.png", &raw)]));
    let source = &f.source;
    let old_identity=storage::sha256(format!("rust-index-v14\0facts-pack-v2-path-radix512\0facts-layout-v11-go323-generated-origins\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}",source.repository,source.product,source.exact_commit,source.parser_revision).as_bytes());
    let legacy = json!({"documents":[],"symbols":[],"assets":[{"path":"Image.png","contentHash":storage::sha256(&raw),"bytes":raw.len(),"extension":".png","mime":"application/octet-stream","format":"png","width":0,"height":0}],"summary":{"repository":source.repository,"product":source.product,"commit":source.exact_commit,"parser":source.parser_revision,"complete":true,"skippedDocuments":0,"diagnostics":0},"compatibilityGaps":[]});
    let blob = lycheedev_core::cache::publish_derived_controlled(
        &f.home,
        &serde_json::to_vec(&legacy).unwrap(),
        1 << 20,
        &OperationControl::default(),
    )
    .unwrap();
    let key = format!("source/rust-index/{old_identity}");
    let manifest =
        json!({"schema":"lycheedev.source-index.rust.v6","identity":old_identity,"blob":blob});
    let mut metadata = Metadata::open(&f.home, true).unwrap();
    metadata.put(&key, &manifest, None).unwrap();
    metadata.close().unwrap();
    let result = f.inspect("--symbol", "Image.png");
    assert!(
        result.data["results"][0]["excerpt"]
            .as_str()
            .unwrap()
            .contains("mime=image/png bytes=24 width=123 height=456")
    );
    let metadata = Metadata::open(&f.home, false).unwrap();
    assert_eq!(metadata.read::<Value>(&key).unwrap(), manifest);
    assert_eq!(
        metadata
            .list::<Value>("source/rust-index/", 10)
            .unwrap()
            .len(),
        2
    );
    metadata.close().unwrap();
    f.assert_capture(&result, "Image.png");
}
#[test]
fn inspect_selector_trimming_preserves_raw_capture_locator() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    for flag in ["--symbol", "--target-path"] {
        let result = f.inspect(flag, "  Main.lua  ");
        assert_eq!(result.data["results"][0]["path"], "Main.lua");
        f.assert_capture(&result, "  Main.lua  ");
    }
}
