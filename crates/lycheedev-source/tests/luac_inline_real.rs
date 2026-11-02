use lycheedev_source::request::{AddonValidationRequest, SelectedSource, SourceContext, SourceRequest};
use lycheedev_core::{
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
    let runtime = Runtime::verified_with_ordered_control(&stage, manifest, &Default::default()).unwrap();
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
    let ctx = SourceContext {
        home: home.clone(),
        cwd: home.clone(),
        control: Default::default(),
    };
    let request = SourceRequest::Validate(AddonValidationRequest {
        selection: SelectedSource::new(pin.clone()).unwrap(),
        root: root.clone(),
        toc: "Main.toc".into(),
        semantic: false,
        environment: None,
        locator: format!("{}/Main.toc", root.display()),
    });
    let tools = || SourceTools {
        luac: Some(&runtime),
    };
    let cycle = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap_err();
    assert_eq!(cycle.code, "codebase.addon_static_check_failed");
    let single_cycle=&cycle.details["result"];
    assert_eq!(single_cycle["staticValid"],false);
    assert_eq!(single_cycle["load"]["loadValid"],false);
    assert_eq!(single_cycle["complete"],false);
    assert_eq!(single_cycle["syntaxCompilation"]["passed"],true);
    assert!(single_cycle["load"]["issues"].as_array().unwrap().iter().any(|issue|issue["code"]=="load_cycle"&&issue["severity"]=="error"));
    assert_eq!(single_cycle["load"]["documents"][0]["path"], "Main.toc");
    assert_eq!(single_cycle["load"]["documents"][1]["path"], xml_path);
    let matrix_path=root.join("matrix.json");
    let source=pin.source.as_ref().unwrap();
    fs::write(&matrix_path,serde_json::to_vec(&json!({"targets":[{"id":"inline","toc":"Main.toc","product":source.product,"ref":source.exact_commit}]})).unwrap()).unwrap();
    let (configuration, matrix_root) = lycheedev_source::matrix::read_config(&matrix_path, &ctx.control).unwrap();
    let matrix_request = lycheedev_source::request::MatrixValidationRequest {
        configuration: matrix_path.clone(),
        root: matrix_root,
        targets: configuration.targets,
        semantic: false,
    };
    let good=lycheedev_source::matrix::validate_with_resolver(&ctx, &matrix_request,tools(),&mut |_|Ok(source.clone())).unwrap();
    let target=&good.data["targets"][0];
    assert_eq!(target["valid"],true,"{}",good.data);
    assert_eq!(target["syntaxCompilation"]["checkedLua"], 0);
    assert_eq!(target["syntaxCompilation"]["checkedInlineScripts"], 1);
    assert_eq!(target["loadClosure"][0]["loadOrder"],0);
    let expected_complete=good.data["valid"]==true&&target["coverage"]["unresolved"]==0&&target["sourceCoverage"]["complete"]==true;
    assert_eq!(good.complete,expected_complete);
    assert_eq!(good.provenance[0]["complete"],expected_complete);
    for code in ["circular_load_reference", "duplicate_load_reference"] {
        assert!(
            target["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == code && d["severity"] == "warning")
        );
    }
    fs::write(root.join(xml_path),legal.replace("<Include file=\"界 面.xml\"/>","")).unwrap();
    let single_good=lycheedev_source::dispatch_with_tools(&ctx, &request,tools()).unwrap();
    assert_eq!(single_good.data["staticValid"],true);
    assert_eq!(single_good.data["syntaxCompilation"]["checkedInlineScripts"],1);
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
        let error = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap_err();
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
        serde_json::to_vec_pretty(&json!({"valid":good,"singleCycle":cycle.details,"singleValid":single_good,"invalid":invalid,"sourcePin":pin}))
            .unwrap(),
    )
    .unwrap();
}
