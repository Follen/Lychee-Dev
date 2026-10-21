use lycheedev_core::{
    Invocation, evidence,
    selection::{self, SelectionSpec, SourcePin},
    storage, workspace,
};
use lycheedev_source::{diff, git, index, prune};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
struct Fixture {
    home: PathBuf,
    mirror: PathBuf,
}
static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl Fixture {
    fn new() -> Self {
        // Owner-isolated TEMP plus PID/counter keeps each fixture independent.
        // Git's worktree GIT_DIR check still has a PATH_MAX - 40 bound even
        // with core.longpaths, so keep only this non-business basename short.
        let home = std::env::temp_dir().join(format!(
            "dp-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        let mirror = git::mirror(&home, "wow-ui-source");
        fs::create_dir_all(mirror.parent().unwrap()).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--bare", mirror.to_str().unwrap()])
                .output()
                .unwrap()
                .status
                .success()
        );
        Self { home, mirror }
    }
    fn run(&self, input: &str, args: &[&str]) -> String {
        let mut child = Command::new("git")
            .arg(format!("--git-dir={}", self.mirror.display()))
            .args(args)
            .env("GIT_AUTHOR_NAME", "Diff Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Diff Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn pin(&self, files: &[(&str, &str)], message: &str) -> SourcePin {
        let mut entries = BTreeMap::new();
        for (path, text) in files {
            let hash = self.run(text, &["hash-object", "-w", "--stdin"]);
            entries.insert(*path, format!("100644 blob {hash}\t{path}\n"));
        }
        let tree = self.run(&entries.values().cloned().collect::<String>(), &["mktree"]);
        let commit = self.run(message, &["commit-tree", &tree]);
        SourcePin {
            repository: "wow-ui-source".into(),
            product: "retail".into(),
            requested_ref: "refs/heads/live".into(),
            exact_commit: commit,
            parser_revision: "source-v1".into(),
        }
    }
    fn invoke(&self, command: &str, flags: &[(&str, &str)]) -> Invocation {
        Invocation {
            home: self.home.clone(),
            cwd: self.home.clone(),
            command: command.into(),
            positional: None,
            control: Default::default(),
            flags: flags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
#[test]
fn diff_preserves_duplicate_declarations_signature_lines_and_original_blob_proofs() {
    let f = Fixture::new();
    let left = f.pin(
        &[
            ("a.lua", "function Shared()\nend\n"),
            ("b.lua", "function Shared()\nend\n"),
            ("changed.lua", "function Changed(value)\nend\n"),
            ("moved.lua", "function Moved()\nend\n"),
            ("removed.lua", "function Removed()\nend\n"),
        ],
        "before",
    );
    let right = f.pin(
        &[
            ("a.lua", "function Shared()\nend\n"),
            ("b.lua", "function Shared(argument)\nend\n"),
            ("changed.lua", "function Changed(other)\nend\n"),
            ("moved.lua", "\n\nfunction Moved()\nend\n"),
            ("added.lua", "function Added()\nend\n"),
        ],
        "after",
    );
    assert_eq!(
        diff::compare(&f.home, &left, &right, 20).unwrap_err().code,
        "codebase.index_not_ready"
    );
    index::build(&f.home, &left).unwrap();
    index::build(&f.home, &right).unwrap();
    let result = diff::compare(&f.home, &left, &right, 20).unwrap();
    assert_eq!(result["documentChanges"], 5);
    assert_eq!(result["declarationChanges"], 5);
    let shared = result["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "Shared")
        .unwrap();
    assert_eq!(shared["before"].as_array().unwrap().len(), 2);
    assert_eq!(shared["after"].as_array().unwrap().len(), 2);
    assert_ne!(
        shared["before"][1]["signature"],
        shared["after"][1]["signature"]
    );
    let moved = result["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "Moved")
        .unwrap();
    assert_eq!(moved["before"][0]["line"], 1);
    assert_eq!(moved["after"][0]["line"], 3);
    let proof = &result["sourceProofs"][0]["after"];
    let (raw, object) = git::blob(&f.home, &right, "added.lua", 1024).unwrap();
    assert_eq!(proof["commit"], right.exact_commit);
    assert_eq!(proof["gitObject"], object);
    assert_eq!(proof["sha256"], storage::sha256(&raw));
    let bounded = diff::compare(&f.home, &left, &right, 1).unwrap();
    assert_eq!(bounded["documents"].as_array().unwrap().len(), 1);
    assert_eq!(bounded["declarations"].as_array().unwrap().len(), 1);
    assert_eq!(bounded["documentChanges"], 5);
    assert_eq!(bounded["truncated"], true);
    let from = selection::pin_selection(
        &f.home,
        SelectionSpec {
            source: Some(left),
            ..Default::default()
        },
    )
    .unwrap();
    let to = selection::pin_selection(
        &f.home,
        SelectionSpec {
            source: Some(right),
            ..Default::default()
        },
    )
    .unwrap();
    let out = diff::dispatch(&f.invoke(
        "source diff",
        &[("--from", &from.id), ("--to", &to.id), ("--limit", "1")],
    ))
    .unwrap();
    assert!(!out.complete);
    assert_eq!(out.provenance[0]["provenance"]["baseSnapshot"], from.id);
    let captured = evidence::read_blob(&f.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&captured).unwrap(),
        out.data
    );
}
#[test]
fn diff_same_commit_no_change_and_incomplete_syntax_cannot_prove_absence() {
    let f = Fixture::new();
    let valid = f.pin(&[("valid.lua", "function Kept() end\n")], "valid");
    index::build(&f.home, &valid).unwrap();
    let same = diff::compare(&f.home, &valid, &valid, 1).unwrap();
    assert_eq!(same["documentChanges"], 0);
    assert_eq!(same["declarationChanges"], 0);
    assert_eq!(same["truncated"], false);
    let broken = f.pin(
        &[
            ("valid.lua", "function Kept() end\n"),
            ("error.lua", "function Broken(\n"),
        ],
        "broken",
    );
    index::build(&f.home, &broken).unwrap();
    let fault = diff::compare(&f.home, &valid, &broken, 10).unwrap_err();
    assert_eq!(fault.code, "codebase.incomplete_index");
    assert_eq!(fault.details["coverage"]["to"]["complete"], false);
    let mut other = valid.clone();
    other.repository = "elvui".into();
    assert_eq!(
        diff::compare(&f.home, &valid, &other, 10).unwrap_err().code,
        "codebase.incompatible_comparison"
    );
    assert_eq!(
        diff::dispatch(&f.invoke("source diff", &[]))
            .unwrap_err()
            .exit_code,
        2
    );
    assert_eq!(
        same["analysis"]["runtimeCompatibility"],
        json!("not-evaluated")
    );
}

fn checkout(f: &Fixture, pin: &SourcePin) -> PathBuf {
    let path = f
        .home
        .join("source/v1/worktrees")
        .join(&pin.repository)
        .join(&pin.exact_commit);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    f.run(
        "",
        &[
            // Owned TEMP plus the immutable commit makes Git's admin path long.
            // Keep this invocation-local: prune rejects extra repository config.
            "-c",
            "core.longpaths=true",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.eol=lf",
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            &pin.exact_commit,
        ],
    );
    let manifest = prune::verify(&f.home, pin, &path).unwrap();
    storage::atomic_json(
        &f.home
            .join("source/v1/manifests")
            .join(&pin.repository)
            .join(format!("{}.json", pin.exact_commit)),
        &manifest,
    )
    .unwrap();
    path
}
#[test]
fn prune_keeps_active_owner_and_immutable_evidence_then_reclaims_only_verified_checkout() {
    use lycheedev_core::storage::ExclusiveLock;
    let f = Fixture::new();
    let source = f.pin(
        &[
            ("a.lua", "function Original() end\n"),
            ("note.txt", "original"),
        ],
        "prune",
    );
    let pin = selection::pin_selection(
        &f.home,
        SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let capture = evidence::commit_capture(
        &f.home,
        b"retained bytes",
        "text/plain",
        json!({"kind":"test","locator":"outside checkout"}),
        true,
        false,
    )
    .unwrap();
    let path = checkout(&f, &source);
    let lease = ExclusiveLock::resource(
        &f.home.join("locks"),
        &format!(
            "source:v1:worktree:{}:{}",
            source.repository, source.exact_commit
        ),
    )
    .unwrap();
    let held = prune::prune(&f.home, 0).unwrap();
    assert!(!held.complete);
    assert!(held.removed.is_empty());
    assert!(held.skipped[0].ends_with(":in-use"));
    assert!(path.exists());
    drop(lease);
    fs::write(path.join("note.txt"), b"modified").unwrap();
    let modified = prune::prune(&f.home, 0).unwrap();
    assert!(modified.skipped[0].ends_with(":modified"));
    assert!(path.exists());
    fs::write(path.join("note.txt"), b"original").unwrap();
    let removed = prune::prune(&f.home, 0).unwrap();
    assert!(removed.complete);
    assert_eq!(
        removed.removed,
        vec![format!("{}:{}", source.repository, source.exact_commit)]
    );
    assert!(!path.exists());
    assert!(selection::load_pinned(&f.home, &pin.id).is_ok());
    assert_eq!(
        evidence::read_blob(&f.home, &capture["blob"], 100).unwrap(),
        b"retained bytes"
    );
    assert!(f.mirror.exists());
    assert_eq!(git::regular_tree(&f.home, &source).unwrap().len(), 2);
}
#[test]
fn prune_preserves_unknown_roots_extra_files_and_live_broker_without_ttl_stealing() {
    use lycheedev_core::storage::ExclusiveLock;
    let f = Fixture::new();
    let source = f.pin(&[("a.lua", "local x=1\n")], "unknown");
    let path = checkout(&f, &source);
    fs::write(path.join("owner.txt"), b"extra owned file").unwrap();
    let unknown = f.home.join("source/v1/worktrees/foreign");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("keep.bin"), b"owner").unwrap();
    let result = prune::prune(&f.home, 0).unwrap();
    assert!(!result.complete);
    assert!(result.skipped.iter().any(|s| s.ends_with(":modified")));
    assert!(result.skipped.iter().any(|s| s == "foreign:untrusted"));
    assert_eq!(fs::read(unknown.join("keep.bin")).unwrap(), b"owner");
    let broker = ExclusiveLock::resource(&f.home.join("locks"), "source:v1:broker-owner").unwrap();
    assert_eq!(prune::prune(&f.home, 0).unwrap_err().exit_code, 6);
    drop(broker);
    assert_eq!(
        prune::dispatch(&f.invoke("source prune", &[]))
            .unwrap_err()
            .exit_code,
        2
    );
    assert_eq!(
        prune::dispatch(&f.invoke("source prune", &[("--target-bytes", "-1")]))
            .unwrap_err()
            .exit_code,
        2
    );
    assert!(path.exists());
}
#[test]
fn prune_exact_stale_registration_and_invalid_manifest_boundaries() {
    let f = Fixture::new();
    let source = f.pin(&[("a.lua", "local x=1\n")], "stale");
    let path = checkout(&f, &source);
    let manifest_path = f
        .home
        .join("source/v1/manifests")
        .join(&source.repository)
        .join(format!("{}.json", source.exact_commit));
    fs::remove_dir_all(&path).unwrap();
    let result = prune::prune(&f.home, 0).unwrap();
    assert!(result.complete);
    assert!(result.removed[0].ends_with(":stale-manifest"));
    assert!(!manifest_path.exists());
    let raw = git::bytes(
        Some(&f.mirror),
        &["worktree", "list", "--porcelain", "-z"],
        1 << 20,
    )
    .unwrap();
    assert!(
        !String::from_utf8(raw)
            .unwrap()
            .contains(path.to_str().unwrap())
    );
    let path = checkout(&f, &source);
    let mut record: Value = storage::read_json(&manifest_path).unwrap();
    record["commit"] = json!("a".repeat(40));
    storage::atomic_json(&manifest_path, &record).unwrap();
    assert_eq!(prune::prune(&f.home, 0).unwrap_err().exit_code, 4);
    assert!(path.exists());
}

#[test]
fn prune_rejects_foreign_git_binding_and_preserves_the_other_repository() {
    let f = Fixture::new();
    let source = f.pin(&[("a.lua", "local x=1\n")], "binding");
    let path = checkout(&f, &source);
    let original = fs::read(path.join(".git")).unwrap();
    let foreign = f.home.join("foreign.git");
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("owner.bin"), b"foreign owner").unwrap();
    fs::write(
        path.join(".git"),
        format!("gitdir: {}\n", foreign.display()),
    )
    .unwrap();
    let result = prune::prune(&f.home, 0).unwrap();
    assert!(!result.complete);
    assert!(result.removed.is_empty());
    assert!(result.skipped[0].ends_with(":modified"));
    assert_eq!(
        fs::read(foreign.join("owner.bin")).unwrap(),
        b"foreign owner"
    );
    assert!(path.join("a.lua").exists());
    fs::write(path.join(".git"), original).unwrap();
    assert!(prune::prune(&f.home, 0).unwrap().complete);
}
