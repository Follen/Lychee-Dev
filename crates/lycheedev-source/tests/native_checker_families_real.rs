use lycheedev_core::selection;
use lycheedev_source::request::{
    AddonValidationRequest, SelectedSource, SourceContext, SourceRequest,
};
use lycheedev_source::{SourceTools, luac};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "explicit fixed Retail ClientPin and bundled Lua51 compiler"]
fn selected_value_call_families_use_native_diagnostics_and_honest_coverage() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id = std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let pin = selection::load_pinned(&home, &pin_id).unwrap();
    let source = pin.source.as_ref().unwrap();
    assert_eq!(
        source.exact_commit,
        "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b"
    );
    let stage = std::env::var_os("LYCHEEDEV_NATIVE_FAMILIES_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lycheedev-native-families-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    fs::create_dir(&stage).expect("fresh family output directory");
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
    let cases = [
        (
            "source_good",
            "---@param x number\nlocal function one(x) end\none(1)\n",
            None,
            "argument_count_check",
            true,
        ),
        (
            "source_missing",
            "---@param x number\nlocal function one(x) end\none()\n",
            Some("missing-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "source_redundant",
            "local function zero() end\nzero(1)\n",
            Some("redundant-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "source_optional",
            "---@param x number?\nlocal function optional(x) end\noptional()\n",
            None,
            "argument_count_check",
            true,
        ),
        (
            "source_variadic",
            "local function spread(...) end\nspread(1,2,3)\n",
            None,
            "argument_count_check",
            true,
        ),
        (
            "source_typed_self",
            "---@param self number\nlocal function f(self) end\nlocal t={f=f}\nt:f()\n",
            Some("param-type-mismatch"),
            "argument_check",
            true,
        ),
        (
            "source_receiver_empty",
            "---@param receiver number\nlocal function f(receiver) end\nlocal t={f=f}\nt:f()\n",
            Some("param-type-mismatch"),
            "argument_check",
            true,
        ),
        (
            "source_receiver_explicit",
            "---@param receiver number\n---@param x number\nlocal function f(receiver,x) end\nlocal t={f=f}\nt:f(7)\n",
            Some("param-type-mismatch"),
            "argument_check",
            true,
        ),
        (
            "source_generic_receiver_colon_number",
            "---@generic T\n---@param receiver T\nlocal function f(receiver) end\nlocal t={f=f}\nt:f--[[@<number>]]()\n",
            Some("param-type-mismatch"),
            "argument_check",
            true,
        ),
        (
            "source_generic_receiver_dot_number",
            "---@generic T\n---@param receiver T\nlocal function f(receiver) end\nlocal t={f=f}\nt.f--[[@<number>]](t)\n",
            Some("param-type-mismatch"),
            "argument_check",
            true,
        ),
        (
            "source_generic_receiver_colon_table",
            "---@generic T\n---@param receiver T\nlocal function f(receiver) end\nlocal t={f=f}\nt:f--[[@<table>]]()\n",
            None,
            "argument_check",
            true,
        ),
        (
            "source_generic_receiver_dot_table",
            "---@generic T\n---@param receiver T\nlocal function f(receiver) end\nlocal t={f=f}\nt.f--[[@<table>]](t)\n",
            None,
            "argument_check",
            true,
        ),
        (
            "api_good",
            "C_Timer.After(0, function() end)\n",
            None,
            "argument_count_check",
            true,
        ),
        (
            "api_unknown_alias_required",
            "UnitHealth()\n",
            Some("missing-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "api_missing",
            "C_Timer.After()\n",
            Some("missing-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "api_redundant",
            "C_Timer.After(0, function() end, 1)\n",
            Some("redundant-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "callback_redundant",
            "RegisterEventCallback(\"EXAMPLE_EVENT\", function(extra) end)\n",
            Some("redundant-parameter"),
            "callback_count_check",
            true,
        ),
        (
            "alias_reassigned",
            "---@param x number\nlocal function one(x) end\nlocal current=one\ncurrent=function() end\ncurrent(1)\n",
            Some("redundant-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "number_callee",
            "local current=function() end\ncurrent=1\nlocal result=current()\n",
            Some("call-non-callable"),
            "callable_check",
            true,
        ),
        (
            "table_callee",
            "local current={}\ncurrent()\n",
            Some("call-non-callable"),
            "callable_check",
            true,
        ),
        (
            "function_callee",
            "local current=function() end\ncurrent()\n",
            None,
            "callable_check",
            true,
        ),
        (
            "unknown_callee",
            "local current=External\ncurrent(1)\n",
            None,
            "callable_check",
            false,
        ),
        (
            "unknown_trailing_return",
            "---@param x number\n---@param y number\nlocal function two(x,y) end\ntwo(External())\n",
            None,
            "argument_count_check",
            false,
        ),
        (
            "source_open_return",
            "---@param n number\nlocal function f(n) return mystery() end\nf('bad',2)\n",
            Some("redundant-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "source_declared_open_return",
            "---@param n number\n---@return number\nlocal function f(n) return mystery() end\nlocal function badcaller() return f('bad',2) end\n---@return string\nlocal function subject() return f(1) end\nsubject()\n",
            Some("return-type-mismatch"),
            "return_check",
            false,
        ),
        (
            "source_open_return_prefix",
            "local function f() return 1,mystery() end\n---@return number,number,number\nlocal function subject() return f() end\nsubject()\n",
            None,
            "return_count_check",
            false,
        ),
        (
            "source_open_prefix_type",
            "---@return string\nlocal function f() return 1,mystery() end\n---@return string\nlocal function subject() return f() end\nsubject()\n",
            Some("return-type-mismatch"),
            "return_check",
            false,
        ),
        (
            "source_known_return_unknown_effect",
            "---@param n number\nlocal function f(n) mystery(); return 1 end\n---@return string\nlocal function subject() return f('bad',2) end\nsubject()\n",
            Some("return-type-mismatch"),
            "return_check",
            true,
        ),
        (
            "source_conditional_call",
            "local function f() end\nlocal condition=mystery()\nif condition then f() end\n",
            None,
            "callable_check",
            false,
        ),
        (
            "source_fixed_tail_count",
            "local function pair() return 1,2 end\nlocal function one(value) end\none(pair())\n",
            Some("redundant-parameter"),
            "argument_count_check",
            true,
        ),
        (
            "source_parenthesized_count",
            "local function pair() return 1,2 end\nlocal function one(value) end\none((pair()))\n",
            None,
            "argument_count_check",
            true,
        ),
        (
            "source_non_tail_vararg_count",
            "local function two(a,b) end\nlocal function subject(...) two(...,1) end\n",
            None,
            "argument_count_check",
            true,
        ),
    ];
    let mut reports = Vec::new();
    for (name, text, expected, operation, available) in cases {
        fs::write(addon.join("main.lua"), text).unwrap();
        // Each case is a separate validate operation with the default 180s budget.
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
        let report = match lycheedev_source::dispatch_with_tools(
            &ctx,
            &request,
            SourceTools {
                luac: Some(&compiler),
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
        let summary = json!({"case":name,"diagnostics":diagnostics,"coverage":coverage,"gaps":semantic["gaps"]});
        for family in [
            "missing-parameter",
            "redundant-parameter",
            "call-non-callable",
        ] {
            assert!(
                semantic["checkerFamilies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value == family),
                "{summary}"
            );
        }
        if let Some(code) = expected {
            assert!(
                diagnostics.iter().any(|row| row["code"] == code),
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
            coverage
                .iter()
                .any(|row| row["operation"] == operation && row["state"] == state),
            "{summary}"
        );
        if let Some(target) = match name {
            "source_fixed_tail_count" => Some("one(pair())"),
            "source_parenthesized_count" => Some("one((pair()))"),
            "source_non_tail_vararg_count" => Some("two(...,1)"),
            _ => None,
        } {
            let start = text.rfind(target).unwrap() as u64;
            assert!(
                coverage
                    .iter()
                    .any(|row| row["generatedSite"]["start"] == start
                        && row["operation"] == "argument_count_check"
                        && row["state"] == "native_checked"),
                "{summary}"
            );
        }
        if !available {
            assert_eq!(semantic["complete"], false, "{summary}");
            assert!(
                !coverage
                    .iter()
                    .any(|row| row["operation"] == operation && row["state"] == "native_checked"),
                "{summary}"
            );
        }
        if name == "callback_redundant" {
            let diagnostic = diagnostics
                .iter()
                .find(|row| row["code"] == "redundant-parameter")
                .unwrap();
            assert_eq!(diagnostic["bytes"]["start"], text.find("extra").unwrap());
            assert_eq!(diagnostic["bytes"]["end"], text.find("extra").unwrap() + 5);
            assert!(
                diagnostic["nativeData"]["bindingCheckOwner"].is_object(),
                "{summary}"
            );
        }
        if matches!(
            name,
            "source_typed_self" | "source_receiver_empty" | "source_receiver_explicit"
        ) {
            let diagnostic = diagnostics
                .iter()
                .find(|row| row["code"] == "param-type-mismatch")
                .unwrap();
            let receiver = text.rfind("t:f").unwrap();
            assert_eq!(diagnostic["bytes"]["start"], receiver, "{summary}");
            assert_eq!(diagnostic["bytes"]["end"], receiver + 1, "{summary}");
        }
        if matches!(
            name,
            "source_generic_receiver_colon_number" | "source_generic_receiver_dot_number"
        ) {
            let diagnostic = diagnostics
                .iter()
                .find(|row| row["code"] == "param-type-mismatch")
                .unwrap();
            let receiver = if name == "source_generic_receiver_dot_number" {
                text.rfind("(t)").unwrap() + 1
            } else {
                text.rfind("t:f").unwrap()
            };
            assert_eq!(diagnostic["bytes"]["start"], receiver, "{summary}");
            assert_eq!(diagnostic["bytes"]["end"], receiver + 1, "{summary}");
        }
        if matches!(
            name,
            "source_open_return"
                | "source_declared_open_return"
                | "source_known_return_unknown_effect"
        ) {
            let target = "f('bad',2)";
            assert_eq!(text.matches(target).count(), 1);
            let start = text.find(target).unwrap() as u64;
            let end = start + target.len() as u64;
            for operation in ["callable_check", "argument_count_check", "argument_check"] {
                assert!(
                    coverage
                        .iter()
                        .any(|row| row["generatedSite"]["start"] == start
                            && row["generatedSite"]["end"] == end
                            && row["operation"] == operation
                            && row["state"] == "native_checked"),
                    "{summary}"
                );
            }
            for code in ["param-type-mismatch", "redundant-parameter"] {
                assert!(
                    diagnostics.iter().any(|row| row["code"] == code),
                    "{summary}"
                );
            }
            if name == "source_open_return" {
                assert!(
                    !coverage
                        .iter()
                        .any(|row| row["generatedSite"]["start"] == start
                            && row["generatedSite"]["end"] == end
                            && row["operation"] == "call_return"),
                    "discarded result has no return projection: {summary}"
                );
            }
        }
        if matches!(
            name,
            "source_open_return"
                | "source_declared_open_return"
                | "source_open_return_prefix"
                | "source_open_prefix_type"
        ) {
            assert_eq!(semantic["complete"], false, "{summary}");
            assert!(
                !diagnostics
                    .iter()
                    .any(|row| row["code"] == "missing-return-value"),
                "{summary}"
            );
        }
        if matches!(
            name,
            "source_declared_open_return" | "source_open_return_prefix" | "source_open_prefix_type"
        ) {
            let target = if name == "source_declared_open_return" {
                "f(1)"
            } else {
                "f()"
            };
            let return_expression = format!("return {target}");
            assert_eq!(text.matches(return_expression.as_str()).count(), 1);
            let start = (text.find(return_expression.as_str()).unwrap() + "return ".len()) as u64;
            let end = start + target.len() as u64;
            assert!(
                coverage
                    .iter()
                    .any(|row| row["generatedSite"]["start"] == start
                        && row["generatedSite"]["end"] == end
                        && row["operation"] == "call_return"
                        && row["state"] == "unavailable"
                        && row["reason"] == "host_sequence_requires_cardinality_projection"),
                "{summary}"
            );
            if matches!(
                name,
                "source_declared_open_return" | "source_open_prefix_type"
            ) {
                assert!(
                    diagnostics
                        .iter()
                        .any(|row| row["code"] == "return-type-mismatch"
                            && row["bytes"]["start"] == start
                            && row["bytes"]["end"] == end),
                    "consumed return type must be checked at its exact call: {summary}"
                );
            }
        }
        reports.push(json!({"case":name,"report":report}));
    }
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"pin":source,"cases":reports,"execution":"actual SDK dispatcher with fixed API catalog and bundled luac"})).unwrap()).unwrap();
}
