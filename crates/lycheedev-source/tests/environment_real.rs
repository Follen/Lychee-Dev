use lycheedev_source::{
    environment::{self, Identity, Limits},
    luals::Runtime,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};
#[test]
#[ignore = "requires an explicit fixed source home/pin and authenticated LuaLS ZIP"]
fn fixed_blizzard_catalog_real_luals_semantics() {
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
    let staging =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-environment-real");
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
    let zip = PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("pinned zip"));
    let runtime_path = staging.join(format!("runtime-{}", std::process::id()));
    let runtime = Runtime::unpack_pinned(&zip, &runtime_path).unwrap();
    let good = BTreeMap::from([(
        "Good.lua".into(),
            b"local health = UnitHealth(\"player\", true)\nreturn health\n".to_vec(),
    )]);
    let good = runtime
        .check_with_control(&staging, &good, env.definitions.as_bytes(), &control)
        .unwrap();
    assert_eq!(good["passed"], true, "{good}");
    let bad = BTreeMap::from([(
        "Bad.lua".into(),
            b"local health = UnitHealth(\"player\", {})\nreturn health\n".to_vec(),
    )]);
    let bad = runtime
        .check_with_control(&staging, &bad, env.definitions.as_bytes(), &control)
        .unwrap();
    assert_eq!(bad["passed"], false, "{bad}");
    assert!(
        bad["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "param-type-mismatch")
    );
    fs::write(staging.join("report.json"),serde_json::to_vec_pretty(&json!({"sourcePin":source,"catalog":env.manifest,"coverage":env.coverage,"good":good,"bad":bad,"publicSemantic":"not_run"})).unwrap()).unwrap();
    drop(runtime);
    assert!(runtime_path.starts_with(&staging));
    fs::remove_dir_all(runtime_path).unwrap();
}
