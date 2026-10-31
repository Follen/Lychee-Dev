use lycheedev_source::request::{AddonValidationRequest, SelectedSource, SourceContext, SourceRequest};
use lycheedev_core::{request::RequestContext, evidence, selection};
use lycheedev_source::{SourceTools, luac};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
#[test]
#[ignore = "explicit fixed ClientPin, bundled luac and native descriptor checker"]
fn public_dispatch_fixed_catalog_luac_native_and_inline_semantic_line_map() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id = std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let pin = selection::load_pinned(&home, &pin_id).unwrap();
    let stage = std::env::var_os("LYCHEEDEV_NATIVE_SEMANTIC_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!(
            "lycheedev-native-semantic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )));
    fs::create_dir(&stage).expect("native semantic output must be a fresh directory");
    let addon = stage.join(format!("中文 空格 addon-{}", std::process::id()));
    fs::create_dir(&addon).unwrap();
    fs::write(
        addon.join("Main.toc"),
        format!(
            "## Interface: {}\n界 面.xml\n",
            selection::source_interface(pin.source.as_ref().unwrap()).unwrap()
        ),
    )
    .unwrap();
    let manifest: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler = luac::Runtime::verified_with_ordered_control(
        &PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed luac")),
        serde_json::from_value(manifest["runtime"].clone()).unwrap(), &Default::default(),
    )
    .unwrap();
    let ctx = SourceContext {
        home: home.clone(),
        cwd: addon.clone(),
        control: Default::default(),
    };
    let request = SourceRequest::Validate(AddonValidationRequest {
        selection: SelectedSource::new(pin.clone()).unwrap(),
        root: addon.clone(),
        toc: "Main.toc".into(),
        semantic: true,
        environment: None,
        locator: format!("{}/Main.toc", addon.display()),
    });
    let tools = || SourceTools {
        luac: Some(&compiler),
    };
    fs::write(addon.join("界 面.xml"),"<Ui>\n<Frame><Scripts><OnLoad>\nlocal value = UnitHealth(\"player\", true)\nself.value = value\n</OnLoad></Scripts></Frame></Ui>\n").unwrap();
    let good = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap();
    assert_eq!(good.data["staticValid"], true, "{}",semantic_summary(&good.data["semantic"]));
    assert_eq!(good.data["syntaxCompilation"]["passed"], true);
    assert_eq!(good.data["semantic"]["passed"], true);
    assert_eq!(good.data["semantic"]["engine"], "emmy_native_common_facts");
    assert!(good.data["semantic"]["coverage"].as_array().unwrap().iter().any(|row|
        row["operation"]=="argument_check"&&row["parameter"]=="usePredicted"&&row["parameterIndex"]==2&&row["state"]=="native_checked"&&row["argument"]["path"]=="界 面.xml"),"{}",semantic_summary(&good.data["semantic"]));
    assert!(good.data["semantic"]["coverage"].as_array().unwrap().iter().any(|row|
        row["operation"]=="argument_check"&&row["parameter"]=="unit"&&row["state"]=="unavailable"));
    assert_eq!(good.data["semantic"]["complete"],false);
    assert_eq!(good.data["complete"],false);
    assert!(!good.complete);
    assert_eq!(good.provenance[0]["complete"],false);
    assert_eq!(good.data["semantic"]["definitions"]["state"], "partial");
    for capture in good.data["semantic"]["captures"]
        .as_object()
        .unwrap()
        .values()
    {
        assert_eq!(
            evidence::dispatch(&RequestContext {
            home: home.clone(), cwd: addon.clone(), control: ctx.control.clone(),
        }, evidence::EvidenceRequest::Verify { id: capture["id"].as_str().unwrap().into() })
            .unwrap()
            .data["verified"],
            true
        );
    }
    fs::write(addon.join("界 面.xml"),"<Ui>\n<Frame><Scripts><OnLoad>\nlocal value = UnitHealth(\"player\", {})\nself.value = value\n</OnLoad></Scripts></Frame></Ui>\n").unwrap();
    let bad = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap_err();
    assert_eq!(bad.exit_code, 4);
    assert_eq!(bad.details["result"]["syntaxCompilation"]["passed"], true);
    let diag = bad.details["result"]["semantic"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "param-type-mismatch")
        .unwrap();
    assert_eq!(diag["path"], "界 面.xml");
    assert_eq!(diag["range"]["start"]["line"], 2);
    assert_eq!(diag["range"]["start"]["character"],35);
    assert_eq!(diag["range"]["end"]["character"],37);
    assert_eq!(diag["positionAccuracy"], "utf16-character");
    assert_eq!(diag["sourceMap"]["accuracy"],"exact");
    let mapped_xml="\u{feff}<Ui>\r\n<Frame><Scripts><OnLoad>local marker = &quot;🙂&quot;; local value = UnitHe&#97;lth(&quot;player&quot;, &#123;&#125;)\r\nself.value = value\r\n</OnLoad></Scripts></Frame></Ui>";
    fs::write(addon.join("界 面.xml"),mapped_xml).unwrap();
    let mapped_error=lycheedev_source::dispatch_with_tools(&ctx, &request,tools()).unwrap_err();
    assert_eq!(mapped_error.exit_code,4);
    let mapped_diag=mapped_error.details["result"]["semantic"]["diagnostics"].as_array().unwrap().iter().find(|row|row["code"]=="param-type-mismatch").unwrap();
    let raw_start=mapped_xml.find("&#123;&#125;").unwrap();
    let expected_column=mapped_xml[..raw_start].rsplit('\n').next().unwrap().encode_utf16().count();
    assert_eq!(mapped_diag["sourceMap"]["covering"],json!({"start":raw_start,"end":raw_start+12}));
    assert_eq!(mapped_diag["range"],json!({"start":{"line":1,"character":expected_column},"end":{"line":1,"character":expected_column+12}}));
    assert_eq!(mapped_diag["positionAccuracy"],"covering");
    assert!(mapped_diag["sourceMap"]["segments"].as_array().unwrap().iter().all(|segment|segment["origin"]=="Entity"));
    let maps_capture=&mapped_error.details["result"]["semantic"]["captures"]["sourceMaps"];
    let maps:Value=serde_json::from_slice(&fs::read(evidence::blob_path(&home,maps_capture["blob"]["sha256"].as_str().unwrap()).unwrap()).unwrap()).unwrap();
    assert_eq!(maps["inputs"][0]["xmlSHA256"],lycheedev_core::storage::sha256(mapped_xml.as_bytes()));
    assert_eq!(maps["inputs"][0]["implicitHandlerParameters"],"self:xml_object; remaining:unknown");
    assert_eq!(maps["inputs"][0]["segments"][0]["origin"],"Synthetic");
    assert!(maps["inputs"][0]["segments"][0]["raw"].is_null());
    fs::write(addon.join("Main.toc"),format!("## Interface: {}\nmain.lua\n",selection::source_interface(pin.source.as_ref().unwrap()).unwrap())).unwrap();
    fs::write(addon.join("main.lua"),"---@return string\nlocal function WrongReturn()\n return UnitHealth(\"player\", true)\nend\n").unwrap();
    let wrong_return=lycheedev_source::dispatch_with_tools(&ctx, &request,tools()).unwrap_err();
    assert_eq!(wrong_return.exit_code,4);
    assert!(wrong_return.details["result"]["semantic"]["diagnostics"].as_array().unwrap().iter().any(|row|
        row["code"]=="return-type-mismatch"&&row["path"]=="main.lua"),"{}",semantic_summary(&wrong_return.details["result"]["semantic"]));
    let source=pin.source.as_ref().unwrap();
    for (name,argument) in [("Good","true"),("Bad","{}")]{
        fs::write(addon.join(format!("{name}.toc")),format!("## Interface: {}\n{name}.lua\n",selection::source_interface(source).unwrap())).unwrap();
        fs::write(addon.join(format!("{name}.lua")),format!("local health = UnitHealth(\"player\", {argument})\n")).unwrap();
    }
    let matrix_path=addon.join("matrix.json");
    // External fixed reference matrix capture completion is static compatibility completeness. The
    // approved native checker keeps its partial evidence independently: the
    // unknown UnitHealth unit argument does not become a complete type check.
    fs::write(&matrix_path,serde_json::to_vec(&json!({"targets":[
        {"id":"good","toc":"Good.toc","source":source.repository,"product":source.product,"ref":source.exact_commit}
    ]})).unwrap()).unwrap();
    let matrix_ctx = SourceContext {
        home: home.clone(),
        cwd: addon.clone(),
        control: Default::default(),
    };
    let (configuration, matrix_root) = lycheedev_source::matrix::read_config(&matrix_path, &matrix_ctx.control).unwrap();
    let admitted_matrix = lycheedev_source::request::MatrixValidationRequest {
        configuration: matrix_path.clone(),
        root: matrix_root,
        targets: configuration.targets,
        semantic: true,
    };
    let matrix_request = SourceRequest::Matrix(admitted_matrix);
    let partial_semantic_matrix=lycheedev_source::dispatch_with_tools(&matrix_ctx, &matrix_request,tools()).unwrap();
    let partial_target=&partial_semantic_matrix.data["targets"][0];
    assert_eq!(partial_target["semantic"]["passed"],true);
    assert_eq!(partial_target["semantic"]["complete"],false);
    assert_eq!(partial_target["coverage"]["unresolved"],0);
    let expected_capture_complete=partial_semantic_matrix.data["valid"]==true
        && partial_target["coverage"]["unresolved"]==0
        && partial_target["sourceCoverage"]["complete"]==true;
    assert_eq!(partial_semantic_matrix.complete,expected_capture_complete);
    assert_eq!(partial_semantic_matrix.provenance[0]["complete"],expected_capture_complete);
    fs::write(&matrix_path,serde_json::to_vec(&json!({"targets":[
        {"id":"good","toc":"Good.toc","source":source.repository,"product":source.product,"ref":source.exact_commit},
        {"id":"bad","toc":"Bad.toc","source":source.repository,"product":source.product,"ref":source.exact_commit}
    ]})).unwrap()).unwrap();
    let (configuration, matrix_root) = lycheedev_source::matrix::read_config(&matrix_path, &matrix_ctx.control).unwrap();
    let admitted_matrix = lycheedev_source::request::MatrixValidationRequest {
        configuration: matrix_path.clone(),
        root: matrix_root,
        targets: configuration.targets,
        semantic: true,
    };
    let matrix_request = SourceRequest::Matrix(admitted_matrix);
    let matrix=lycheedev_source::dispatch_with_tools(&matrix_ctx, &matrix_request,tools()).unwrap_err();
    assert_eq!(matrix.exit_code,4);
    let targets=matrix.details["result"]["targets"].as_array().unwrap();
    assert_eq!(targets.len(),2);
    let matrix_good=targets.iter().find(|target|target["id"]=="good").unwrap();
    let matrix_bad=targets.iter().find(|target|target["id"]=="bad").unwrap();
    assert_eq!(matrix_good["semantic"]["engine"],"emmy_native_common_facts");
    assert_eq!(matrix_good["semantic"]["passed"],true);
    assert_eq!(matrix_bad["semantic"]["passed"],false);
    assert!(matrix_bad["semantic"]["diagnostics"].as_array().unwrap().iter().any(|row|row["code"]=="param-type-mismatch"));
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"good":good.data,"bad":bad.details,"mappedXml":mapped_error.details,"wrongReturn":wrong_return.details,"partialSemanticMatrix":partial_semantic_matrix.data,"partialSemanticMatrixCaptures":partial_semantic_matrix.provenance,"matrix":matrix.details,"profile":"SDK public dispatcher; actual packaged CLI semantic not_run"})).unwrap()).unwrap();
}
fn semantic_summary(value:&Value)->Value{
    json!({"coverage":value["coverage"],"gaps":value["gaps"],"diagnostics":value["diagnostics"],"state":value["state"],"complete":value["complete"]})
}
