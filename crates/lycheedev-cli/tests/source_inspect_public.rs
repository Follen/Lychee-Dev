//! Actual CLI admission, fixed-reference inspect failures, and committed evidence.
//! No release tools, native engine, external repository, or game are required.
use lycheedev_core::{
    evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::git;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const ADMISSION: &str = "source sync requires --source/--product; source inspect requires --snapshot and exactly one of --path, --symbol, --target-path";

struct Fixture {
    home: PathBuf,
    snapshot: String,
}
impl Fixture {
    fn unused() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lycheedev-inspect-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        assert!(!home.exists());
        Self {
            home,
            snapshot: String::new(),
        }
    }

    fn indexed() -> Self {
        let mut fixture = Self::unused();
        workspace::initialize(&fixture.home).unwrap();
        let mirror = git::mirror(&fixture.home, "wow-ui-source");
        fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        let output = Command::new("git")
            .args(["init", "--bare"])
            .arg(&mirror)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let run = |input: &[u8], args: &[&str]| {
            let mut child = Command::new("git")
                .arg("--git-dir")
                .arg(&mirror)
                .args(args)
                .env("GIT_AUTHOR_NAME", "Inspect CLI fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Inspect CLI fixture")
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
        let lua = run(
            b"-- main\nfunction Main() end\n",
            &["hash-object", "-w", "--stdin"],
        );
        // Header-only metadata input, not a complete decodable PNG.
        let mut png = vec![0; 24];
        png[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
        png[8..12].copy_from_slice(&13u32.to_be_bytes());
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&123u32.to_be_bytes());
        png[20..24].copy_from_slice(&456u32.to_be_bytes());
        let image = run(&png, &["hash-object", "-w", "--stdin"]);
        let tree = run(
            format!("100644 blob {image}\tImage.png\n100644 blob {lua}\tMain.lua\n").as_bytes(),
            &["mktree"],
        );
        let pin = selection::pin_selection(
            &fixture.home,
            SelectionSpec {
                source: Some(SourcePin {
                    repository: "wow-ui-source".into(),
                    product: "retail".into(),
                    requested_ref: "fixture".into(),
                    exact_commit: run(b"inspect CLI", &["commit-tree", &tree]),
                    parser_revision: git::PARSER.into(),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        fixture.snapshot = pin.id;
        let (exit, value) = invoke(
            &fixture.home,
            &["source", "index", "--snapshot", &fixture.snapshot],
        );
        assert_eq!(exit, 0, "{value}");
        assert_eq!(value["ok"], true);
        assert_eq!(fixture.captures().len(), 1);
        fixture
    }

    fn inspect(&self, args: &[&str]) -> (i32, Value) {
        let mut full = vec!["source", "inspect", "--snapshot", &self.snapshot];
        full.extend_from_slice(args);
        invoke(&self.home, &full)
    }

    fn records(&self, prefix: &str) -> Vec<Value> {
        let metadata = Metadata::open(&self.home, false).unwrap();
        let records = metadata.list::<Value>(prefix, 1000).unwrap();
        metadata.close().unwrap();
        records
    }

    fn captures(&self) -> Vec<Value> {
        self.records("capture/")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn invoke(home: &Path, args: &[&str]) -> (i32, Value) {
    invoke_at(home, None, args)
}

fn invoke_at(home: &Path, cwd: Option<&Path>, args: &[&str]) -> (i32, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lycheedev"));
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command
        .args(args)
        .arg("--home")
        .arg(home)
        .args(["--format", "json"])
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().expect("normal CLI exit"), value)
}

fn failure(code: &str, message: &str) -> Value {
    json!({
        "schema":"lycheedev.result.v1", "ok":false, "operationId":"",
        "context":{}, "result":null, "captures":[], "warnings":[],
        "error":{"code":code,"message":message,"stage":"dispatch","retryable":false,"resumeOperationId":""}
    })
}

#[test]
fn actual_source_inspect_trimmed_target_errors_keep_reference_plain_failure_envelope() {
    let fixture = Fixture::indexed();
    let captures = fixture.captures();
    let too_long = "a".repeat(4097);
    for (flag, text, message) in [
        ("--target-path", too_long.as_str(), "codebase.invalid_span"),
        ("--symbol", " ", "codebase.inspect_target_required"),
        ("--target-path", " \t", "codebase.inspect_target_required"),
    ] {
        let (exit, value) = fixture.inspect(&[flag, text]);
        assert_eq!(exit, 5, "{flag}: {value}");
        assert_eq!(value, failure("command.failed", message));
        assert_eq!(
            fixture.captures(),
            captures,
            "failure must not commit a capture"
        );
    }
}

#[test]
fn actual_source_inspect_raw_admission_precedes_workspace_and_pin() {
    let fixture = Fixture::unused();
    for args in [
        vec!["source", "inspect", "--snapshot", "missing"],
        vec![
            "source",
            "inspect",
            "--snapshot",
            "missing",
            "--symbol",
            "Main",
            "--target-path",
            "Main.lua",
        ],
        vec![
            "source",
            "inspect",
            "--snapshot",
            "missing",
            "--symbol",
            " ",
            "--target-path",
            "Main.lua",
        ],
        vec![
            "source",
            "inspect",
            "--snapshot",
            "missing",
            "--path",
            "Main.lua",
            "--symbol",
            "Main",
        ],
    ] {
        let (exit, value) = invoke(&fixture.home, &args);
        assert_eq!(exit, 2, "{args:?}: {value}");
        assert_eq!(
            value,
            failure("command.invalid_arguments", ADMISSION),
            "{args:?}"
        );
        assert!(
            !fixture.home.exists(),
            "raw admission must not create a workspace"
        );
    }
}

#[test]
fn actual_source_inspect_empty_argv_values_keep_parser_rejection() {
    let fixture = Fixture::unused();
    // The public parser rejects empty values before the raw-selector route gate.
    // The public CLI admits command arguments before calling typed Source requests.
    for flag in ["--snapshot", "--path", "--symbol", "--target-path"] {
        for args in [
            vec![
                "source".to_owned(),
                "inspect".into(),
                flag.into(),
                String::new(),
            ],
            vec!["source".to_owned(), "inspect".into(), format!("{flag}=")],
        ] {
            let borrowed: Vec<_> = args.iter().map(String::as_str).collect();
            let (exit, value) = invoke(&fixture.home, &borrowed);
            assert_eq!(exit, 2);
            let details = &value["error"]["details"];
            assert_eq!(details["parser"], "clap");
            assert_eq!(details["kind"], "InvalidValue");
            assert!(details["diagnostic"].as_str().unwrap().contains(flag));
            let mut expected = failure("command.invalid_arguments", "invalid command arguments");
            expected["error"]["details"] = details.clone();
            assert_eq!(value, expected);
            assert!(!fixture.home.exists());
        }
    }
}

#[test]
fn actual_source_inspect_document_asset_and_excerpt_keep_public_capture_routes() {
    let fixture = Fixture::indexed();
    for (args, kind, path) in [
        (
            vec!["--target-path", "Main.lua"],
            "source-inspect",
            "Main.lua",
        ),
        (vec!["--symbol", "Main.lua"], "source-inspect", "Main.lua"),
        (vec!["--symbol", "Image.png"], "source-inspect", "Image.png"),
        (
            vec!["--path", "Main.lua", "--line", "1", "--count", "1"],
            "source-excerpt",
            "Main.lua",
        ),
    ] {
        let (exit, value) = fixture.inspect(&args);
        assert_eq!(exit, 0, "{args:?}: {value}");
        assert_eq!(value["schema"], "lycheedev.result.v1");
        assert_eq!(value["ok"], true);
        assert_eq!(value["error"], Value::Null);
        assert_eq!(value["context"]["snapshot"], fixture.snapshot);
        assert_eq!(value["captures"].as_array().unwrap().len(), 1);
        let capture = &value["captures"][0];
        assert_eq!(capture["provenance"]["kind"], kind);
        assert_eq!(capture["provenance"]["locator"], path);
        assert_eq!(capture["provenance"]["snapshot"], fixture.snapshot);
        assert_eq!(capture["complete"], true);
        assert_eq!(capture["truncated"], false);
        let stored: Value = serde_json::from_slice(
            &evidence::read_blob(&fixture.home, &capture["blob"], 1 << 20).unwrap(),
        )
        .unwrap();
        assert_eq!(stored, value["result"]);
        if kind == "source-excerpt" {
            assert_eq!(value["result"]["path"], path);
            assert_eq!(value["result"]["text"], "-- main\n");
        } else {
            assert_eq!(value["result"]["results"].as_array().unwrap().len(), 1);
            let row = &value["result"]["results"][0];
            assert_eq!(row["path"], path);
            if path == "Image.png" {
                let local =
                    evidence::blob_path(&fixture.home, row["contentHash"].as_str().unwrap())
                        .unwrap();
                assert_eq!(
                    row["excerpt"],
                    format!(
                        "format=png mime=image/png bytes=24 width=123 height=456 local={}",
                        local.display()
                    )
                );
                let raw = fs::read(local).unwrap();
                assert_eq!(raw.len(), 24);
                assert_eq!(&raw[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
                assert_eq!(&raw[16..20], &123u32.to_be_bytes());
                assert_eq!(&raw[20..24], &456u32.to_be_bytes());
            } else {
                assert_eq!(row["excerpt"], "1: -- main\n2: function Main() end\n3: ");
            }
        }
    }
}

#[test]
fn actual_source_inspect_nearest_project_preserves_success_and_error_context() {
    let fixture = Fixture::indexed();
    let project = fixture.home.join("project");
    let cwd = project.join("nested");
    fs::create_dir_all(&cwd).unwrap();
    let (exit, value) = invoke_at(
        &fixture.home,
        Some(&cwd),
        &["source", "inspect", "--symbol", "Main.lua"],
    );
    assert_eq!(exit, 2);
    assert_eq!(value, failure("command.invalid_arguments", ADMISSION));

    let original_captures = fixture.captures();
    for (declaration, code, expected_exit) in [
        (b"{}".as_slice(), "selection.project_format", 4),
        (
            br#"{"schema":"lycheedev.project.v1","product":"retail"}"#.as_slice(),
            "selection.project_unlocked",
            3,
        ),
    ] {
        fs::write(project.join("lycheedev.json"), declaration).unwrap();
        for args in [
            vec!["source", "inspect", "--symbol", "Main.lua"],
            vec![
                "source",
                "inspect",
                "--symbol",
                " ",
                "--target-path",
                "Main.lua",
            ],
        ] {
            let (exit, value) = invoke_at(&fixture.home, Some(&cwd), &args);
            let mut expected = failure(code, code);
            expected["error"]["stage"] = json!("selection");
            assert_eq!(exit, expected_exit, "{value}");
            assert_eq!(value, expected);
            assert_eq!(fixture.captures(), original_captures);
        }
    }
    let pin = selection::load_pinned(&fixture.home, &fixture.snapshot).unwrap();
    fs::write(
        project.join("lycheedev.json"),
        br#"{"schema":"lycheedev.project.v1","product":"retail"}"#,
    )
    .unwrap();
    fs::write(
        project.join("lycheedev.lock.json"),
        serde_json::to_vec(&json!({"schema":"lycheedev.project-lock.v1","selection":pin})).unwrap(),
    )
    .unwrap();
    let context = json!({"project":project,"snapshot":fixture.snapshot});
    let (exit, value) = invoke_at(
        &fixture.home,
        Some(&cwd),
        &["source", "inspect", "--symbol", "Main.lua"],
    );
    assert_eq!(exit, 0, "{value}");
    assert_eq!(value["context"], context);
    assert_eq!(value["result"]["results"][0]["path"], "Main.lua");
    assert_eq!(
        value["captures"][0]["provenance"]["snapshot"],
        fixture.snapshot
    );
    assert_eq!(value["captures"][0]["provenance"]["kind"], "source-inspect");
    let captures = fixture.captures();
    let too_long = "a".repeat(4097);
    for (args, code, message, expected_exit) in [
        (
            vec!["source", "inspect", "--symbol", " "],
            "command.failed",
            "codebase.inspect_target_required",
            5,
        ),
        (
            vec!["source", "inspect", "--target-path", too_long.as_str()],
            "command.failed",
            "codebase.invalid_span",
            5,
        ),
        (
            vec![
                "source",
                "inspect",
                "--symbol",
                " ",
                "--target-path",
                "Main.lua",
            ],
            "command.invalid_arguments",
            ADMISSION,
            2,
        ),
    ] {
        let (exit, value) = invoke_at(&fixture.home, Some(&cwd), &args);
        let mut expected = failure(code, message);
        expected["context"] = context.clone();
        assert_eq!(exit, expected_exit, "{value}");
        assert_eq!(value, expected);
        assert_eq!(fixture.captures(), captures);
    }
}

#[test]
fn actual_source_inspect_index_failure_precedes_trimmed_target_validation() {
    let fixture = Fixture::indexed();
    let captures = fixture.captures();
    let manifests = fixture.records("source/rust-index/");
    assert_eq!(manifests.len(), 1);
    let digest = manifests[0]["blob"]["sha256"].as_str().unwrap();
    fs::write(
        fixture
            .home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]),
        b"corrupt",
    )
    .unwrap();
    let baseline = fixture.inspect(&["--symbol", "Main.lua"]);
    assert_eq!(baseline.0, 4);
    assert_eq!(baseline.1["error"]["code"], "codebase.source_integrity");
    assert_eq!(baseline.1["captures"], json!([]));
    let too_long = "a".repeat(4097);
    for (flag, text) in [("--symbol", " "), ("--target-path", too_long.as_str())] {
        assert_eq!(fixture.inspect(&[flag, text]), baseline);
        assert_eq!(fixture.captures(), captures);
    }
}
