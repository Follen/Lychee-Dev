use lycheedev_core::{Invocation, evidence, selection};
use lycheedev_source::{SourceTools, luac, luals, matrix};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
#[test]
#[ignore = "explicit fixed Retail ClientPin and authenticated real bundled tools; two TOC targets, not multiple products"]
fn two_target_actual_luac_luals_keeps_independent_failure_and_matrix_capture() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id = std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let selected = selection::load_pinned(&home, &pin_id).unwrap();
    let pin = selected.source.as_ref().unwrap();
    let stage =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-matrix-real");
    fs::create_dir_all(&stage).unwrap();
    let addon = stage.join(format!("中文 矩阵 addon-{}", std::process::id()));
    fs::create_dir(&addon).unwrap();
    let interface = selection::source_interface(pin).unwrap();
    fs::write(
        addon.join("A.toc"),
        format!("## Interface: {interface}\nA.lua\n"),
    )
    .unwrap();
    fs::write(
        addon.join("B.toc"),
        format!("## Interface: {interface}\n界 面.xml\n"),
    )
    .unwrap();
    fs::write(
        addon.join("A.lua"),
        "local health = UnitHealth(\"player\", true)\n",
    )
    .unwrap();
    fs::write(addon.join("界 面.xml"),"<Ui>\n<Frame><Scripts><OnLoad>\nlocal health = UnitHealth(\"player\", true)\n</OnLoad></Scripts></Frame></Ui>\n").unwrap();
    let config = addon.join("矩阵.json");
    fs::write(&config,serde_json::to_vec(&json!({"targets":[{"id":"retail-lua","toc":"A.toc","product":pin.product,"ref":pin.exact_commit},{"id":"retail-xml","toc":"B.toc","product":pin.product,"ref":pin.exact_commit}]})).unwrap()).unwrap();
    let manifest: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler = luac::Runtime::verified(
        &PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed compiler")),
        serde_json::from_value(manifest["runtime"].clone()).unwrap(),
    )
    .unwrap();
    let runtime_path = stage.join(format!("runtime-{}", std::process::id()));
    let runtime = luals::Runtime::unpack_pinned(
        &PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("fixed LuaLS ZIP")),
        &runtime_path,
    )
    .unwrap();
    let inv = Invocation {
        command: "source validate".into(),
        positional: None,
        home: home.clone(),
        cwd: addon.clone(),
        flags: BTreeMap::from([
            ("--matrix".into(), config.to_string_lossy().into()),
            ("--semantic".into(), "true".into()),
        ]),
        control: Default::default(),
    };
    let tools = || SourceTools {
        luac: Some(&compiler),
        luals: Some(&runtime),
    };
    let mut resolutions = 0;
    let good = matrix::validate_with_resolver(&inv, tools(), &mut |_| {
        resolutions += 1;
        Ok(pin.clone())
    })
    .unwrap();
    assert_eq!(resolutions, 2);
    assert_eq!(good.data["valid"], true);
    for (i, id) in ["retail-lua", "retail-xml"].iter().enumerate() {
        let target = &good.data["targets"][i];
        assert_eq!(target["id"], *id);
        assert_eq!(target["resolvedCommit"], pin.exact_commit);
        assert_eq!(target["syntaxCompilation"]["passed"], true);
        assert_eq!(target["semantic"]["passed"], true);
        assert_eq!(target["semantic"]["definitions"]["state"], "partial");
    }
    for capture in &good.provenance {
        let proof = evidence::dispatch(&Invocation {
            command: "evidence verify".into(),
            positional: Some(capture["id"].as_str().unwrap().into()),
            flags: BTreeMap::new(),
            home: home.clone(),
            cwd: addon.clone(),
            control: inv.control.clone(),
        })
        .unwrap();
        assert_eq!(proof.data["verified"], true);
    }
    fs::write(addon.join("界 面.xml"),"<Ui>\n<Frame><Scripts><OnLoad>\nlocal health = UnitHealth(\"player\", {})\n</OnLoad></Scripts></Frame></Ui>\n").unwrap();
    let bad = matrix::validate_with_resolver(&inv, tools(), &mut |_| Ok(pin.clone())).unwrap_err();
    assert_eq!(bad.exit_code, 4);
    assert_eq!(bad.details["result"]["valid"], false);
    assert_eq!(bad.details["result"]["targets"][0]["valid"], true);
    assert_eq!(
        bad.details["result"]["targets"][1]["syntaxCompilation"]["passed"],
        true
    );
    assert_eq!(
        bad.details["result"]["targets"][1]["semantic"]["passed"],
        false
    );
    let diagnostics = bad.details["result"]["targets"][1]["semantic"]["diagnostics"]
        .as_array()
        .unwrap();
    let d = diagnostics
        .iter()
        .find(|d| d["code"] == "param-type-mismatch")
        .unwrap();
    assert_eq!(d["path"], "界 面.xml");
    assert_eq!(d["range"]["start"]["line"], 2);
    assert_eq!(good.data["targets"].as_array().unwrap().len(), 2);
    fs::write(stage.join("two-target-report.json"),serde_json::to_vec_pretty(&json!({"sourcePin":pin,"compiler":manifest["runtime"],"runtime":luals::pinned(),"good":good.data,"bad":bad.details,"scope":"actual SDK matrix resolver/luac/LuaLS; two independent TOCs with one fixed Retail ClientPin; multiple-product and packaged CLI not_run"})).unwrap()).unwrap();
    drop(runtime);
    assert!(runtime_path.starts_with(&stage));
    fs::remove_dir_all(runtime_path).unwrap();
}
