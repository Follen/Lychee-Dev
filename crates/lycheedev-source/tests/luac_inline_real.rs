use lycheedev_core::{
    Invocation,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::{
    SourceTools, git,
    luac::{Manifest, Runtime},
    xml_inline,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
fn git_input(mirror: &Path, args: &[&str], raw: &[u8]) -> String {
    let mut child = Command::new("git")
        .arg(format!("--git-dir={}", mirror.display()))
        .args(args)
        .env("GIT_AUTHOR_NAME", "Inline fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Inline fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(raw).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
#[test]
#[ignore = "explicit bundled luac real process and Git/TOC validation regression"]
fn public_validate_inline_51_errors_original_lines_and_legacy_cycle_order() {
    let stage = PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("verified stage"));
    let release: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let manifest: Manifest = serde_json::from_value(release["runtime"].clone()).unwrap();
    let runtime = Runtime::verified(&stage, manifest).unwrap();
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../.tmp/rust-source-inline-real/home-{}",
        std::process::id()
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
    let blob = git_input(
        &mirror,
        &["hash-object", "-w", "--stdin"],
        b"local baseline = true\n",
    );
    let tree = git_input(
        &mirror,
        &["mktree"],
        format!("100644 blob {blob}\tApi.lua\n").as_bytes(),
    );
    let commit = git_input(
        &mirror,
        &["commit-tree", &tree],
        b"immutable inline validation fixture\n",
    );
    let source = SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: "refs/heads/live".into(),
        exact_commit: commit,
        parser_revision: "source-v1".into(),
    };
    let pin = selection::pin_selection(
        &home,
        SelectionSpec {
            source: Some(source),
            ..Default::default()
        },
    )
    .unwrap();
    let root = home.join("中文 空格 addon");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("Main.toc"),
        "## Title: Inline fixture\n界 面.xml\n界 面.xml\n",
    )
    .unwrap();
    let xml_path = "界 面.xml";
    let legal = "<Ui>\n<Include file=\"界 面.xml\"/>\n<Frame><Scripts><OnLoad>\nlocal x = 1 &lt; 2\n<![CDATA[local arg = ...; self.value = '中文']]>&#10;local s = &quot;text&quot;\n</OnLoad></Scripts></Frame></Ui>";
    fs::write(root.join(xml_path), legal).unwrap();
    let inv = Invocation {
        command: "source validate".into(),
        positional: None,
        flags: BTreeMap::from([
            ("--snapshot".into(), pin.id.clone()),
            ("--path".into(), root.to_string_lossy().into_owned()),
            ("--toc".into(), "Main.toc".into()),
        ]),
        home: home.clone(),
        cwd: home.clone(),
        control: Default::default(),
    };
    let tools = || SourceTools {
        luac: Some(&runtime),
        luals: None,
    };
    let good = lycheedev_source::dispatch_with_tools(&inv, tools()).unwrap();
    assert_eq!(good.data["valid"], true, "{}", good.data);
    assert_eq!(good.data["syntaxCompilation"]["checkedLua"], 0);
    assert_eq!(good.data["syntaxCompilation"]["checkedInlineScripts"], 1);
    assert_eq!(good.data["loadClosure"][0]["loadOrder"], 0);
    for code in ["circular_load_reference", "duplicate_load_reference"] {
        assert!(
            good.data["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == code && d["severity"] == "warning")
        );
    }
    let mut invalid = vec![];
    for (expression, expected_line) in [
        ("goto done", 4),
        ("self.value = 1 &amp; 2", 4),
        ("local broken =", 5),
    ] {
        let xml = format!(
            "<Ui>\n<Frame><Scripts>\n<OnLoad>\n{expression}\n</OnLoad></Scripts></Frame></Ui>"
        );
        fs::write(root.join(xml_path), &xml).unwrap();
        let error = lycheedev_source::dispatch_with_tools(&inv, tools()).unwrap_err();
        assert_eq!(error.exit_code, 4);
        let diagnostics = error.details["result"]["diagnostics"].as_array().unwrap();
        let syntax = diagnostics
            .iter()
            .find(|d| d["engine"] == "luac-5.1.5")
            .unwrap();
        assert_eq!(syntax["file"], xml_path);
        assert_eq!(syntax["line"], expected_line);
        assert_eq!(syntax["columnKnown"], false);
        assert_eq!(syntax["inlineHandler"], "OnLoad");
        assert!(!syntax.as_object().unwrap().contains_key("rawError"));
        invalid.push(error.details);
    }
    assert!(
        xml_inline::extract(xml_path, legal.as_bytes()).unwrap()[0]
            .source_bytes()
            .starts_with(b"return function(self, ...)")
    );
    fs::write(
        home.parent().unwrap().join("inline-public-report.json"),
        serde_json::to_vec_pretty(&json!({"valid":good,"invalid":invalid,"sourcePin":pin}))
            .unwrap(),
    )
    .unwrap();
}
