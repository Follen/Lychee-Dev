use lycheedev_core::{evidence, selection};
use lycheedev_source::request::{
    AddonValidationRequest, SelectedSource, SourceContext, SourceRequest,
};
use lycheedev_source::{SourceTools, environment, luac};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "explicit fixed Blizzard ClientPin and bundled Lua51 compiler"]
fn fixed_script_object_owners_and_callbacktype_reach_native_checker() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin = selection::load_pinned(
        &home,
        &std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin"),
    )
    .unwrap();
    let source = pin.source.as_ref().unwrap();
    assert_eq!(
        source.exact_commit, "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b",
        "this actual-catalog case is bound to the recorded Retail baseline"
    );
    let control = Default::default();
    let catalog = environment::from_pin(&home, source, &control).unwrap();
    let roots = catalog
        .facts
        .iter()
        .filter(|record| record.declaration_role == "script-object-root")
        .collect::<Vec<_>>();
    assert_eq!(
        catalog.manifest.identity.generator_version,
        environment::GENERATOR
    );
    assert_eq!(roots.len(), 79);
    let methods = catalog
        .facts
        .iter()
        .filter(|record| record.owner.is_some())
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 1438);
    let collisions = catalog
        .facts
        .iter()
        .filter(|record| record.name == "RegisterEventCallback" && record.kind == "function")
        .collect::<Vec<_>>();
    assert_eq!(collisions.len(), 2);
    assert_eq!(
        collisions
            .iter()
            .filter(|record| record.owner.is_none())
            .count(),
        1
    );
    assert_eq!(
        collisions
            .iter()
            .filter(|record| record
                .owner
                .as_ref()
                .is_some_and(|owner| owner.name == "SimpleFrameAPI"))
            .count(),
        1
    );
    assert!(
        catalog
            .facts
            .iter()
            .any(|record| record.name == "EventCallbackType" && record.kind == "callbacktype")
    );
    assert!(
        catalog
            .definitions
            .contains("---@alias EventCallbackType fun()")
    );
    assert!(!catalog.definitions.contains("---@class SimpleFrameAPI"));
    let stage = std::env::var_os("LYCHEEDEV_NATIVE_CATALOG_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lycheedev-native-catalog-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    fs::create_dir(&stage).expect("fresh catalog output directory");
    let addon = stage.join("addon");
    fs::create_dir(&addon).unwrap();
    fs::write(
        addon.join("Main.toc"),
        format!(
            "## Interface: {}\nmain.lua\n",
            selection::source_interface(source).unwrap()
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
    fs::write(
        addon.join("main.lua"),
        "RegisterEventCallback(\"EXAMPLE_EVENT\", function() end)\n",
    )
    .unwrap();
    let good = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap();
    assert_eq!(good.data["semantic"]["passed"], true);
    assert!(
        good.data["semantic"]["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["operation"] == "argument_check"
                && row["parameter"] == "callback"
                && row["parameterIndex"] == 2
                && row["state"] == "native_checked"),
        "{}",
        summary(&good.data["semantic"])
    );
    fs::write(
        addon.join("main.lua"),
        "RegisterEventCallback(\"EXAMPLE_EVENT\", {})\n",
    )
    .unwrap();
    let bad = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap_err();
    assert_eq!(bad.exit_code, 4);
    assert!(
        bad.details["result"]["semantic"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "param-type-mismatch"),
        "{}",
        summary(&bad.details["result"]["semantic"])
    );
    fs::write(addon.join("main.lua"), "AbortDrag()\n").unwrap();
    let abort_drag = catalog
        .facts
        .iter()
        .filter(|record| record.kind == "function" && record.name == "AbortDrag")
        .collect::<Vec<_>>();
    assert_eq!(abort_drag.len(), 1);
    assert!(
        abort_drag[0]
            .owner
            .as_ref()
            .is_some_and(|owner| owner.kind == "script-object" && owner.name == "SimpleFrameAPI")
    );
    let method_only = lycheedev_source::dispatch_with_tools(&ctx, &request, tools()).unwrap();
    assert_eq!(method_only.data["semantic"]["complete"], false);
    // This discarded call reaches the checker gates without requesting a
    // return projection. Its rejection belongs to those actual producers.
    let rejected = method_only.data["semantic"]["coverage"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["path"] == "main.lua" && row["bytes"]["start"] == 0 && row["bytes"]["end"] == 11
        })
        .collect::<Vec<_>>();
    for operation in ["argument_check", "argument_count_check", "callable_check"] {
        assert!(
            rejected.iter().any(|row| row["operation"] == operation
                && row["state"] == "unavailable"
                && row["reason"] == "open_initial_host_environment"),
            "{}",
            summary(&method_only.data["semantic"])
        );
    }
    assert!(
        !rejected.iter().any(
            |row| (row["operation"] == "call_permit" && row["state"] == "bound")
                || row["state"] == "native_checked"
                || row["state"] == "native_typing_permitted"
        ),
        "{}",
        summary(&method_only.data["semantic"])
    );
    let blob = &good.data["semantic"]["artifacts"]["definitions"];
    let archived: Value = serde_json::from_slice(
        &fs::read(evidence::blob_path(&home, blob["sha256"].as_str().unwrap()).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        archived["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|record| record["declarationRole"] == "script-object-root")
            .count(),
        roots.len()
    );
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"sourcePin":source,"catalog":catalog.manifest,"scriptObjectRoots":roots.len(),"ownedMethods":methods.len(),"good":good.data,"bad":bad.details,"methodOnly":method_only.data,"nominalRuntimeTypes":"unavailable_without_authenticated_correspondence","inheritance":"unavailable_without_authenticated_parent_edges"})).unwrap()).unwrap();
}
fn summary(value: &Value) -> Value {
    json!({"state":value["state"],"complete":value["complete"],"coverage":value["coverage"],"diagnostics":value["diagnostics"],"gaps":value["gaps"]})
}
