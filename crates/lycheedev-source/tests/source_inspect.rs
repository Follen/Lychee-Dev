//! Typed Source inspection/query consumers over fixed local Git inputs.
use lycheedev_core::{
    Outcome,
    control::OperationControl,
    evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec, SourcePin},
    storage, workspace,
};
use lycheedev_source::{
    dispatch, git, index,
    request::{InspectTarget, QueryRequest, SelectedSource, SourceContext, SourceRequest},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    home: PathBuf,
    source: SourcePin,
    snapshot: String,
    selection: SelectedSource,
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
            snapshot: pin.id.clone(),
            selection: SelectedSource::new(pin).unwrap(),
        }
    }
    fn context(&self) -> SourceContext {
        SourceContext {
            home: self.home.clone(),
            cwd: self.home.clone(),
            control: Default::default(),
        }
    }
    fn inspect_request(&self, target: InspectTarget) -> SourceRequest {
        SourceRequest::Inspect {
            selection: self.selection.clone(),
            target,
        }
    }
    fn inspect(&self, target: InspectTarget) -> Outcome {
        let outcome = dispatch(&self.context(), &self.inspect_request(target)).unwrap();
        assert_eq!(
            outcome.provenance[0]["provenance"]["kind"],
            "source-inspect"
        );
        outcome
    }
    fn query(&self, target: &str) -> Outcome {
        let request = SourceRequest::Query {
            selection: self.selection.clone(),
            request: QueryRequest {
                query: target.into(),
                mode: String::new(),
                topic: "asset".into(),
                limit: 50,
                cursor: String::new(),
            },
        };
        let outcome = dispatch(&self.context(), &request).unwrap();
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
    // Header-only metadata input, not a complete decodable PNG.
    let mut raw = vec![0; 24];
    raw[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    raw[8..12].copy_from_slice(&13u32.to_be_bytes());
    raw[12..16].copy_from_slice(b"IHDR");
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
    let symbol = f.inspect(InspectTarget::Symbol("Main.lua".into()));
    let path = f.inspect(InspectTarget::TargetPath("Main.lua".into()));
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
        for target in [
            InspectTarget::Symbol(name.into()),
            InspectTarget::TargetPath(name.into()),
        ] {
            let result = f.inspect(target);
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
    let result = f.inspect(InspectTarget::TargetPath("Needle".into()));
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
    let result = f.inspect(InspectTarget::TargetPath("Image.png".into()));
    assert_eq!(result.data["results"][0]["path"], "Image.png");
    assert_eq!(result.data["results"][1]["path"], "A/Image.png");
    f.assert_capture(&result, "Image.png");
}
#[test]
fn inspect_non_path_symbol_uses_existing_precise_query_and_no_match_suggests_assets() {
    let f = Fixture::new(files(&[("Main.lua", b"function Main() end\n")]));
    let result = f.inspect(InspectTarget::Symbol("Main".into()));
    let full = index::build(&f.home, &f.source)
        .unwrap()
        .search(&f.source, &f.snapshot, "Main", "precise", "", 25, "")
        .unwrap();
    assert_eq!(result.data["results"], full["results"]);
    assert!(!result.data["results"].as_array().unwrap().is_empty());
    f.assert_capture(&result, "Main");
    let missing = f.inspect(InspectTarget::TargetPath("missing.asset".into()));
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
    let result = f.inspect(InspectTarget::Symbol("Main.lua".into()));
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
    let first = f.inspect(InspectTarget::Symbol("Main.lua".into()));
    let original = f
        .home
        .join("cache/source-originals-rust")
        .join(storage::sha256(raw));
    assert_eq!(fs::read(&original).unwrap(), raw);
    fs::write(&original, vec![b'x'; raw.len()]).unwrap();
    let recovered = f.inspect(InspectTarget::Symbol("Main.lua".into()));
    assert_eq!(recovered.data["results"], first.data["results"]);
    assert_eq!(fs::read(&original).unwrap(), raw);
    fs::remove_file(&original).unwrap();
    let recovered = f.inspect(InspectTarget::TargetPath("Main.lua".into()));
    assert_eq!(recovered.data["results"], first.data["results"]);
    assert_eq!(fs::read(original).unwrap(), raw);
}
#[test]
fn inspect_asset_missing_original_recovers_fixed_git_but_corruption_is_an_error() {
    let raw = png();
    for name in ["Image.png", "Assets/quo\"te ÜBER.png"] {
        let f = Fixture::new(files(&[(name, &raw)]));
        let first = f.inspect(InspectTarget::Symbol(name.into()));
        let original = evidence::blob_path(&f.home, &storage::sha256(&raw)).unwrap();
        fs::remove_file(&original).unwrap();
        let recovered = f.inspect(InspectTarget::TargetPath(name.into()));
        assert_eq!(recovered.data["results"], first.data["results"]);
        assert_eq!(fs::read(&original).unwrap(), raw);
        f.assert_capture(&first, name);
        f.assert_capture(&recovered, name);
        let corrupt = vec![1; raw.len()];
        fs::write(&original, &corrupt).unwrap();
        assert_eq!(
            dispatch(
                &f.context(),
                &f.inspect_request(InspectTarget::Symbol(name.into()))
            )
            .unwrap_err()
            .code,
            "vault.blob_integrity"
        );
        assert_eq!(fs::read(&original).unwrap(), corrupt);
    }
}
#[test]
fn inspect_trimmed_target_errors_keep_reference_plain_failure_after_ready_index() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    index::build(&f.home, &f.source).unwrap();
    let too_long = "x".repeat(4097);
    for (target, message) in [
        (InspectTarget::TargetPath(too_long), "codebase.invalid_span"),
        (
            InspectTarget::Symbol(" ".into()),
            "codebase.inspect_target_required",
        ),
        (
            InspectTarget::TargetPath(" \t".into()),
            "codebase.inspect_target_required",
        ),
    ] {
        let error = dispatch(&f.context(), &f.inspect_request(target)).unwrap_err();
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
    let baseline = dispatch(
        &f.context(),
        &f.inspect_request(InspectTarget::Symbol("Main.lua".into())),
    )
    .unwrap_err();
    assert_eq!(baseline.code, "codebase.source_integrity");
    let too_long = "x".repeat(4097);
    for target in [
        InspectTarget::Symbol(" ".into()),
        InspectTarget::TargetPath(too_long),
    ] {
        let error = dispatch(&f.context(), &f.inspect_request(target)).unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    for (control, code) in [
        (
            {
                let signal = lycheedev_core::control::TrackedCancellation::new();
                signal.cancel();
                OperationControl::tracked(Instant::now() + Duration::from_secs(60), signal)
            },
            "command.cancelled",
        ),
        (
            OperationControl::uncancelable(Instant::now()),
            "command.deadline",
        ),
    ] {
        let mut context = f.context();
        context.control = control;
        assert_eq!(
            dispatch(
                &context,
                &f.inspect_request(InspectTarget::Symbol("Main.lua".into()))
            )
            .unwrap_err()
            .code,
            code
        );
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
        if phase == "warm" {
            assert_eq!(first.coverage["io"]["cachedIndex"], true);
            assert_eq!(first.coverage["io"]["projection"], "file-posting");
        } else {
            assert_eq!(first.coverage["io"].get("cachedIndex"), None);
            assert_eq!(first.coverage["io"]["gitTreeProcesses"], 1);
            assert_eq!(first.coverage["io"]["projection"], "full-index");
        }
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
fn inspect_selector_trimming_preserves_raw_capture_locator() {
    let f = Fixture::new(files(&[("Main.lua", b"-- main\n")]));
    for target in [
        InspectTarget::Symbol("  Main.lua  ".into()),
        InspectTarget::TargetPath("  Main.lua  ".into()),
    ] {
        let result = f.inspect(target);
        assert_eq!(result.data["results"][0]["path"], "Main.lua");
        f.assert_capture(&result, "  Main.lua  ");
    }
}
// N11: source inspect and source query share one U+FFFD display while the
// archived blob, its hash and the byte coordinates keep the original bytes.
#[test]
fn non_utf8_lua_comment_inspect_and_query_display_fffd_without_rewriting_bytes() {
    let path = "Interface/AddOns/Blizzard_NewPlayerExperience/Camelot/Blizzard_TutorialData.lua";
    let mut raw = b"function Before() end\n".to_vec();
    raw.extend_from_slice(b"-- Night Web\x92s Hollow\r\n");
    raw.extend_from_slice(b"function After() end\n");
    let offset = raw.iter().position(|byte| *byte == 0x92).unwrap();
    let f = Fixture::new(BTreeMap::from([
        (path.to_owned(), raw.clone()),
        ("Other.lua".to_owned(), b"function Other() end\n".to_vec()),
    ]));
    let inspected = dispatch(
        &f.context(),
        &f.inspect_request(InspectTarget::Excerpt {
            path: path.into(),
            first_line: 1,
            line_count: 80,
        }),
    )
    .unwrap();
    assert_eq!(
        inspected.provenance[0]["provenance"]["kind"],
        "source-excerpt"
    );
    let expected = "function Before() end\n-- Night Web\u{fffd}s Hollow\r\nfunction After() end\n";
    assert_eq!(inspected.data["text"], expected);
    assert_eq!(inspected.data["firstLine"], 1);
    assert_eq!(inspected.data["lastLine"], 3);
    assert_eq!(inspected.data["totalLines"], 3);
    assert_eq!(inspected.data["warnings"][0]["path"], path);
    assert_eq!(inspected.data["warnings"][0]["line"], 2);
    assert_eq!(inspected.data["warnings"][0]["byteOffset"], offset);
    assert_eq!(inspected.data["blob"]["sha256"], storage::sha256(&raw));
    assert_eq!(inspected.data["blob"]["bytes"], raw.len());

    let queried = dispatch(
        &f.context(),
        &SourceRequest::Query {
            selection: f.selection.clone(),
            request: QueryRequest {
                query: "Night Web".into(),
                mode: String::new(),
                topic: String::new(),
                limit: 50,
                cursor: String::new(),
            },
        },
    )
    .unwrap();
    let row = &queried.data["results"][0];
    assert_eq!(row["path"], path);
    assert_eq!(row["contentHash"], storage::sha256(&raw));
    let excerpt = row["excerpt"].as_str().unwrap();
    assert!(
        excerpt.contains("2: -- Night Web\u{fffd}s Hollow"),
        "query display must match inspect: {excerpt}"
    );
    assert_eq!(queried.coverage["warnings"], 1);
    assert_eq!(queried.coverage["warningSample"][0]["byteOffset"], offset);
    // The indexed store keeps the original bytes and hash, cold and warm.
    for _ in 0..2 {
        let index = index::build(&f.home, &f.source).unwrap();
        let document = index
            .documents
            .iter()
            .find(|document| document.path == path)
            .unwrap();
        assert_eq!(document.sha256, storage::sha256(&raw));
        assert_eq!(
            index.excerpt(path, 1, 3).unwrap(),
            "1: function Before() end\n2: -- Night Web\u{fffd}s Hollow\n3: function After() end"
        );
    }
}
