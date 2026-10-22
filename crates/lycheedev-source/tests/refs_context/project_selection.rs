//! Real public research consumers, including the mutable project-lock boundary.
use super::*;
use lycheedev_core::{Outcome, control::OperationControl, metadata::Metadata};
use lycheedev_source::request::{
    RelationDirection, RelationRequest, ResearchMode, ResearchSelection, SymbolSelector,
};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

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
fn project_options(f: &Fixture) -> BTreeMap<String, String> {
    let mut options = cli_options(f, &[("--symbol", "Target"), ("--static-only", "true")]);
    options.remove("--snapshot");
    options
}
fn selected(f: &Fixture, pin: &str) -> ResearchSelection {
    ResearchSelection {
        pin: selection::load_pinned(&f.home, pin).unwrap(),
        environment: None,
    }
}
fn refs_request(selector: &str, limit: usize, cursor: &str) -> RelationRequest {
    RelationRequest {
        selector: SymbolSelector::Name(selector.into()),
        direction: RelationDirection::Both,
        limit,
        cursor: cursor.into(),
    }
}
fn typed_refs(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    selector: &str,
    limit: usize,
    cursor: &str,
) -> lycheedev_core::Result<Outcome> {
    refs::dispatch(
        ctx,
        selection,
        &refs_request(selector, limit, cursor),
        ResearchMode::Static,
    )
}
fn typed_context(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    selector: &str,
    limit: usize,
    max_bytes: usize,
    cursor: &str,
) -> lycheedev_core::Result<Outcome> {
    context::dispatch(
        ctx,
        selection,
        &context_request(selector, limit, max_bytes, 120, 1, cursor, false),
        ResearchMode::Static,
    )
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
fn typed_refs_and_context_use_fixed_selection_with_exact_evidence() {
    let f = fixture();
    write_project(&f, &f.pin);
    let selected = selected(&f, &f.pin);
    let ctx = source_context(&f);
    let nearest = typed_refs(&ctx, &selected, "Target", 50, "").unwrap();
    assert_capture(&f, &nearest, &f.pin);
    assert!(
        !nearest.complete,
        "static candidates cannot claim semantic coverage"
    );
    assert_eq!(nearest.data["coverage"]["semantic"], "unavailable");
    let context = typed_context(&ctx, &selected, "Target", 50, 16 << 10, "").unwrap();
    assert_capture(&f, &context, &f.pin);
    assert_eq!(context.selection["snapshot"], selected.pin.id);
    let nested_ctx = SourceContext {
        cwd: f.home.join("project/nested"),
        ..ctx
    };
    assert_eq!(
        typed_refs(&nested_ctx, &selected, "Target", 50, "")
            .unwrap()
            .data,
        nearest.data
    );
}

#[test]
fn typed_fixed_selection_is_independent_of_nearer_project_files() {
    let f = fixture();
    let project = write_project(&f, &f.pin);
    fs::write(project.join("nested/lycheedev.json"), b"broken").unwrap();
    let selected = selected(&f, &f.pin);
    let ctx = source_context(&f);
    let refs = typed_refs(&ctx, &selected, "Target", 50, "").unwrap();
    assert_capture(&f, &refs, &f.pin);
    let context = typed_context(&ctx, &selected, "Target", 50, 16 << 10, "").unwrap();
    assert_capture(&f, &context, &f.pin);
}

#[test]
fn typed_selection_identity_rejects_old_cursor_and_restores_original() {
    let f = fixture();
    write_project(&f, &f.pin);
    let replacement = replacement_pin(&f);
    let ctx = source_context(&f);
    let original_selection = selected(&f, &f.pin);
    let original = typed_refs(&ctx, &original_selection, "Target", 1, "").unwrap();
    let cursor = original.data["nextCursor"]
        .as_str()
        .expect("bounded continuation")
        .to_owned();
    let changed_selection = selected(&f, &replacement);
    let changed = typed_refs(&ctx, &changed_selection, "Target", 1, "").unwrap();
    assert_capture(&f, &changed, &replacement);
    assert_ne!(changed.data["commit"], original.data["commit"]);
    assert_eq!(
        typed_refs(&ctx, &changed_selection, "Target", 1, &cursor)
            .unwrap_err()
            .code,
        "codebase.invalid_search_cursor"
    );
    let restored = typed_refs(&ctx, &original_selection, "Target", 1, &cursor).unwrap();
    assert_capture(&f, &restored, &f.pin);
    assert_ne!(
        restored.data, original.data,
        "restored cursor advances the original query"
    );
}

#[test]
fn typed_selection_authenticates_persisted_pin_before_use() {
    let f = fixture();
    write_project(&f, &f.pin);
    let mut pin = selection::load_pinned(&f.home, &f.pin).unwrap();
    pin.spec.source.as_mut().unwrap().exact_commit = "a".repeat(40);
    let mut metadata = Metadata::open(&f.home, true).unwrap();
    let key = format!("pin/{}", f.pin);
    let generation = metadata.generation(&key).unwrap();
    metadata.put(&key, &pin, generation).unwrap();
    metadata.close().unwrap();
    assert_eq!(
        selection::load_pinned(&f.home, &f.pin).unwrap_err().code,
        "selection.pin_integrity"
    );
}

#[test]
fn typed_source_context_keeps_cancellation_and_absolute_deadline() {
    let f = fixture();
    let selected = selected(&f, &f.pin);
    let mut ctx = source_context(&f);
    let cancellation = ctx.control.tracked_cancel_handle().unwrap();
    let deadline = ctx.control.deadline();
    cancellation.cancel();
    assert_eq!(
        typed_refs(&ctx, &selected, "Target", 50, "")
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(ctx.control.deadline(), deadline);
    ctx.control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        typed_refs(&ctx, &selected, "Target", 50, "")
            .unwrap_err()
            .code,
        "command.deadline"
    );
}

#[test]
fn typed_static_mode_works_and_native_mode_requires_services() {
    let f = fixture();
    write_project(&f, &f.pin);
    let selected = selected(&f, &f.pin);
    let ctx = source_context(&f);
    let static_result = typed_refs(&ctx, &selected, "Target", 50, "").unwrap();
    assert_eq!(static_result.selection["snapshot"], f.pin);
    assert_eq!(
        typed_context(&ctx, &selected, "Target", 50, 16 << 10, "")
            .unwrap()
            .selection["snapshot"],
        f.pin
    );
    assert_eq!(
        refs::dispatch(
            &ctx,
            &selected,
            &refs_request("Target", 50, ""),
            ResearchMode::OneShot,
        )
        .unwrap_err()
        .code,
        "source.native_unavailable"
    );
    assert_eq!(
        refs::dispatch(
            &ctx,
            &selected,
            &refs_request("Target", 50, ""),
            ResearchMode::Session,
        )
        .unwrap_err()
        .code,
        "source.native_unavailable"
    );
}

fn cli(executable: &Path, release: &Path, f: &Fixture, cwd: &Path, name: &str, options: &BTreeMap<String, String>) -> Value {
    cli_envelope(executable, release, f, cwd, name, options)["result"].clone()
}
pub(super) fn cli_envelope(executable: &Path, release: &Path, f: &Fixture, cwd: &Path, name: &str, options: &BTreeMap<String, String>) -> Value {
    let mut command = Command::new(executable);
    command
        .current_dir(cwd)
        .args(name.split(' '))
        .args(["--home", f.home.to_str().unwrap(), "--format", "json"]);
    for (key, value) in options {
        command.arg(key);
        if value != "true" {
            command.arg(value);
        }
    }
    if !options.get("--static-only").is_some_and(|value| value == "true") {
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
    assert_eq!(envelope["ok"], true);
    envelope
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
            let mut options = project_options(&f);
            let cwd = f.home.join("project/nested");
            options.insert("--project".into(), "..".into());
            if native {
                options.remove("--static-only");
            }
            let project = cli(&executable, &release, &f, &cwd, command, &options);
            assert_eq!(project["snapshot"], f.pin);
            options.insert("--snapshot".into(), f.pin.clone());
            options.insert("--project".into(), "absent".into());
            let explicit = cli(&executable, &release, &f, &cwd, command, &options);
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
            options.remove("--snapshot");
            options.remove("--project");
            write_project(&f, &replacement);
            let changed = cli(&executable, &release, &f, &cwd, command, &options);
            assert_eq!(changed["snapshot"], replacement);
            assert_ne!(changed["commit"], project["commit"]);
        }
    }
}
