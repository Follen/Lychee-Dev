//! Explicit source-only product observations; does not perform client input.
use lycheedev_core::{
    request::RequestContext, evidence,
    selection::{self, SourcePin},
};
use lycheedev_source::{SourceTools, environment, luac};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
};
fn argument(kind: &str) -> Option<&'static str> {
    match kind {
        "bool" | "boolean" => Some("true"),
        "string" => Some("\"matrix\""),
        "number" | "integer" => Some("1"),
        _ => None,
    }
}
#[test]
#[ignore = "explicit four authenticated source observations and actual bundled tools; default resolver may fetch exact commits"]
fn four_distinct_client_pins_default_resolver_actual_catalog_luac_native() {
    let manifest_path =
        PathBuf::from(std::env::var_os("LYCHEEDEV_MATRIX_PINS").expect("explicit prepared pins"));
    let observed: Value = serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    let home = PathBuf::from(observed["home"].as_str().unwrap());
    let pins = observed["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| serde_json::from_value::<SourcePin>(t["pin"]["source"].clone()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(pins.len(), 4);
    assert_eq!(
        pins.iter()
            .map(|p| &p.exact_commit)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    let stage = home
        .parent()
        .unwrap()
        .join(format!("actual-matrix-{}", std::process::id()));
    fs::create_dir(&stage).unwrap();
    let control = Default::default();
    let mut targets = vec![];
    let mut apis = vec![];
    for pin in &pins {
        selection::load_pinned(
            &home,
            observed["targets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["product"] == pin.product)
                .unwrap()["pin"]["id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let catalog = environment::from_pin(&home, pin, &control).unwrap();
        let record = catalog
            .facts
            .iter()
            .find(|r| {
                r.kind == "function"
                    && !r.parameters.is_empty()
                    && r.parameters.len() <= 3
                    && r.parameters
                        .iter()
                        .all(|s| s.inner_type.is_empty() && argument(&s.type_name).is_some())
                    && catalog.mappings.iter().any(|m| m.callable == r.name)
            })
            .expect("observed emitted primitive-parameter API");
        let args = record
            .parameters
            .iter()
            .map(|s| argument(&s.type_name).unwrap())
            .collect::<Vec<_>>()
            .join(",");
        let lua = format!("{}.lua", pin.product);
        let toc = format!("{}.toc", pin.product);
        fs::write(
            stage.join(&lua),
            format!("local answer = {}({args})\n", record.name),
        )
        .unwrap();
        fs::write(
            stage.join(&toc),
            format!(
                "## Interface: {}\n{lua}\n",
                selection::source_interface(pin).unwrap()
            ),
        )
        .unwrap();
        targets
            .push(json!({"id":pin.product,"product":pin.product,"ref":pin.exact_commit,"toc":toc}));
        apis.push(json!({"product":pin.product,"sourcePin":pin,"api":record,"definitions":catalog.manifest,"catalogCoverage":catalog.coverage,"args":args}));
    }
    let config = stage.join("matrix.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({"targets":targets})).unwrap(),
    )
    .unwrap();
    let compiler_manifest: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler = luac::Runtime::verified_with_ordered_control(
        &PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed compiler")),
        serde_json::from_value(compiler_manifest["runtime"].clone()).unwrap(), &Default::default(),
    )
    .unwrap();
    let ctx = lycheedev_source::request::SourceContext {
        home: home.clone(),
        cwd: stage.clone(),
        control,
    };
    let (configuration, matrix_root) = lycheedev_source::matrix::read_config(&config, &ctx.control).unwrap();
    let request = lycheedev_source::request::MatrixValidationRequest {
        configuration: config.clone(),
        root: matrix_root,
        targets: configuration.targets,
        semantic: true,
    };
    let result = lycheedev_source::matrix::dispatch(
        &ctx, &request,
        SourceTools {
            luac: Some(&compiler),
        },
    )
    .unwrap();
    assert_eq!(result.data["valid"], true, "{}", result.data);
    for (i, pin) in pins.iter().enumerate() {
        let target = &result.data["targets"][i];
        assert_eq!(target["product"], pin.product);
        assert_eq!(target["resolvedCommit"], pin.exact_commit);
        assert_eq!(target["syntaxCompilation"]["passed"], true);
        assert_eq!(target["semantic"]["passed"], true);
    }
    let mut proofs = vec![];
    for capture in &result.provenance {
        let proof = evidence::dispatch(&RequestContext {
            home: home.clone(), cwd: stage.clone(), control: ctx.control.clone(),
        }, evidence::EvidenceRequest::Verify { id: capture["id"].as_str().unwrap().into() })
        .unwrap();
        assert_eq!(proof.data["verified"], true);
        proofs.push(proof.data);
    }
    fs::write(home.parent().unwrap().join("four-product-sdk-report.json"),serde_json::to_vec_pretty(&json!({"observed":observed,"APIs":apis,"result":result.data,"captures":proofs,"compiler":compiler_manifest["runtime"],"runtime":"emmy_native_common_facts","scope":"four distinct actual source pins, observed API type metadata, default exact resolver and actual bundled tools; source-only Forever, no interactive acceptance or packaged CLI/reference full parity"})).unwrap()).unwrap();
}
