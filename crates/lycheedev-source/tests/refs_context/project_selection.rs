//! Real public research consumers, including the mutable project-lock boundary.
use super::*;
use lycheedev_core::{Outcome, control::OperationControl, metadata::Metadata};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

const ARGUMENTS: &str =
    "source refs/context require --snapshot and exactly one of --symbol-id or --symbol";

fn write_project(f: &Fixture, pin: &str) -> PathBuf {
    let project = f.home.join("project");
    fs::create_dir_all(project.join("nested")).unwrap();
    let pin = selection::load_pinned(&f.home, pin).unwrap();
    fs::write(
        project.join("lycheedev.json"),
        serde_json::to_vec(&json!({
            "schema":"lycheedev.project.v1", "product":pin.source.as_ref().unwrap().product,
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        project.join("lycheedev.lock.json"),
        serde_json::to_vec(&json!({
            "schema":"lycheedev.project-lock.v1", "selection":pin,
        }))
        .unwrap(),
    )
    .unwrap();
    project
}
fn project_inv(f: &Fixture, command: &str) -> Invocation {
    let mut inv = invoke(
        f,
        command,
        &[("--symbol", "Target"), ("--static-only", "true")],
    );
    inv.flags.remove("--snapshot");
    inv.cwd = f.home.join("project/nested");
    inv
}
fn dispatch(inv: &Invocation) -> lycheedev_core::Result<Outcome> {
    // Same SourceTools/native-services boundary as the public CLI, even in
    // static mode. No private relation resolver is substituted for the consumer.
    lycheedev_source::dispatch_with_tools(inv, SourceTools::default())
}
fn assert_capture(f: &Fixture, result: &Outcome, pin: &str) {
    assert_eq!(result.selection["snapshot"], pin);
    assert_eq!(result.data["snapshot"], pin);
    let bytes = evidence::read_blob(&f.home, &result.provenance[0]["blob"], 1 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        result.data
    );
}
fn replacement_pin(f: &Fixture) -> String {
    let mirror = git::mirror(&f.home, "elvui");
    let old = selection::load_pinned(&f.home, &f.pin).unwrap();
    let source = old.source.as_ref().unwrap();
    let tree = Command::new("git")
        .arg(format!("--git-dir={}", mirror.display()))
        .args(["rev-parse", &format!("{}^{{tree}}", source.exact_commit)])
        .output()
        .unwrap();
    assert!(tree.status.success());
    let tree = String::from_utf8(tree.stdout).unwrap();
    let output = Command::new("git")
        .arg(format!("--git-dir={}", mirror.display()))
        .args([
            "commit-tree",
            tree.trim(),
            "-p",
            &source.exact_commit,
            "-m",
            "replacement pin",
        ])
        .env("GIT_AUTHOR_NAME", "Research Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Research Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut spec = old.spec;
    spec.source.as_mut().unwrap().exact_commit =
        String::from_utf8(output.stdout).unwrap().trim().into();
    selection::pin_selection(&f.home, spec).unwrap().id
}

#[test]
fn project_refs_and_context_match_explicit_snapshot_with_exact_evidence() {
    let f = fixture();
    write_project(&f, &f.pin);
    for command in ["source refs", "source context"] {
        let mut inv = project_inv(&f, command);
        let nearest = dispatch(&inv).unwrap();
        assert_capture(&f, &nearest, &f.pin);
        inv.flags.insert("--project".into(), "..".into());
        let relative = dispatch(&inv).unwrap();
        assert_eq!(nearest.data, relative.data);
        inv.flags.insert("--snapshot".into(), f.pin.clone());
        let explicit = dispatch(&inv).unwrap();
        assert_eq!(nearest.data, explicit.data);
        assert!(
            !nearest.complete,
            "static candidates cannot claim semantic coverage"
        );
        assert_eq!(nearest.data["coverage"]["semantic"], "unavailable");
    }
}

#[test]
fn project_research_explicit_snapshot_ignores_invalid_project_and_nearer_declaration() {
    let f = fixture();
    let project = write_project(&f, &f.pin);
    fs::write(project.join("nested/lycheedev.json"), b"broken").unwrap();
    for command in ["source refs", "source context"] {
        let mut inv = project_inv(&f, command);
        inv.flags.insert("--snapshot".into(), f.pin.clone());
        inv.flags.insert("--project".into(), "absent".into());
        let result = dispatch(&inv).unwrap();
        assert_capture(&f, &result, &f.pin);
    }
}

#[test]
fn project_research_reselects_new_lock_rejects_old_cursor_and_restores_original() {
    let f = fixture();
    write_project(&f, &f.pin);
    let replacement = replacement_pin(&f);
    for command in ["source refs", "source context"] {
        write_project(&f, &f.pin);
        let mut inv = project_inv(&f, command);
        inv.flags.insert("--limit".into(), "1".into());
        if command == "source context" {
            inv.flags.insert("--max-bytes".into(), "8".into());
        }
        let original = dispatch(&inv).unwrap();
        let cursor = original.data["nextCursor"]
            .as_str()
            .expect("bounded continuation")
            .to_owned();
        write_project(&f, &replacement);
        let changed = dispatch(&inv).unwrap();
        assert_capture(&f, &changed, &replacement);
        assert_ne!(changed.data["commit"], original.data["commit"]);
        inv.flags.insert("--cursor".into(), cursor);
        assert_eq!(
            dispatch(&inv).unwrap_err().code,
            "codebase.invalid_search_cursor"
        );
        write_project(&f, &f.pin);
        let restored = dispatch(&inv).unwrap();
        assert_capture(&f, &restored, &f.pin);
        assert_ne!(
            restored.data, original.data,
            "restored cursor advances the original query"
        );
    }
}

#[test]
fn project_research_nearer_malformed_lock_and_selector_error_priority_are_preserved() {
    let f = fixture();
    let project = write_project(&f, &f.pin);
    fs::write(project.join("nested/lycheedev.json"), b"broken").unwrap();
    for command in ["source refs", "source context"] {
        let mut inv = project_inv(&f, command);
        inv.flags.insert("--symbol-id".into(), "conflicting".into());
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("selection.project_format", 4)
        );
        inv.flags.insert("--snapshot".into(), f.pin.clone());
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("command.invalid_arguments", ARGUMENTS, 2)
        );
    }
}

#[test]
fn project_research_missing_unlocked_and_conflicting_projects_keep_specific_failures() {
    let f = fixture();
    for command in ["source refs", "source context"] {
        let mut inv = project_inv(&f, command);
        inv.cwd = f.home.clone();
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("command.invalid_arguments", ARGUMENTS, 2)
        );
        inv.flags.insert("--project".into(), ".".into());
        assert_eq!(
            dispatch(&inv).unwrap_err().code,
            "selection.project_missing"
        );
    }
    let project = write_project(&f, &f.pin);
    fs::remove_file(project.join("lycheedev.lock.json")).unwrap();
    for command in ["source refs", "source context"] {
        assert_eq!(
            dispatch(&project_inv(&f, command)).unwrap_err().code,
            "selection.project_unlocked"
        );
    }
    write_project(&f, &f.pin);
    fs::write(
        project.join("lycheedev.json"),
        br#"{"schema":"lycheedev.project.v1","product":"classic"}"#,
    )
    .unwrap();
    for command in ["source refs", "source context"] {
        assert_eq!(
            dispatch(&project_inv(&f, command)).unwrap_err().code,
            "selection.project_conflict"
        );
    }
}

#[test]
fn project_context_numeric_errors_precede_malformed_project_selection() {
    let f = fixture();
    let project = write_project(&f, &f.pin);
    fs::write(project.join("lycheedev.json"), b"broken").unwrap();
    // Fixed Go323 parses these typed options before entry.go selects a project.
    // Preserve the existing public consumer messages as well as that first error.
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
        let mut inv = project_inv(&f, "source context");
        inv.flags.insert(flag.into(), value.into());
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("command.invalid_arguments", message, 2),
            "{flag}={value}"
        );
    }
}

#[test]
fn project_research_authenticates_persisted_pin_instead_of_trusting_lock_copy() {
    let f = fixture();
    write_project(&f, &f.pin);
    let mut pin = selection::load_pinned(&f.home, &f.pin).unwrap();
    pin.spec.source.as_mut().unwrap().exact_commit = "a".repeat(40);
    let mut metadata = Metadata::open(&f.home, true).unwrap();
    let key = format!("pin/{}", f.pin);
    let generation = metadata.generation(&key).unwrap();
    metadata.put(&key, &pin, generation).unwrap();
    metadata.close().unwrap();
    for command in ["source refs", "source context"] {
        let error = dispatch(&project_inv(&f, command)).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("command.failed", "selection.pin_integrity", 5)
        );
    }
}

#[test]
fn project_research_keeps_original_cancel_and_absolute_deadline_before_selection() {
    let f = fixture();
    let project = write_project(&f, &f.pin);
    fs::write(project.join("lycheedev.json"), b"broken").unwrap();
    for command in ["source refs", "source context"] {
        let mut inv = project_inv(&f, command);
        let cancellation = inv.control.tracked_cancel_handle().unwrap();
        let deadline = inv.control.deadline();
        cancellation.cancel();
        assert_eq!(dispatch(&inv).unwrap_err().code, "command.cancelled");
        assert_eq!(inv.control.deadline(), deadline);
        inv.control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
        assert_eq!(dispatch(&inv).unwrap_err().code, "command.deadline");
    }
}

#[test]
fn project_research_does_not_relax_static_or_provider_constraints() {
    let f = fixture();
    write_project(&f, &f.pin);
    for command in ["source refs", "source context"] {
        let mut inv = project_inv(&f, command);
        inv.flags.insert("--release".into(), "unverified".into());
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(
            error.message,
            "--static-only cannot be combined with --release or --session-reuse"
        );
        inv.flags.remove("--static-only");
        assert_eq!(
            dispatch(&inv).unwrap_err().code,
            "luals.research_provider_unavailable"
        );
        inv.flags.remove("--release");
        inv.flags.insert("--session-reuse".into(), "true".into());
        assert_eq!(
            dispatch(&inv).unwrap_err().code,
            "luals.session_unavailable"
        );
    }
}

fn cli(executable: &Path, release: &Path, inv: &Invocation) -> Value {
    let mut command = Command::new(executable);
    command
        .current_dir(&inv.cwd)
        .args(inv.command.split(' '))
        .args(["--home", inv.home.to_str().unwrap(), "--format", "json"]);
    for (key, value) in &inv.flags {
        command.arg(key);
        if value != "true" {
            command.arg(value);
        }
    }
    if !inv.flag_bool("--static-only") {
        command.args(["--release", release.to_str().unwrap()]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["schema"], "lycheedev.result.v1");
    envelope["result"].clone()
}

#[test]
#[ignore = "requires exact packaged CLI and release; exercises the real native worker process"]
fn project_research_actual_cli_static_and_native_use_project_selection_and_reselect_identity() {
    let executable =
        PathBuf::from(std::env::var_os("LYCHEEDEV_PROJECT_SELECTION_EXE").expect("sealed CLI"));
    let release = PathBuf::from(
        std::env::var_os("LYCHEEDEV_PROJECT_SELECTION_RELEASE").expect("matching release"),
    );
    let f = fixture();
    write_project(&f, &f.pin);
    let replacement = replacement_pin(&f);
    for command in ["source refs", "source context"] {
        for native in [false, true] {
            write_project(&f, &f.pin);
            let mut inv = project_inv(&f, command);
            inv.flags.insert("--project".into(), "..".into());
            if native {
                inv.flags.remove("--static-only");
            }
            let project = cli(&executable, &release, &inv);
            assert_eq!(project["snapshot"], f.pin);
            inv.flags.insert("--snapshot".into(), f.pin.clone());
            inv.flags.insert("--project".into(), "absent".into());
            let explicit = cli(&executable, &release, &inv);
            for field in [
                "snapshot",
                "commit",
                "symbol",
                "relations",
                "snippets",
                "loadEvidence",
                "coverage",
                "complete",
                "truncated",
            ] {
                assert_eq!(
                    project[field], explicit[field],
                    "{command} native={native}: {field}"
                );
            }
            if native {
                assert_eq!(project["semanticRuntime"]["engine"], "native-common-source");
                assert!(project["semanticAnalysis"].is_object());
                assert_eq!(project["nativeResearch"]["lifecycle"]["retired"], true);
            }
            inv.flags.remove("--snapshot");
            inv.flags.remove("--project");
            write_project(&f, &replacement);
            let changed = cli(&executable, &release, &inv);
            assert_eq!(changed["snapshot"], replacement);
            assert_ne!(changed["commit"], project["commit"]);
        }
    }
}
