//! Test-only complete catalog/source fixture; never a production backend.
use lycheedev_core::{
    Result,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::{environment, git, index};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
fn object(mirror: &Path, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new("git")
        .arg(format!("--git-dir={}", mirror.display()))
        .args(args)
        .env("GIT_AUTHOR_NAME", "Broker Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Broker Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-10-08T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-10-08T00:00:00Z")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}
fn tree(mirror: &Path, files: &BTreeMap<String, String>) -> String {
    let mut dirs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut entries = String::new();
    for (path, text) in files {
        if let Some((dir, rest)) = path.split_once('/') {
            dirs.entry(dir.into())
                .or_default()
                .insert(rest.into(), text.clone());
        } else {
            let id = object(mirror, &["hash-object", "-w", "--stdin"], text.as_bytes());
            entries.push_str(&format!("100644 blob {id}\t{path}\n"));
        }
    }
    for (name, files) in dirs {
        let id = tree(mirror, &files);
        entries.push_str(&format!("040000 tree {id}\t{name}\n"));
    }
    object(mirror, &["mktree"], entries.as_bytes())
}
fn main() -> Result<()> {
    let home = std::path::absolute(PathBuf::from(
        std::env::args_os().nth(1).expect("fresh fixture home"),
    ))?;
    assert!(!home.exists(), "fresh task-owned home only");
    workspace::initialize(&home)?;
    let mirror = git::mirror(&home, "wow-ui-source");
    fs::create_dir_all(mirror.parent().unwrap())?;
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&mirror)
            .output()?
            .status
            .success()
    );
    let files=BTreeMap::from([
 ("Interface/AddOns/Blizzard_APIDocumentationGenerated/Fixture.lua".into(),"local API={Name=\"Fixture\",Type=\"System\",Functions={{Name=\"FixtureAPI\",Type=\"Function\",Returns={{Name=\"v\",Type=\"number\"}}}}};APIDocumentation:AddDocumentationTable(API)\n".into()),
 ("probe.lua".into(),"function First()\n return FixtureAPI()\nend\nfunction Second()\n return FixtureAPI()\nend\nfunction Third()\n return First()\nend\n".into())]);
    let tree = tree(&mirror, &files);
    let commit = object(
        &mirror,
        &["commit-tree", &tree],
        b"complete broker fixture\n",
    );
    let source = SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: "fixture".into(),
        exact_commit: commit,
        parser_revision: "source-v1".into(),
    };
    let pin = selection::pin_selection(
        &home,
        SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )?;
    let index = index::build_with_control(&home, &source, &Default::default())?;
    let env = environment::from_pin(&home, &source, &Default::default())?;
    assert!(env.coverage.complete, "{:?}", env.coverage.gaps);
    let report = json!({"schema":"lycheedev.source-broker-fixture.v1","home":home,"pin":pin,"index":index.summary,"environment":env.coverage,"files":files,"scope":"complete synthetic fixed Git source/catalog; not a real-client baseline or game acceptance"});
    fs::write(
        home.parent().unwrap().join("prepared.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
