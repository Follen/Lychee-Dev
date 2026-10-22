use lycheedev_source::request::{AddonValidationRequest, SelectedSource, SourceContext, SourceRequest};
use lycheedev_source::{
    environment::{self, Identity, Limits},
    SourceTools, luac,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};
#[test]
#[ignore = "requires an explicit fixed source home/pin and verified bundled Lua compiler"]
fn fixed_blizzard_catalog_real_native_semantics() {
    let home = PathBuf::from(
        std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("explicit source home"),
    );
    let id = std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("explicit fixed pin");
    let pin = lycheedev_core::selection::load_pinned(&home, &id).unwrap();
    let source = pin.source.as_ref().unwrap();
    let control = Default::default();
    let docs = environment::documents_from_pin(&home, source, &control).unwrap();
    let identity = Identity {
        repository: source.repository.clone(),
        commit: source.exact_commit.clone(),
        client: source.product.clone(),
        dependency_commits: BTreeMap::new(),
        generator_version: environment::GENERATOR.into(),
    };
    let env = environment::build(identity.clone(), &docs, Limits::default(), &control).unwrap();
    assert!(env.facts.len() > 1000);
    assert!(
        env.facts
            .iter()
            .any(|r| r.name == "UnitHealth" && r.namespace.is_empty() && !r.returns.is_empty())
    );
    let staging = std::env::var_os("LYCHEEDEV_SOURCE_TEST_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.tmp/rust-source-environment-real")
        });
    fs::create_dir_all(&staging).unwrap();
    let input_docs: BTreeMap<_, _> = docs
        .iter()
        .map(|(p, b)| (p.clone(), std::str::from_utf8(b).unwrap()))
        .collect();
    fs::write(
        staging.join("input.json"),
        serde_json::to_vec(
            &json!([{"name":"fixed-blizzard-retail","identity":identity,"documents":input_docs}]),
        )
        .unwrap(),
    )
    .unwrap();
    fs::write(
        staging.join("rust.json"),
        serde_json::to_vec(&json!([{"name":"fixed-blizzard-retail","result":env}])).unwrap(),
    )
    .unwrap();
    let compiler_manifest: serde_json::Value = serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler = luac::Runtime::verified(
        &PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed compiler")),
        serde_json::from_value(compiler_manifest["runtime"].clone()).unwrap(),
    ).unwrap();
    let addon = staging.join(format!("native-addon-{}", std::process::id()));
    fs::create_dir(&addon).unwrap();
    fs::write(addon.join("Main.toc"),format!("## Interface: {}\nMain.lua\n",lycheedev_core::selection::source_interface(source).unwrap())).unwrap();
    let ctx = SourceContext {
        home: home.clone(),
        cwd: addon.clone(),
        control,
    };
    let request = SourceRequest::Validate(AddonValidationRequest {
        selection: SelectedSource::new(pin.clone()).unwrap(),
        root: addon.clone(),
        toc: "Main.toc".into(),
        semantic: true,
        environment: None,
        locator: format!("{}/Main.toc", addon.display()),
    });
    fs::write(addon.join("Main.lua"), "local health = UnitHealth(\"player\", true)\nreturn health\n").unwrap();
    let good = lycheedev_source::dispatch_with_tools(&ctx, &request, SourceTools { luac: Some(&compiler) }).unwrap();
    assert_eq!(good.data["semantic"]["passed"], true);
    assert_eq!(good.data["semantic"]["engine"], "emmy_native_common_facts");
    assert!(!good.provenance.is_empty());
    fs::write(addon.join("Main.lua"), "local health = UnitHealth(\"player\", {})\nreturn health\n").unwrap();
    let bad = lycheedev_source::dispatch_with_tools(&ctx, &request, SourceTools { luac: Some(&compiler) }).unwrap_err();
    assert_eq!(bad.exit_code, 4);
    assert_eq!(bad.details["result"]["semantic"]["passed"], false);
    assert!(bad.details["result"]["semantic"]["diagnostics"].as_array().unwrap().iter().any(|d|d["code"]=="param-type-mismatch"));
    fs::write(staging.join("report.json"),serde_json::to_vec_pretty(&json!({"sourcePin":source,"catalog":env.manifest,"coverage":env.coverage,"good":good.data,"bad":bad.details,"publicSemantic":"emmy_native_common_facts"})).unwrap()).unwrap();
}
