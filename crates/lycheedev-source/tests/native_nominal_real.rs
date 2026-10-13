use lycheedev_core::{Invocation, selection};
use lycheedev_source::{SourceTools, environment, luac};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[test]
#[ignore = "explicit fixed Retail ClientPin and bundled Lua51 compiler"]
fn duration_nominal_methods_follow_actual_values_and_mutations() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id = std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let pin = selection::load_pinned(&home, &pin_id).unwrap();
    let source = pin.source.as_ref().unwrap();
    assert_eq!(
        source.exact_commit,
        "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b"
    );
    let catalog = environment::from_pin(&home, source, &Default::default()).unwrap();
    let owned = |owner: &str, name: &str| {
        catalog
            .facts
            .iter()
            .find(|record| {
                record.kind == "function"
                    && record.name == name
                    && record.owner.as_ref().is_some_and(|actual| {
                        actual.kind == "script-object" && actual.name == owner
                    })
            })
            .expect("actual owned method")
    };
    assert_eq!(
        owned("LuaDurationObjectAPI", "Copy").returns[0].type_name,
        "LuaDurationObject"
    );
    assert_eq!(
        owned("LuaDurationObjectAPI", "Assign").parameters[0].type_name,
        "LuaDurationObject"
    );
    assert_eq!(
        owned("LuaCurveObjectAPI", "Copy").returns[0].type_name,
        "LuaCurveObject"
    );
    assert!(
        catalog
            .facts
            .iter()
            .any(|record| record.name == "C_DurationUtil.CreateDuration"
                && record.owner.is_none()
                && record.returns[0].type_name == "LuaDurationObject")
    );
    let stage = std::env::var_os("LYCHEEDEV_NOMINAL_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lycheedev-native-nominal-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    fs::create_dir(&stage).expect("fresh nominal output directory");
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
    let compiler = luac::Runtime::verified(
        &PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed luac")),
        serde_json::from_value(manifest["runtime"].clone()).unwrap(),
    )
    .unwrap();
    // The final expression in each case is the observed business call. Constructors
    // are captured before unknown effects when the case needs more than one API.
    let cases = [
        (
            "copy_assign_chain",
            "local a=C_DurationUtil.CreateDuration()\nlocal b=a:Copy()\nlocal c=b:Copy()\nc:Assign(a)\n",
            "c:Assign(a)",
            None,
            true,
        ),
        (
            "method_alias",
            "local a=C_DurationUtil.CreateDuration()\nlocal assign=a.Assign\nlocal b=a:Copy()\nassign(b,a)\n",
            "assign(b,a)",
            None,
            true,
        ),
        (
            "explicit_dot_self",
            "local a=C_DurationUtil.CreateDuration()\na.Assign(a,a)\n",
            "a.Assign(a,a)",
            None,
            true,
        ),
        (
            "explicit_dot_wrong_other",
            "local a=C_DurationUtil.CreateDuration()\na.Assign(a,{})\n",
            "a.Assign(a,{})",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "explicit_dot_missing_other",
            "local a=C_DurationUtil.CreateDuration()\na.Assign(a)\n",
            "a.Assign(a)",
            Some("missing-parameter"),
            true,
        ),
        (
            "wrong_scalar",
            "local a=C_DurationUtil.CreateDuration()\na:Assign(\"bad\")\n",
            "a:Assign(\"bad\")",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "ordinary_table",
            "local a=C_DurationUtil.CreateDuration()\na:Assign({})\n",
            "a:Assign({})",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "different_nominal",
            "local duration=C_DurationUtil.CreateDuration\nlocal curve=C_CurveUtil.CreateCurve\nlocal a=duration()\nlocal b=curve()\na:Assign(b)\n",
            "a:Assign(b)",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "missing_argument",
            "local a=C_DurationUtil.CreateDuration()\na:Assign()\n",
            "a:Assign()",
            Some("missing-parameter"),
            true,
        ),
        (
            "redundant_argument",
            "local a=C_DurationUtil.CreateDuration()\na:Copy(1)\n",
            "a:Copy(1)",
            Some("redundant-parameter"),
            true,
        ),
        (
            "method_not_global",
            "LuaDurationObjectAPI.Copy()\n",
            "LuaDurationObjectAPI.Copy()",
            None,
            false,
        ),
        (
            "creator_rewritten",
            "C_DurationUtil.CreateDuration=function() return {Copy=1} end\nlocal a=C_DurationUtil.CreateDuration()\na:Copy()\n",
            "a:Copy()",
            Some("call-non-callable"),
            false,
        ),
        (
            "method_rewritten",
            "local a=C_DurationUtil.CreateDuration()\n---@param receiver any\n---@param n number\na.Copy=function(receiver,n) return n end\na:Copy(\"bad\")\n",
            "a:Copy(\"bad\")",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "unknown_member_effect",
            "local a=C_DurationUtil.CreateDuration()\nExternal(a)\na:Copy()\n",
            "a:Copy()",
            None,
            false,
        ),
        (
            "saved_method_after_effect",
            "local a=C_DurationUtil.CreateDuration()\nlocal copy=a.Copy\nExternal(a)\ncopy(a)\n",
            "copy(a)",
            None,
            true,
        ),
        (
            "ordinary_receiver",
            "local a=C_DurationUtil.CreateDuration()\nlocal copy=a.Copy\ncopy({})\n",
            "copy({})",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "stolen_colon_receiver",
            "local a=C_DurationUtil.CreateDuration()\nlocal other={Copy=a.Copy}\nother:Copy()\n",
            "other:Copy()",
            Some("param-type-mismatch"),
            true,
        ),
        (
            "saved_creator_after_rewrite",
            "local create=C_DurationUtil.CreateDuration\nC_DurationUtil.CreateDuration=function() return {} end\nlocal a=create()\na:Copy()\n",
            "a:Copy()",
            None,
            true,
        ),
    ];
    let mut reports = Vec::new();
    for (name, text, focus, expected, available) in cases {
        fs::write(addon.join("main.lua"), text).unwrap();
        let inv = Invocation {
            command: "source validate".into(),
            positional: None,
            home: home.clone(),
            cwd: addon.clone(),
            flags: BTreeMap::from([
                ("--path".into(), addon.to_string_lossy().into_owned()),
                ("--toc".into(), "Main.toc".into()),
                ("--snapshot".into(), pin_id.clone()),
                ("--semantic".into(), "true".into()),
            ]),
            control: Default::default(),
        };
        let report = match lycheedev_source::dispatch_with_tools(
            &inv,
            SourceTools {
                luac: Some(&compiler),
                luals: None,
            },
        ) {
            Ok(result) => result.data,
            Err(error) => {
                assert_eq!(error.exit_code, 4, "{name}: {error:?}");
                error.details["result"].clone()
            }
        };
        fs::write(
            stage.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        let semantic = &report["semantic"];
        let diagnostics = semantic["diagnostics"]
            .as_array()
            .expect("native diagnostics");
        let coverage = semantic["coverage"].as_array().expect("native coverage");
        let focus_start = text.rfind(focus).unwrap();
        let at_focus = |row: &Value| row["bytes"]["start"].as_u64() == Some(focus_start as u64);
        let focused = coverage
            .iter()
            .filter(|row| at_focus(row))
            .collect::<Vec<_>>();
        let summary = json!({"case":name,"diagnostics":diagnostics,"focusedCoverage":focused,"gaps":semantic["gaps"]});
        if let Some(code) = expected {
            assert!(
                diagnostics.iter().any(|row| row["code"] == code),
                "{summary}"
            );
            assert!(
                diagnostics.iter().all(|row| row["code"] == code),
                "{summary}"
            );
            assert_eq!(semantic["passed"], false, "{summary}");
        } else {
            assert!(diagnostics.is_empty(), "{summary}");
            assert_eq!(semantic["passed"], true, "{summary}");
        }
        let state = if available {
            "native_checked"
        } else {
            "unavailable"
        };
        assert!(
            focused
                .iter()
                .any(|row| row["operation"] == "argument_count_check" && row["state"] == state),
            "{summary}"
        );
        if !available {
            assert!(
                !focused
                    .iter()
                    .any(|row| row["operation"] == "argument_count_check"
                        && row["state"] == "native_checked"),
                "{summary}"
            );
        }
        if [
            "copy_assign_chain",
            "method_alias",
            "explicit_dot_self",
            "explicit_dot_wrong_other",
            "wrong_scalar",
            "ordinary_table",
            "different_nominal",
        ]
        .contains(&name)
        {
            assert!(
                focused
                    .iter()
                    .any(|row| row["operation"] == "argument_check"
                        && row["parameter"] == "other"
                        && row["state"] == "native_checked"),
                "{summary}"
            );
        }
        if name == "different_nominal" {
            assert!(
                diagnostics.iter().any(|row| row["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("LuaDurationObject")
                        && message.contains("LuaCurveObject"))),
                "{summary}"
            );
        }
        if name == "stolen_colon_receiver" {
            let diagnostic = diagnostics
                .iter()
                .find(|row| row["code"] == "param-type-mismatch")
                .unwrap();
            assert_eq!(diagnostic["bytes"]["start"], focus_start, "{summary}");
            assert_eq!(
                diagnostic["bytes"]["end"],
                focus_start + "other".len(),
                "{summary}"
            );
            assert!(
                focused
                    .iter()
                    .any(|row| row["operation"] == "argument_check"
                        && row["parameter"] == "receiver"
                        && row["state"] == "native_checked"),
                "{summary}"
            );
        }
        reports.push(json!({"case":name,"source":text,"report":report}));
    }
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"pin":source,"cases":reports,"execution":"actual SDK dispatcher with fixed API catalog and bundled luac","nominalEvidence":{"durationCopy":owned("LuaDurationObjectAPI","Copy"),"durationAssign":owned("LuaDurationObjectAPI","Assign"),"curveCopy":owned("LuaCurveObjectAPI","Copy")}})).unwrap()).unwrap();
}
