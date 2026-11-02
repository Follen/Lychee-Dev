use super::*;
use lycheedev_core::control::OperationControl;
use std::time::{Duration, Instant};

fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn file(path: &str, text: &str) -> SourceFileInput {
    SourceFileInput {
        path: path.into(),
        original_utf8: text.into(),
        content_sha256: hash(text.as_bytes()),
        analysis_utf8: text.into(),
        map: SourceMap::Identity,
    }
}
fn spec(control: &OperationControl) -> SourceInputSpec {
    let mut config = Emmyrc::default();
    config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    SourceInputSpec {
        files: vec![file("owned.lua", "local value = 1\n")],
        workspace_root: std::env::current_dir()
            .unwrap()
            .join("native-admission-contract"),
        source_identity: json!({"fixed":"one"}),
        phase: "after_load".into(),
        ordered_occurrences: vec!["owned.lua".into()],
        config,
        host: TrustedHostEnvironment::from_host_material(
            "closed-admission".into(),
            true,
            vec![],
            vec![],
            control,
        )
        .unwrap(),
        luac: LuacIdentity {
            version: "5.1.5".into(),
            platform: "windows-amd64".into(),
            executable_sha256: "a".repeat(64),
            executable_bytes: 1,
        },
        limits: SourceLimits::default(),
    }
}

#[test]
fn admission_rejects_changed_bytes_unmapped_text_paths_and_untrusted_configuration() {
    let changes: &[(&str, fn(&mut SourceInputSpec))] = &[
        ("hash", |s| s.files[0].content_sha256 = "0".repeat(64)),
        ("analysis", |s| {
            s.files[0].analysis_utf8 = "local value = 2\n".into()
        }),
        ("BOM map", |s| s.files[0].map = SourceMap::Utf8BomPrefix),
        ("duplicate path", |s| {
            s.files.push(file("owned.lua", "local second = 2\n"))
        }),
        ("case collision", |s| {
            s.files.push(file("OWNED.lua", "local second = 2\n"))
        }),
        ("escape", |s| s.files[0].path = "../owned.lua".into()),
        ("empty", |s| s.files.clear()),
        ("bytes", |s| s.limits.input_bytes = 1),
        ("file ceiling", |s| s.limits.files = 17),
        ("missing load", |s| {
            s.ordered_occurrences = vec!["missing.lua".into()]
        }),
        ("compiler", |s| s.luac.version = "5.4".into()),
        ("relative root", |s| s.workspace_root = "relative".into()),
        ("language", |s| {
            s.config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua54
        }),
        ("external library", |s| {
            s.config
                .workspace
                .library
                .push(serde_json::from_value(json!("C:/unverified-library")).unwrap())
        }),
    ];
    for (label, change) in changes {
        let c = control();
        let mut input = spec(&c);
        change(&mut input);
        let error = SourceInputs::admit(input, &c).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("source.native_invalid_input", 4),
            "{label}"
        );
    }
}

#[test]
fn admitted_original_bytes_and_bom_unicode_ranges_remain_reversible() {
    let c = control();
    let input = SourceInputs::admit(spec(&c), &c).unwrap();
    assert_eq!(input.original("owned.lua").unwrap(), "local value = 1\n");
    let identity = input.identity();
    assert_eq!(identity["syntaxCheck"]["state"], "pending");
    assert_eq!(identity["sourceExecution"], false);
    assert_eq!(identity["configExecution"], false);
    let mut input = spec(&c);
    input.files[0] = file("owned.lua", "\u{feff}local 值 = 1\r\n");
    input.files[0].analysis_utf8 = "local 值 = 1\r\n".into();
    input.files[0].map = SourceMap::Utf8BomPrefix;
    let input = SourceInputs::admit(input, &c).unwrap();
    let range = input
        .original_range("owned.lua", TextRange::new(6.into(), 9.into()))
        .unwrap();
    assert_eq!((u32::from(range.start()), u32::from(range.end())), (9, 12));
    assert_eq!(
        input.analysis_range("owned.lua", range).unwrap(),
        TextRange::new(6.into(), 9.into())
    );
    assert_eq!(
        input
            .original_range("owned.lua", TextRange::new(7.into(), 8.into()))
            .err()
            .unwrap()
            .exit_code,
        4
    );
    assert_eq!(
        input
            .analysis_range("owned.lua", TextRange::new(0.into(), 3.into()))
            .err()
            .unwrap()
            .exit_code,
        4
    );
}

#[test]
fn source_identity_tracks_phase_load_occurrences_limits_and_compiler_bytes() {
    let c = control();
    let original = SourceInputs::admit(spec(&c), &c).unwrap();
    for change in [
        (|s: &mut SourceInputSpec| s.phase = "unknown".into()) as fn(&mut SourceInputSpec),
        |s| s.ordered_occurrences.push("owned.lua".into()),
        |s| s.source_identity["fixed"] = json!("two"),
        |s| s.limits.construction_steps -= 1,
        |s| s.luac.executable_sha256 = "1".repeat(64),
    ] {
        let mut input = spec(&c);
        change(&mut input);
        let changed = SourceInputs::admit(input, &c).unwrap();
        assert_ne!(changed.identity_sha256(), original.identity_sha256());
    }
    let mut duplicate = spec(&c);
    duplicate.ordered_occurrences.push("owned.lua".into());
    assert_eq!(
        SourceInputs::admit(duplicate, &c).unwrap().identity()["orderedScriptLoadOccurrences"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn admission_and_prepare_keep_the_original_control_lineage_and_first_stop() {
    let c = control();
    let admitted = SourceInputs::admit(spec(&c), &c).unwrap();
    assert_eq!(
        prepare(admitted.clone(), &c)
            .unwrap()
            .files()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["owned.lua"]
    );
    for replacement in [control(), OperationControl::uncancelable(c.deadline())] {
        let error = prepare(admitted.clone(), &replacement).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("source.native_invalid_input", 4)
        );
    }
    for expired in [false, true] {
        let stopped = stopped_control(expired, Instant::now() + Duration::from_secs(30));
        let error = SourceInputs::admit(spec(&c), &stopped).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            if expired {
                ("command.deadline", 5)
            } else {
                ("command.cancelled", 7)
            }
        );
    }
    let c = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        lycheedev_core::control::TrackedCancellation::new(),
    );
    let admitted = SourceInputs::admit(spec(&c), &c).unwrap();
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        prepare(admitted, &c).err().unwrap().code,
        "command.cancelled"
    );
    let excessive = OperationControl::uncancelable(Instant::now() + Duration::from_secs(31));
    assert_eq!(
        SourceInputs::admit(spec(&excessive), &excessive)
            .err()
            .unwrap()
            .code,
        "source.native_invalid_input"
    );
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(500));
    let admitted = SourceInputs::admit(spec(&short), &short).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        prepare(admitted, &short).err().unwrap().code,
        "command.deadline"
    );
}

fn host_data() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/source-native/host-facts-materials.json"
    ))
    .unwrap()
}
fn reference(value: &Value) -> HostMaterialRef {
    HostMaterialRef {
        material: value["material"].as_str().unwrap().into(),
        pointer: value["pointer"].as_str().unwrap().into(),
    }
}
fn environment(
    data: &Value,
    c: &OperationControl,
) -> lycheedev_core::Result<TrustedHostEnvironment> {
    let materials = data["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| HostMaterialInput {
            name: row["name"].as_str().unwrap().into(),
            bytes: row["bytes"].as_str().unwrap().as_bytes().to_vec(),
            expected_sha256: row["sha256"].as_str().unwrap().into(),
        })
        .collect();
    let bindings = data["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let declaration = reference(&row["declaration"]);
            match row["kind"].as_str().unwrap() {
                "callable" => HostBindingInput::Callable {
                    declaration,
                    rules: Some(reference(&row["rules"])),
                },
                "predicate" => HostBindingInput::Predicate { declaration },
                "empty_object" => HostBindingInput::EmptyObject { declaration },
                _ => panic!("fixed binding kind"),
            }
        })
        .collect();
    TrustedHostEnvironment::from_host_material(
        "host-admission-contract".into(),
        false,
        materials,
        vec![],
        c,
    )?
    .with_bindings(bindings, c)
}
fn changed_host(change: impl FnOnce(&mut Value)) -> Value {
    let mut data = host_data();
    let row = &mut data["materials"][0];
    let mut host: Value = serde_json::from_str(row["bytes"].as_str().unwrap()).unwrap();
    change(&mut host);
    let bytes = serde_json::to_string(&host).unwrap();
    row["sha256"] = json!(hash(bytes.as_bytes()));
    row["bytes"] = json!(bytes);
    data
}

#[test]
fn host_admission_rejects_ambiguous_records_arity_and_unsupported_effect_claims() {
    let changes: &[fn(&mut Value)] = &[
        |h| h["callables"][1]["name"] = h["callables"][2]["name"].clone(),
        |h| h["callables"][1]["identity"] = h["callables"][2]["identity"].clone(),
        |h| h["callables"][2]["returnArity"] = json!(-1),
        |h| h["callables"][2]["returnArity"] = json!(1.5),
        |h| h["callables"][2]["returnArity"] = json!(33),
        |h| h["callables"][2]["returnArity"] = json!(0),
        |h| h["callables"][2]["effect"] = json!("write_everything"),
        |h| h["callables"][2]["retainsArguments"] = json!(true),
        |h| h["sourceExecution"] = json!(true),
        |h| h["configExecution"] = json!(true),
    ];
    let c = control();
    for open in [false, true] {
        let mut data = host_data();
        if open {
            for binding in data["bindings"].as_array_mut().unwrap() {
                binding["declaration"]["material"] = json!("fixed-host-open-arity-v1");
            }
        }
        let host = environment(&data, &c).unwrap();
        assert_eq!(host.bindings().len(), 7);
        let mut input = spec(&c);
        input.host = host;
        SourceInputs::admit(input, &c).unwrap();
    }
    for change in changes {
        let error = environment(&changed_host(change), &c).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("source.native_invalid_input", 4)
        );
    }
    let mut changed = host_data();
    changed["materials"][0]["sha256"] = json!("0".repeat(64));
    assert_eq!(environment(&changed, &c).err().unwrap().exit_code, 4);
}

#[test]
fn host_bindings_reject_invalid_pointer_kind_material_rules_and_repeated_admission() {
    let c = control();
    for (pointer, material) in [
        ("/initialObjects/0", "fixed-host-v2"),
        ("/callables/31", "fixed-host-v2"),
        ("/callables/02", "fixed-host-v2"),
        ("/callables/2", "unadmitted"),
    ] {
        let mut data = host_data();
        data["bindings"] = json!([{"kind":"callable","declaration":{"material":material,"pointer":pointer},"rules":{"material":"original-flow-rules-v1","pointer":"/calls/Source"}}]);
        assert_eq!(environment(&data, &c).err().unwrap().exit_code, 4);
    }
    let mut data = host_data();
    data["bindings"][2]["rules"]["pointer"] = json!("/calls/Use");
    assert_eq!(environment(&data, &c).err().unwrap().exit_code, 4);
    assert_eq!(
        environment(&host_data(), &c)
            .unwrap()
            .with_bindings(vec![], &c)
            .err()
            .unwrap()
            .exit_code,
        4
    );
    let foreign = OperationControl::uncancelable(c.deadline());
    assert_eq!(
        environment(&host_data(), &c)
            .unwrap()
            .with_bindings(vec![], &foreign)
            .err()
            .unwrap()
            .exit_code,
        4
    );
    let mut input = spec(&foreign);
    input.host = environment(&host_data(), &c).unwrap();
    assert_eq!(
        SourceInputs::admit(input, &foreign)
            .err()
            .unwrap()
            .exit_code,
        4
    );
    for raw in [
        r#"{"callables":[],"callables":[],"sourceExecution":false,"configExecution":false}"#,
        "{not-json",
    ] {
        let mut data = host_data();
        data["materials"][0]["bytes"] = json!(raw);
        data["materials"][0]["sha256"] = json!(hash(raw.as_bytes()));
        assert_eq!(environment(&data, &c).err().unwrap().exit_code, 4);
    }
    for expired in [false, true] {
        let stopped = stopped_control(expired, Instant::now() + Duration::from_secs(30));
        let error = environment(&host_data(), &stopped).err().unwrap();
        assert_eq!(error.exit_code, if expired { 5 } else { 7 });
    }
}
