use super::*;
use lycheedev_core::{
    metadata::Metadata,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    pin: String,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lycheedev-source-cli-{}-{}-{}",
            std::process::id(),
            lycheedev_core::storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let home = root.join("home");
        let project = root.join("project/nested");
        std::fs::create_dir_all(&project).unwrap();
        workspace::initialize(&home).unwrap();
        let commit = Self::source_commit(&home);
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                source: Some(SourcePin {
                    repository: "elvui".into(),
                    product: "main".into(),
                    exact_commit: commit,
                    parser_revision: "source-v1".into(),
                    requested_ref: "fixture".into(),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        Self { root, home, pin }
    }

    fn source_commit(home: &Path) -> String {
        let mirror = lycheedev_source::git::mirror(home, "elvui");
        std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--bare", "--quiet", mirror.to_str().unwrap()])
                .output()
                .unwrap()
                .status
                .success()
        );
        let git = |input: &[u8], args: &[&str]| {
            let mut child = Command::new("git")
                .arg(format!("--git-dir={}", mirror.display()))
                .args(args)
                .env("GIT_AUTHOR_NAME", "Source CLI fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Source CLI fixture")
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
        let blob = git(
            b"function Target()\n  return 'needle'\nend\n",
            &["hash-object", "-w", "--stdin"],
        );
        let tree = git(
            format!("100644 blob {blob}\tMain.lua\n").as_bytes(),
            &["mktree"],
        );
        git(b"", &["commit-tree", &tree, "-m", "Source CLI fixture"])
    }

    fn inv(&self, command: &str, flags: &[(&str, &str)]) -> Invocation {
        Invocation {
            command: command.into(),
            positional: None,
            flags: flags
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect::<BTreeMap<_, _>>(),
            home: self.home.clone(),
            cwd: self.root.join("project/nested"),
            control: Default::default(),
        }
    }

    fn write_project(&self, declaration: &[u8], locked: bool) {
        let project = self.root.join("project");
        std::fs::write(project.join("lycheedev.json"), declaration).unwrap();
        if locked {
            let pin = selection::load_pinned(&self.home, &self.pin).unwrap();
            std::fs::write(
                project.join("lycheedev.lock.json"),
                serde_json::to_vec(&json!({
                    "schema":"lycheedev.project-lock.v1",
                    "selection":pin,
                }))
                .unwrap(),
            )
            .unwrap();
        }
    }

    fn batch_file(&self, name: &str, contents: Value) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, serde_json::to_vec(&contents).unwrap()).unwrap();
        path
    }

    fn captures(&self) -> usize {
        let mut metadata = Metadata::open(&self.home, false).unwrap();
        let count = metadata.list::<Value>("capture/", 100).unwrap().len();
        metadata.close().unwrap();
        count
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn declaration(product: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema":"lycheedev.project.v1",
        "product":product,
    }))
    .unwrap()
}

fn assert_source_list_status(fixture: &Fixture, inv: &Invocation) {
    let request = ordinary(inv).unwrap();
    let SourceRequest::Status(selected) = &request.request else {
        panic!("selected source list must inspect its fixed pin, not the repository catalog");
    };
    assert_eq!(selected.snapshot(), fixture.pin);
    assert_eq!(selected.source().repository, "elvui");
    assert_eq!(selected.source().product, "main");
    let expected_commit = selected.source().exact_commit.clone();
    let outcome = SourceCommand::Ordinary(request)
        .dispatch(&context(inv), SourceTools::default())
        .unwrap();
    assert_eq!(outcome.data["activeSnapshot"], expected_commit);
    assert_eq!(outcome.data["ready"], false);
    assert_eq!(outcome.data["readySnapshots"], 0);
    assert_eq!(outcome.data["complete"], false);
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_list_explicit_project_selects_pinned_readiness() {
    let fixture = Fixture::new();
    fixture.write_project(&declaration("main"), true);
    let project = fixture.root.join("project");
    let mut inv = fixture.inv("source list", &[("--project", project.to_str().unwrap())]);
    // Only the explicit project can select this pin from outside its ancestors.
    inv.cwd = fixture.root.clone();
    assert_source_list_status(&fixture, &inv);
}

#[test]
fn source_list_nearest_project_selects_pinned_readiness() {
    let fixture = Fixture::new();
    fixture.write_project(&declaration("main"), true);
    assert_source_list_status(&fixture, &fixture.inv("source list", &[]));
}

#[test]
fn source_list_bad_explicit_project_does_not_fall_back_to_catalog() {
    let fixture = Fixture::new();
    fixture.write_project(b"broken", true);
    let project = fixture.root.join("project");
    let inv = fixture.inv("source list", &[("--project", project.to_str().unwrap())]);
    let error = ordinary(&inv)
        .err()
        .expect("an explicit malformed project must fail");
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.project_format", 4)
    );
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_list_without_project_retains_repository_catalog() {
    let fixture = Fixture::new();
    let mut inv = fixture.inv("source list", &[]);
    inv.cwd = fixture.root.clone();
    let request = ordinary(&inv).unwrap();
    assert!(matches!(&request.request, SourceRequest::Catalog));
    let outcome = SourceCommand::Ordinary(request)
        .dispatch(&context(&inv), SourceTools::default())
        .unwrap();
    let rows = outcome.data.as_array().unwrap();
    assert!(rows.iter().any(|row| row["key"] == "wow-ui-source"));
    assert!(rows.iter().any(|row| row["key"] == "elvui"));
    assert!(outcome.selection.as_object().unwrap().is_empty());
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_list_explicit_snapshot_bypasses_malformed_project() {
    let fixture = Fixture::new();
    fixture.write_project(b"broken", true);
    let project = fixture.root.join("project");
    let inv = fixture.inv(
        "source list",
        &[
            ("--snapshot", &fixture.pin),
            ("--project", project.to_str().unwrap()),
        ],
    );
    assert_source_list_status(&fixture, &inv);
}

#[test]
fn source_research_keeps_project_selector_and_explicit_snapshot_error_priority() {
    let fixture = Fixture::new();
    fixture.write_project(b"broken", true);

    let mut inv = fixture.inv(
        "source refs",
        &[("--symbol", "Target"), ("--symbol-id", "conflict")],
    );
    let error = research(&inv).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.project_format", 4)
    );

    inv.flags.insert("--snapshot".into(), fixture.pin.clone());
    let error = research(&inv).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.invalid_arguments", 2)
    );

    let mut no_selector = fixture.inv("source refs", &[]);
    no_selector.cwd = fixture.root.clone();
    let error = research(&no_selector).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.invalid_arguments", 2)
    );

    let bypass = fixture.inv(
        "source refs",
        &[
            ("--snapshot", &fixture.pin),
            ("--project", "absent"),
            ("--symbol", "Target"),
            ("--static-only", "true"),
        ],
    );
    let request = research(&bypass).unwrap().unwrap();
    assert_eq!(request.selection.pin.id, fixture.pin);
    assert!(!request.needs_tools());
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_project_selection_failures_remain_distinct_at_the_cli_adapter() {
    let fixture = Fixture::new();
    let missing = fixture.inv("source refs", &[("--project", "."), ("--symbol", "Target")]);
    assert_eq!(
        research(&missing).err().unwrap().code,
        "selection.project_missing"
    );

    fixture.write_project(&declaration("main"), false);
    let unlocked = fixture.inv("source refs", &[("--symbol", "Target")]);
    assert_eq!(
        research(&unlocked).err().unwrap().code,
        "selection.project_unlocked"
    );

    fixture.write_project(&declaration("classic"), true);
    let conflict = fixture.inv("source refs", &[("--symbol", "Target")]);
    assert_eq!(
        research(&conflict).err().unwrap().code,
        "selection.project_conflict"
    );
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_context_numeric_errors_precede_malformed_project_selection() {
    let fixture = Fixture::new();
    fixture.write_project(b"broken", true);
    for (flag, value, message) in [
        ("--limit", "bad", "--limit requires an integer"),
        ("--limit", "0", "--limit must be in 1..=200"),
        ("--limit", "201", "--limit must be in 1..=200"),
        (
            "--max-lines",
            "bad",
            "--max-lines must be between 1 and 2000",
        ),
        ("--max-lines", "0", "--max-lines must be between 1 and 2000"),
        (
            "--max-lines",
            "2001",
            "--max-lines must be between 1 and 2000",
        ),
        (
            "--max-bytes",
            "bad",
            "source context --max-bytes must be between 1 and 1048576",
        ),
        (
            "--max-bytes",
            "0",
            "source context --max-bytes must be between 1 and 1048576",
        ),
        (
            "--max-bytes",
            "1048577",
            "source context --max-bytes must be between 1 and 1048576",
        ),
        ("--depth", "bad", "--depth must be between 0 and 4"),
        ("--depth", "-1", "--depth must be between 0 and 4"),
        ("--depth", "5", "--depth must be between 0 and 4"),
    ] {
        let inv = fixture.inv("source context", &[("--symbol", "Target"), (flag, value)]);
        let error = research(&inv).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("command.invalid_arguments", message, 2),
            "{flag}={value}"
        );
    }
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_batch_invalid_item_is_rejected_before_missing_snapshot() {
    let fixture = Fixture::new();
    let path = fixture.batch_file(
        "invalid-batch.json",
        json!({"requests":[{"id":"bad-selector","refs":{}}]}),
    );
    let inv = fixture.inv("source batch", &[("--file", path.to_str().unwrap())]);
    let error = super::super::dispatch_source(&inv).err().unwrap();
    assert_eq!(error.code, "codebase.invalid_symbol");
    assert!(error.message.contains("request 1 (bad-selector)"));
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn static_source_rejects_session_reuse_and_explicit_release_before_pin_resolution() {
    let fixture = Fixture::new();
    let snapshot = "absent-pin";
    for options in [
        vec![
            ("--symbol", "Target"),
            ("--snapshot", snapshot),
            ("--static-only", "true"),
            ("--session-reuse", "true"),
        ],
        vec![
            ("--symbol", "Target"),
            ("--snapshot", snapshot),
            ("--static-only", "true"),
            ("--release", "missing-release"),
        ],
    ] {
        let inv = fixture.inv("source refs", &options);
        let error = research(&inv).err().unwrap();
        assert_eq!(error.code, "command.invalid_arguments");
        assert!(error.message.contains("--static-only cannot be combined"));
    }
    let path = fixture.batch_file(
        "query.json",
        json!({"requests":[{"query":{"text":"Target"}}]}),
    );
    for (flag, value) in [
        ("--session-reuse", "true"),
        ("--release", "missing-release"),
    ] {
        let inv = fixture.inv(
            "source batch",
            &[
                ("--file", path.to_str().unwrap()),
                ("--static-only", "true"),
                (flag, value),
            ],
        );
        let error = research(&inv).err().unwrap();
        assert_eq!(error.code, "command.invalid_arguments");
        assert!(error.message.contains("--static-only cannot be combined"));
    }
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn admitted_batch_keeps_its_input_when_the_file_is_replaced() {
    let fixture = Fixture::new();
    let path = fixture.batch_file(
        "replaceable.json",
        json!({"requests":[{"id":"original","query":{"text":"Target"}}]}),
    );
    let inv = fixture.inv(
        "source batch",
        &[
            ("--file", path.to_str().unwrap()),
            ("--snapshot", &fixture.pin),
        ],
    );
    let request = research(&inv).unwrap().unwrap();
    std::fs::write(path, b"replaced invalid JSON").unwrap();
    let outcome = request
        .dispatch(&context(&inv), SourceTools::default())
        .unwrap();
    assert_eq!(outcome.data["results"][0]["id"], "original");
    assert_eq!(outcome.selection["snapshot"], fixture.pin);
    assert_eq!(fixture.captures(), 1);
}

#[test]
fn admitted_research_keeps_its_pin_when_the_project_lock_is_replaced() {
    let fixture = Fixture::new();
    fixture.write_project(&declaration("main"), true);
    let inv = fixture.inv(
        "source refs",
        &[("--symbol", "Target"), ("--static-only", "true")],
    );
    let request = research(&inv).unwrap().unwrap();
    std::fs::write(
        fixture.root.join("project/lycheedev.lock.json"),
        b"replaced after selection",
    )
    .unwrap();
    let outcome = request
        .dispatch(&context(&inv), SourceTools::default())
        .unwrap();
    assert_eq!(outcome.selection["snapshot"], fixture.pin);
    assert_eq!(outcome.data["symbol"]["name"], "Target");
    assert_eq!(
        research(&inv).err().unwrap().code,
        "selection.project_format"
    );
    assert_eq!(fixture.captures(), 1);
}

#[test]
fn query_only_batch_ignores_an_unusable_explicit_release_without_runtime() {
    let fixture = Fixture::new();
    let path = fixture.batch_file(
        "query-only.json",
        json!({"requests":[{"id":"text-search","query":{"text":"needle","topic":"lua","limit":10}}]}),
    );
    let inv = fixture.inv(
        "source batch",
        &[
            ("--file", path.to_str().unwrap()),
            ("--snapshot", &fixture.pin),
            (
                "--release",
                fixture.root.join("missing-release").to_str().unwrap(),
            ),
        ],
    );
    let outcome = super::super::dispatch_source(&inv).unwrap();
    assert_eq!(outcome.data["snapshot"], fixture.pin);
    assert_eq!(outcome.data["results"][0]["id"], "text-search");
    assert!(
        !outcome.data["results"][0]["query"]["results"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(outcome.provenance.len(), 1);
    assert_eq!(fixture.captures(), 1);
}

#[test]
fn explicit_release_failure_for_batch_research_does_not_fall_back_to_static_output() {
    let fixture = Fixture::new();
    let path = fixture.batch_file(
        "research-batch.json",
        json!({"requests":[{"id":"refs","refs":{"symbol":"Target","direction":"incoming","limit":10}}]}),
    );
    let inv = fixture.inv(
        "source batch",
        &[
            ("--file", path.to_str().unwrap()),
            ("--snapshot", &fixture.pin),
            (
                "--release",
                fixture.root.join("missing-release").to_str().unwrap(),
            ),
        ],
    );
    let error = super::super::dispatch_source(&inv).err().unwrap();
    assert_eq!(error.code, "external.io");
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn private_session_server_rejects_invalid_startup_arguments_before_starting_a_process() {
    let fixture = Fixture::new();
    let valid = || {
        vec![
            "--source-native-session-server".into(),
            "--home".into(),
            fixture.home.to_string_lossy().into_owned(),
            "--parent-pid".into(),
            "1".into(),
            "--parent-created".into(),
            "1".into(),
            "--start-token".into(),
            "a".repeat(32),
        ]
    };
    let mut missing = valid();
    missing.truncate(7);
    let mut unknown = valid();
    unknown[5] = "--unknown".into();
    let mut duplicate = valid();
    duplicate[5] = "--home".into();
    let mut relative_home = valid();
    relative_home[2] = "relative-home".into();
    let mut bad_pid = valid();
    bad_pid[4] = "bad-pid".into();

    for (name, args, message) in [
        (
            "missing field",
            missing,
            "private native Source startup requires exact owned arguments",
        ),
        (
            "unknown field",
            unknown,
            "invalid private native Source startup arguments",
        ),
        (
            "duplicate field",
            duplicate,
            "invalid private native Source startup arguments",
        ),
        (
            "relative home",
            relative_home,
            "native Source home must be absolute",
        ),
        ("bad PID", bad_pid, "invalid native Source parent PID"),
    ] {
        let error = session_server(&args).err().unwrap();
        assert_eq!(error.code, "command.invalid_arguments", "{name}");
        assert_eq!(error.exit_code, 2, "{name}");
        assert_eq!(error.message, message, "{name}");
    }
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_query_and_diff_limit_defaults_and_bounds_are_admitted_by_cli() {
    let fixture = Fixture::new();
    for command in ["source query", "source diff"] {
        let mut inv = if command == "source query" {
            fixture.inv(command, &[("--snapshot", &fixture.pin)])
        } else {
            fixture.inv(command, &[("--from", &fixture.pin), ("--to", &fixture.pin)])
        };
        inv.positional = (command == "source query").then(|| "Target".into());
        for (raw, expected) in [
            (None, 50),
            (Some("1"), 1),
            (Some("50"), 50),
            (Some("200"), 200),
        ] {
            if let Some(raw) = raw {
                inv.flags.insert("--limit".into(), raw.into());
            } else {
                inv.flags.remove("--limit");
            }
            let limit = match ordinary(&inv).unwrap().request {
                SourceRequest::Query { request, .. } => request.limit,
                SourceRequest::Diff(request) => request.limit,
                _ => panic!("expected query or diff"),
            };
            assert_eq!(limit, expected, "{command}: {raw:?}");
        }
        for raw in ["0", "201", "-1", "", "not-an-integer"] {
            inv.flags.insert("--limit".into(), raw.into());
            let error = ordinary(&inv).err().unwrap();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.invalid_arguments", 2)
            );
        }
    }
    for flags in [vec![], vec![("--from", "missing-pin")]] {
        let error = ordinary(&fixture.inv("source diff", &flags)).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.invalid_arguments", 2)
        );
    }
    let missing_pin = selection::load_pinned(&fixture.home, "missing-pin").unwrap_err();
    let inv = fixture.inv(
        "source diff",
        &[
            ("--from", "missing-pin"),
            ("--to", &fixture.pin),
            ("--limit", "0"),
        ],
    );
    assert_eq!(ordinary(&inv).err().unwrap().code, missing_pin.code);
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_diff_pin_admission_retains_original_cancellation() {
    let fixture = Fixture::new();
    let inv = fixture.inv(
        "source diff",
        &[("--from", &fixture.pin), ("--to", &fixture.pin)],
    );
    let SourceRequest::Diff(request) = ordinary(&inv).unwrap().request else {
        panic!("expected a fixed diff request");
    };
    assert_eq!(request.from.snapshot(), fixture.pin);
    assert_eq!(request.to.snapshot(), fixture.pin);

    inv.control.tracked_cancel_handle().unwrap().cancel();
    // Enter the actual pin-admission branch rather than admit's outer stop
    // check: a fresh default control here would still admit both valid pins.
    let error = ordinary(&inv)
        .err()
        .expect("fixed diff reads must retain the caller's cancelled control");
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn source_prune_rejects_bad_capacity_before_storage_work() {
    let fixture = Fixture::new();
    for raw in [
        None,
        Some("-1"),
        Some(""),
        Some("not-an-integer"),
        Some("9223372036854775808"),
    ] {
        let flags = raw
            .map(|value| vec![("--target-bytes", value)])
            .unwrap_or_default();
        let error = admit(&fixture.inv("source prune", &flags)).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.invalid_arguments", 2)
        );
    }
    for (raw, expected) in [("0", 0), ("9223372036854775807", i64::MAX as u64)] {
        let request = ordinary(&fixture.inv("source prune", &[("--target-bytes", raw)])).unwrap();
        assert!(
            matches!(request.request, SourceRequest::Prune { target_bytes } if target_bytes == expected)
        );
    }
    assert_eq!(fixture.captures(), 0);
}

#[test]
fn admitted_inspect_keeps_project_context_and_fixed_source_after_lock_replacement() {
    let fixture = Fixture::new();
    fixture.write_project(&declaration("main"), true);
    let inv = fixture.inv(
        "source inspect",
        &[("--path", "Main.lua"), ("--line", "2"), ("--count", "1")],
    );
    let request = admit(&inv).unwrap();
    assert!(!request.needs_tools());
    std::fs::write(
        fixture.root.join("project/lycheedev.lock.json"),
        b"changed after admission",
    )
    .unwrap();
    let outcome = request
        .dispatch(&context(&inv), SourceTools::default())
        .unwrap();
    assert_eq!(outcome.data["text"], "  return 'needle'\n");
    assert_eq!(
        outcome.selection,
        json!({"project":fixture.root.join("project"),"snapshot":fixture.pin})
    );
    assert_eq!(outcome.provenance[0]["provenance"]["snapshot"], fixture.pin);
    assert_eq!(admit(&inv).err().unwrap().code, "selection.project_format");
    assert_eq!(fixture.captures(), 1);
}

#[test]
fn matrix_admission_rejects_mixed_selectors_and_retains_its_original_targets() {
    let fixture = Fixture::new();
    for (flag, value) in [
        ("--snapshot", fixture.pin.as_str()),
        ("--path", "addon"),
        ("--toc", "Main.toc"),
        ("--environment", fixture.pin.as_str()),
    ] {
        let inv = fixture.inv(
            "source validate",
            &[("--matrix", "missing.json"), (flag, value)],
        );
        let error = admit(&inv).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.invalid_arguments", 2)
        );
    }
    let configuration = fixture.batch_file(
        "matrix.json",
        json!({
            "path":"addon",
            "targets":[{"id":"original","toc":"Main.toc","product":"main","ref":"original-ref"}]
        }),
    );
    let inv = fixture.inv(
        "source validate",
        &[
            ("--matrix", configuration.to_str().unwrap()),
            ("--semantic", "true"),
        ],
    );
    let admitted = admit(&inv).unwrap();
    assert!(admitted.needs_tools());
    let SourceCommand::Ordinary(Ordinary {
        request: SourceRequest::Matrix(request),
        ..
    }) = admitted
    else {
        panic!("expected a matrix request");
    };
    std::fs::write(&configuration, b"replaced invalid JSON").unwrap();
    assert_eq!(request.root, fixture.root.join("addon"));
    assert!(request.semantic);
    let mut resolved = Vec::new();
    let error = lycheedev_source::matrix::validate_with_resolver(
        &context(&inv),
        &request,
        SourceTools::default(),
        &mut |target| {
            resolved.push((target.id.clone(), target.reference.clone()));
            Err(ToolkitError::new(
                "fixture.resolver_stop",
                "stop before source acquisition",
                3,
            ))
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.resolver_stop");
    assert_eq!(resolved, vec![("original".into(), "original-ref".into())]);
    assert_eq!(fixture.captures(), 0);
}
