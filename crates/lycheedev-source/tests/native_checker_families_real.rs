use lycheedev_core::{Invocation, selection};
use lycheedev_source::{SourceTools, luac};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[test]
#[ignore = "explicit fixed Retail ClientPin and bundled Lua51 compiler"]
fn selected_value_call_families_use_native_diagnostics_and_honest_coverage() {
    let home=PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id=std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let pin=selection::load_pinned(&home,&pin_id).unwrap();
    let source=pin.source.as_ref().unwrap();
    assert_eq!(source.exact_commit,"31c7f7b9cc79e56c986b365c06a6afbcf3c9177b");
    let stage=std::env::var_os("LYCHEEDEV_NATIVE_FAMILIES_OUTPUT").map(PathBuf::from).unwrap_or_else(||
        std::env::temp_dir().join(format!("lycheedev-native-families-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    fs::create_dir(&stage).expect("fresh family output directory");
    let addon=stage.join("addon");fs::create_dir(&addon).unwrap();
    fs::write(addon.join("Main.toc"),format!("## Interface: {}\nmain.lua\n",selection::source_interface(source).unwrap())).unwrap();
    let manifest:Value=serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler=luac::Runtime::verified(&PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed luac")),serde_json::from_value(manifest["runtime"].clone()).unwrap()).unwrap();
    let cases=[
        ("source_good","---@param x number\nlocal function one(x) end\none(1)\n",None,"argument_count_check",true),
        ("source_missing","---@param x number\nlocal function one(x) end\none()\n",Some("missing-parameter"),"argument_count_check",true),
        ("source_redundant","local function zero() end\nzero(1)\n",Some("redundant-parameter"),"argument_count_check",true),
        ("source_optional","---@param x number?\nlocal function optional(x) end\noptional()\n",None,"argument_count_check",true),
        ("source_variadic","local function spread(...) end\nspread(1,2,3)\n",None,"argument_count_check",true),
        ("api_good","C_Timer.After(0, function() end)\n",None,"argument_count_check",true),
        ("api_unknown_alias_required","UnitHealth()\n",Some("missing-parameter"),"argument_count_check",true),
        ("api_missing","C_Timer.After()\n",Some("missing-parameter"),"argument_count_check",true),
        ("api_redundant","C_Timer.After(0, function() end, 1)\n",Some("redundant-parameter"),"argument_count_check",true),
        ("callback_redundant","RegisterEventCallback(\"EXAMPLE_EVENT\", function(extra) end)\n",Some("redundant-parameter"),"callback_count_check",true),
        ("alias_reassigned","---@param x number\nlocal function one(x) end\nlocal current=one\ncurrent=function() end\ncurrent(1)\n",Some("redundant-parameter"),"argument_count_check",true),
        ("number_callee","local current=function() end\ncurrent=1\nlocal result=current()\n",Some("call-non-callable"),"callable_check",true),
        ("table_callee","local current={}\ncurrent()\n",Some("call-non-callable"),"callable_check",true),
        ("function_callee","local current=function() end\ncurrent()\n",None,"callable_check",true),
        ("unknown_callee","local current=External\ncurrent(1)\n",None,"callable_check",false),
        ("unknown_trailing_return","---@param x number\n---@param y number\nlocal function two(x,y) end\ntwo(External())\n",None,"argument_count_check",false),
    ];
    let mut reports=Vec::new();
    for(name,text,expected,operation,available)in cases{
        fs::write(addon.join("main.lua"),text).unwrap();
        // Each case is a separate validate operation with the default 180s budget.
        let inv=Invocation{command:"source validate".into(),positional:None,home:home.clone(),cwd:addon.clone(),flags:BTreeMap::from([
            ("--path".into(),addon.to_string_lossy().into_owned()),("--toc".into(),"Main.toc".into()),("--snapshot".into(),pin_id.clone()),("--semantic".into(),"true".into())
        ]),control:Default::default()};
        let report=match lycheedev_source::dispatch_with_tools(&inv,SourceTools{luac:Some(&compiler),luals:None}){
            Ok(result)=>result.data,
            Err(error)=>{assert_eq!(error.exit_code,4,"{name}: {error:?}");error.details["result"].clone()}
        };
        fs::write(stage.join(format!("{name}.json")),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        let semantic=&report["semantic"];
        let diagnostics=semantic["diagnostics"].as_array().expect("native diagnostics");
        let coverage=semantic["coverage"].as_array().expect("native coverage");
        let summary=json!({"case":name,"diagnostics":diagnostics,"coverage":coverage,"gaps":semantic["gaps"]});
        for family in ["missing-parameter","redundant-parameter","call-non-callable"]{
            assert!(semantic["checkerFamilies"].as_array().unwrap().iter().any(|value|value==family),"{summary}");
        }
        if let Some(code)=expected{
            assert!(diagnostics.iter().any(|row|row["code"]==code),"{summary}");
            assert_eq!(semantic["passed"],false,"{summary}");
        }else{
            assert!(diagnostics.is_empty(),"{summary}");
            assert_eq!(semantic["passed"],true,"{summary}");
        }
        let state=if available{"native_checked"}else{"unavailable"};
        assert!(coverage.iter().any(|row|row["operation"]==operation&&row["state"]==state),"{summary}");
        if !available{
            assert_eq!(semantic["complete"],false,"{summary}");
            assert!(!coverage.iter().any(|row|row["operation"]==operation&&row["state"]=="native_checked"),"{summary}");
        }
        if name=="callback_redundant"{
            let diagnostic=diagnostics.iter().find(|row|row["code"]=="redundant-parameter").unwrap();
            assert_eq!(diagnostic["bytes"]["start"],text.find("extra").unwrap());
            assert_eq!(diagnostic["bytes"]["end"],text.find("extra").unwrap()+5);
            assert!(diagnostic["nativeData"]["bindingCheckOwner"].is_object(),"{summary}");
        }
        reports.push(json!({"case":name,"report":report}));
    }
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"pin":source,"cases":reports,"execution":"actual SDK dispatcher with fixed API catalog and bundled luac"})).unwrap()).unwrap();
}
