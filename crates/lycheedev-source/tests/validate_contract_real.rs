//! Real compiler/Git/metadata regression for the distinct external fixed reference 3.2.3
//! single-addon and matrix entrypoints. No network or game is required.
use lycheedev_source::request::{AddonValidationRequest, SelectedSource, SourceContext, SourceRequest};
use lycheedev_core::{
    evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::{
    SourceTools, git, index,
    luac::{Manifest, Runtime},
    matrix,
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
        .env("GIT_AUTHOR_NAME", "Validation fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Validation fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(raw).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn tree(mirror: &Path, entries: Vec<(&str, String)>) -> String {
    let mut entries = entries;
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let input = entries
        .into_iter()
        .map(|(name, entry)| format!("{entry}\t{name}\n"))
        .collect::<String>();
    git_input(mirror, &["mktree"], input.as_bytes())
}
fn blob(mirror: &Path, raw: &[u8]) -> String {
    format!(
        "100644 blob {}",
        git_input(mirror, &["hash-object", "-w", "--stdin"], raw)
    )
}
fn source(mirror: &Path, gaps: bool) -> SourcePin {
    let docs=blob(mirror,b"local api={Name='Good',Type='System',Namespace='C_Good',Functions={{Name='Do',Type='Function'}}}\nAPIDocumentation:AddDocumentationTable(api)\n");
    let broken = blob(mirror, b"local broken =\n");
    let mut generated = vec![("Docs.lua", docs)];
    if gaps {
        generated.push(("ZBroken.lua", broken.clone()));
    }
    let mut directory = tree(mirror, generated);
    for name in ["Blizzard_APIDocumentationGenerated", "AddOns", "Interface"] {
        directory = tree(mirror, vec![(name, format!("040000 tree {directory}"))]);
    }
    // The wrapper is now the repository root. Add its Interface entry and TOC.
    let interface = git_input(mirror, &["ls-tree", &directory], b"");
    let mut root = format!(
        "{interface}\n{}\tGame.toc\n",
        blob(mirror, b"## Interface: 120100\n")
    );
    if gaps {
        // These precede Interface in the immutable tree: the generated API
        // gap must survive beyond the public diagnostic sample and warm cache.
        for i in 0..25 {
            root.push_str(&format!("{broken}\t00-Bad{i:02}.lua\n"));
        }
    }
    let directory = git_input(mirror, &["mktree"], root.as_bytes());
    let commit = git_input(
        mirror,
        &["commit-tree", &directory],
        b"fixed validation contract fixture\n",
    );
    SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: commit.clone(),
        exact_commit: commit,
        parser_revision: "source-v1".into(),
    }
}
fn verify_capture(home: &Path, capture: &Value, result: &Value, complete: bool, kind: &str) {
    assert_eq!(capture["complete"], complete);
    assert_eq!(capture["provenance"]["kind"], kind);
    assert_eq!(
        evidence::read_capture(home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    let bytes =
        fs::read(evidence::blob_path(home, capture["blob"]["sha256"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        lycheedev_core::storage::sha256(&bytes),
        capture["blob"]["sha256"].as_str().unwrap()
    );
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), *result);
}

#[test]
#[ignore = "explicit verified bundled luac, local Git and persisted dual-entry regression"]
fn single_and_matrix_native_validation_keep_coverage_and_warm_index_gaps() {
    let stage =
        PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("verified luac stage"));
    let release: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let manifest: Manifest = serde_json::from_value(release["runtime"].clone()).unwrap();
    let runtime = Runtime::verified_with_ordered_control(&stage, manifest, &Default::default()).unwrap();
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../.tmp/rust-validate-contract-real/home-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    workspace::initialize(&home).unwrap();
    let mirror = git::mirror(&home, "wow-ui-source");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    assert!(
        Command::new("git")
            .args([
                "init",
                "--bare",
                "--object-format=sha1",
                mirror.to_str().unwrap()
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    let source = source(&mirror, false);
    let pin = selection::pin_selection(
        &home,
        SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let addon = home.join("中文 addon");
    fs::create_dir(&addon).unwrap();
    fs::write(
        addon.join("Main.toc"),
        "## Interface: 1\n## Interface: 120100\nMain.lua\n",
    )
    .unwrap();
    fs::write(
        addon.join("Main.lua"),
        "C_Good.Do(); C_Missing.Do(); C_Missing.Do()\n",
    )
    .unwrap();
    let ctx = SourceContext {
        home: home.clone(),
        cwd: home.clone(),
        control: Default::default(),
    };
    let single_request = SourceRequest::Validate(AddonValidationRequest {
        selection: SelectedSource::new(pin.clone()).unwrap(),
        root: addon.clone(),
        toc: "Main.toc".into(),
        semantic: false,
        environment: None,
        locator: format!("{}/Main.toc", addon.display()),
    });
    let tools = || SourceTools {
        luac: Some(&runtime),
    };
    let single = lycheedev_source::dispatch_with_tools(&ctx, &single_request, tools()).unwrap();
    assert_eq!(single.data["staticValid"], true);
    assert!(single.data.get("valid").is_none());
    assert_eq!(single.data["resolved"], 1);
    assert_eq!(single.data["unresolved"], 2);
    assert_eq!(single.data["references"].as_array().unwrap().len(), 3);
    assert_eq!(single.data["interfaceStatus"], "unresolved");
    assert!(!single.complete);
    assert_eq!(single.data["load"]["documents"][1]["path"], "Main.lua");
    verify_capture(
        &home,
        &single.provenance[0],
        &single.data,
        false,
        "addon-static-check",
    );
    fs::write(addon.join("Complete.toc"), "## Interface: 120100\nComplete.lua\n").unwrap();
    fs::write(addon.join("Complete.lua"), "local count = 1\n").unwrap();
    let mut semantic_request = single_request.clone();
    if let SourceRequest::Validate(request) = &mut semantic_request {
        request.toc = "Complete.toc".into(); request.semantic = true;
        request.locator = format!("{}/Complete.toc", addon.display());
    }
    let semantic_complete=lycheedev_source::dispatch_with_tools(&ctx, &semantic_request,tools()).unwrap();
    assert_eq!(semantic_complete.data["semantic"]["passed"],true);
    assert_eq!(semantic_complete.data["semantic"]["complete"],true);
    // The local commit has complete semantic inputs but no pinned client
    // Interface baseline. Semantic success cannot certify that missing fact.
    assert_eq!(semantic_complete.data["staticValid"], true);
    assert_eq!(semantic_complete.data["interfaceStatus"], "unresolved");
    assert!(!semantic_complete.complete,"{}",semantic_complete.data);
    verify_capture(&home,&semantic_complete.provenance[0],&semantic_complete.data,false,"addon-static-check");
    let warm_single = lycheedev_source::dispatch_with_tools(&ctx, &single_request, tools()).unwrap();
    assert_eq!(warm_single.data["references"], single.data["references"]);
    let matrix_path = addon.join("matrix.json");
    fs::write(&matrix_path,serde_json::to_vec(&json!({"targets":[{"id":"retail","toc":"Main.toc","product":"retail","ref":source.exact_commit}]})).unwrap()).unwrap();
    let (configuration, matrix_root) = lycheedev_source::matrix::read_config(&matrix_path, &ctx.control).unwrap();
    let matrix_request = lycheedev_source::request::MatrixValidationRequest {
        configuration: matrix_path.clone(),
        root: matrix_root,
        targets: configuration.targets,
        semantic: false,
    };
    let missing = matrix::validate_with_resolver(&ctx, &matrix_request, tools(), &mut |_| Ok(source.clone()))
        .unwrap_err();
    assert_eq!(missing.code, "codebase.addon_static_check_failed");
    let target = &missing.details["result"]["targets"][0];
    assert_eq!(target["valid"], false);
    assert!(target.get("staticValid").is_none());
    assert_eq!(target["interface"], "120100");
    assert_eq!(
        target["coverage"],
        json!({"checked":3,"resolved":3,"unresolved":0})
    );
    assert_eq!(target["loadClosure"][0]["loadOrder"], 0);
    assert_eq!(
        target["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == "api_not_found")
            .count(),
        1
    );
    verify_capture(
        &home,
        &missing.details["captures"][0],
        &missing.details["result"],
        false,
        "addon-matrix-validate",
    );
    fs::write(
        addon.join("Main.lua"),
        "LocalExtension(); LocalExtension()\n",
    )
    .unwrap();
    let partial =
        matrix::validate_with_resolver(&ctx, &matrix_request, tools(), &mut |_| Ok(source.clone())).unwrap();
    assert_eq!(partial.data["valid"], true);
    assert!(!partial.complete);
    assert_eq!(
        partial.data["targets"][0]["coverage"],
        json!({"checked":2,"resolved":1,"unresolved":1})
    );
    verify_capture(
        &home,
        &partial.provenance[0],
        &partial.data,
        false,
        "addon-matrix-validate",
    );
    fs::write(addon.join("Main.lua"), "C_Good.Do()\n").unwrap();
    let complete =
        matrix::validate_with_resolver(&ctx, &matrix_request, tools(), &mut |_| Ok(source.clone())).unwrap();
    assert!(complete.complete);
    verify_capture(
        &home,
        &complete.provenance[0],
        &complete.data,
        true,
        "addon-matrix-validate",
    );
    // Public syntax accepts the same generic/namespaced XML tokens as the external fixed reference,
    // while still compiling the real inline body and mapping diagnostics.
    fs::write(addon.join("Main.toc"), "## Interface: 120100\nMain.xml\n").unwrap();
    let mut public_xml = vec![];
    for xml in [
        "<Root><OnLoad>local value = 1</OnLoad></Root>",
        "<n:Root xmlns:n='urn:fixture'><n:OnLoad>local value = 1</n:OnLoad></n:Root>",
    ] {
        fs::write(addon.join("Main.xml"), xml).unwrap();
        let single = lycheedev_source::dispatch_with_tools(&ctx, &single_request, tools()).unwrap();
        assert_eq!(single.data["staticValid"], true);
        assert_eq!(single.data["syntaxCompilation"]["passed"], true);
        assert_eq!(single.data["syntaxCompilation"]["checkedInlineScripts"], 1);
        assert_eq!(single.data["semantic"]["state"], "not_requested");
        let matrix =
            matrix::validate_with_resolver(&ctx, &matrix_request, tools(), &mut |_| Ok(source.clone()))
                .unwrap();
        assert_eq!(matrix.data["valid"], true);
        assert_eq!(
            matrix.data["targets"][0]["syntaxCompilation"]["checkedInlineScripts"],
            1
        );
                {
            let mut semantic_request = single_request.clone();
            if let SourceRequest::Validate(request) = &mut semantic_request { request.semantic = true; }
            let native = lycheedev_source::dispatch_with_tools(&ctx, &semantic_request, tools()).unwrap_err();
            assert_eq!(native.code, "codebase.xml_invalid");
        }
        public_xml.push(json!({"xml":xml,"single":single.data,"matrix":matrix.data}));
    }
    fs::write(
        addon.join("Main.xml"),
        "<Root>\n<OnLoad>\nlocal broken =\n</OnLoad></Root>",
    )
    .unwrap();
    let bad_inline = lycheedev_source::dispatch_with_tools(&ctx, &single_request, tools()).unwrap_err();
    assert_eq!(bad_inline.code, "codebase.addon_static_check_failed");
    let diagnostic = bad_inline.details["result"]["syntaxCompilation"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["engine"] == "luac-5.1.5")
        .unwrap();
    assert_eq!(diagnostic["file"], "Main.xml");
    assert_eq!(diagnostic["line"], 4);
    assert!(diagnostic["sourceMap"].is_object());
    fs::write(
        addon.join("Main.toc"),
        "## Interface: 1\n## Interface: 120100\nMain.lua\n",
    )
    .unwrap();
    let gap_source = source_with_gaps(&mirror);
    fs::write(&matrix_path,serde_json::to_vec(&json!({"targets":[{"id":"retail","toc":"Main.toc","product":"retail","ref":gap_source.exact_commit}]})).unwrap()).unwrap();
    fs::write(addon.join("Main.lua"), "C_Good.Do(); C_Missing.Do()\n").unwrap();
    let (configuration, matrix_root) = lycheedev_source::matrix::read_config(&matrix_path, &ctx.control).unwrap();
    let matrix_request = lycheedev_source::request::MatrixValidationRequest {
        configuration: matrix_path.clone(),
        root: matrix_root,
        targets: configuration.targets,
        semantic: false,
    };
    let cold_gap =
        matrix::validate_with_resolver(&ctx, &matrix_request, tools(), &mut |_| Ok(gap_source.clone()))
            .unwrap();
    let warm_gap =
        matrix::validate_with_resolver(&ctx, &matrix_request, tools(), &mut |_| Ok(gap_source.clone()))
            .unwrap();
    for result in [&cold_gap, &warm_gap] {
        let target = &result.data["targets"][0];
        assert_eq!(target["valid"], true);
        assert!(!result.complete);
        assert_eq!(target["sourceCoverage"]["complete"], false);
        assert_eq!(
            target["coverage"],
            json!({"checked":3,"resolved":2,"unresolved":1})
        );
        assert!(
            target["facts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|fact| fact["name"] == "C_Good.Do" && fact["exists"] == true)
        );
        verify_capture(
            &home,
            &result.provenance[0],
            &result.data,
            false,
            "addon-matrix-validate",
        );
    }
    assert_eq!(
        cold_gap.data["targets"][0]["facts"],
        warm_gap.data["targets"][0]["facts"]
    );
    assert_eq!(
        cold_gap.data["targets"][0]["unresolved"],
        warm_gap.data["targets"][0]["unresolved"]
    );
    let cached = index::build_with_control(&home, &gap_source, &Default::default()).unwrap();
    assert_eq!(cached.summary.io["cachedIndex"], true);
    assert_eq!(cached.summary.diagnostics, 26);
    assert!(
        cached
            .summary
            .diagnostic_sample
            .as_ref()
            .unwrap()
            .iter()
            .all(|row| !row.path.contains("Blizzard_APIDocumentationGenerated"))
    );
    // Missing Git source and invalid addon are independent failures. The index
    // error wins without publishing any new syntax/addon capture or file blob.
    let unavailable = SourcePin {
        exact_commit: "f".repeat(40),
        ..source.clone()
    };
    let metadata = Metadata::open(&home, false).unwrap();
    let before = metadata.list::<Value>("capture/", 1000).unwrap();
    metadata.close().unwrap();
    let missing_root = home.join("missing-addon");
    let failed = lycheedev_source::validate::addon_with_environment(
        &home,
        &unavailable,
        "PIN",
        &missing_root,
        "Invalid.lua",
        &tools(),
        false,
        &Default::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(failed.code, "codebase.git_failed");
    let failed_matrix = lycheedev_source::validate::toc_with_control(
        &home,
        &unavailable,
        "",
        &missing_root,
        "Invalid.lua",
        &tools(),
        false,
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(failed_matrix.code, "codebase.git_failed");
    let metadata = Metadata::open(&home, false).unwrap();
    assert_eq!(metadata.list::<Value>("capture/", 1000).unwrap(), before);
    metadata.close().unwrap();
    fs::write(home.join("validation-contract-report.json"),serde_json::to_vec_pretty(&json!({"source":source,"single":single.data,"missingMatrix":missing.details,"partialMatrix":partial.data,"completeMatrix":complete.data,"publicXml":public_xml,"badInline":bad_inline.details,"coldGap":cold_gap.data,"warmGap":warm_gap.data,"doubleFailure":failed.code,"profile":"real SDK single dispatcher and matrix resolver entry; CLI process not_run; fixtures retained"})).unwrap()).unwrap();
}
fn source_with_gaps(mirror: &Path) -> SourcePin {
    source(mirror, true)
}
