use lycheedev_core::{Invocation, selection};
use lycheedev_source::{SourceTools, environment, luac};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[test]
#[ignore = "explicit fixed Retail ClientPin and bundled Lua51 compiler"]
fn return_count_family_uses_selected_native_slots_and_source_control_flow() {
    let home=PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id=std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let pin=selection::load_pinned(&home,&pin_id).unwrap();
    let source=pin.source.as_ref().unwrap();
    assert_eq!(source.exact_commit,"31c7f7b9cc79e56c986b365c06a6afbcf3c9177b");
    let stage=std::env::var_os("LYCHEEDEV_NATIVE_RETURNS_OUTPUT").map(PathBuf::from).unwrap_or_else(||
        std::env::temp_dir().join(format!("lycheedev-native-returns-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    fs::create_dir(&stage).expect("fresh return-count output directory");
    let addon=stage.join("addon");fs::create_dir(&addon).unwrap();
    fs::write(addon.join("Main.toc"),format!("## Interface: {}\nmain.lua\n",selection::source_interface(source).unwrap())).unwrap();
    let manifest:Value=serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler=luac::Runtime::verified(&PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed luac")),serde_json::from_value(manifest["runtime"].clone()).unwrap()).unwrap();
    let catalog=environment::from_pin(&home,source,&Default::default()).unwrap();
    let after=catalog.facts.iter().find(|record|record.name=="C_Timer.After"&&record.namespace=="C_Timer"&&record.kind=="function"&&record.owner.is_none()).expect("real C_Timer.After declaration");
    assert!(after.returns.is_empty(),"actual pinned declaration establishes the API zero-return case");
    assert_ne!(after.raw.get("MayReturnNothing"),Some(&Value::Bool(true)));
    let cases=[
        ("fixed_good","---@return number\nlocal function subject() return 1 end\n",None,"return_count_check",true),
        ("explicit_missing","---@return number\nlocal function subject() return end\n",Some("missing-return-value"),"return_count_check",true),
        ("implicit_missing","---@return number\nlocal function subject() end\n",Some("missing-return"),"return_flow_check",true),
        ("redundant","---@return number\nlocal function subject() return 1, 2 end\n",Some("redundant-return-value"),"return_count_check",true),
        ("optional_empty","---@return number?\nlocal function subject() end\n",None,"return_flow_check",true),
        ("zero_contract_empty","---@return nil\nlocal function subject() end\n",None,"return_flow_check",true),
        ("zero_tail_missing","local function empty() end\n---@return number\nlocal function subject() return empty() end\n",Some("missing-return-value"),"return_count_check",true),
        ("tail_multi_good","local function pair() return 1, 2 end\n---@return number, number\nlocal function subject() return pair() end\n",None,"return_count_check",true),
        ("tail_multi_excess","local function pair() return 1, 2 end\n---@return number\nlocal function subject() return pair() end\n",Some("redundant-return-value"),"return_count_check",true),
        ("non_tail_call_scalar","local function pair() return 1, 2 end\n---@return number, number\nlocal function subject() return pair(), 3 end\n",None,"return_count_check",true),
        ("parenthesized_call_scalar","local function pair() return 1, 2 end\n---@return number\nlocal function subject() return (pair()) end\n",None,"return_count_check",true),
        ("alias_reassigned_zero","local function pair() return 1, 2 end\nlocal current=pair\ncurrent=function() end\n---@return number\nlocal function subject() return current() end\n",Some("missing-return-value"),"return_count_check",true),
        ("conditional_good","---@return number\nlocal function subject(flag) if flag then return 1 else return 2 end end\n",None,"return_count_check",true),
        ("conditional_fallthrough","---@return number\nlocal function subject(flag) if flag then return 1 end end\n",Some("missing-return"),"return_flow_check",true),
        ("conditional_missing_value","---@return number, number\nlocal function subject(flag) if flag then return 1 else return 2, 3 end end\n",Some("missing-return-value"),"return_count_check",true),
        ("unknown_tail","---@return number\nlocal function subject() return External() end\n",None,"return_count_check",false),
        ("unknown_annotation","---@return UndeclaredReturnType\nlocal function subject() end\n",None,"return_count_check",false),
        ("conditional_tail_unavailable","local function choose(flag) if flag then return 1 else return 1, 2 end end\n---@return number\nlocal function subject(flag) return choose(flag) end\n",None,"return_count_check",false),
        ("api_zero_tail","---@return number\nlocal function subject() return C_Timer.After(0, function() end) end\n",Some("missing-return-value"),"return_count_check",true),
    ];
    let mut reports=Vec::new();
    for(name,text,expected,operation,available)in cases{
        fs::write(addon.join("main.lua"),text).unwrap();
        // Each independent validate request retains the ordinary 180s budget.
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
        for family in ["missing-return-value","redundant-return-value","missing-return"]{
            assert!(semantic["checkerFamilies"].as_array().unwrap().iter().any(|value|value==family),"{summary}");
        }
        if let Some(code)=expected{
            assert!(diagnostics.iter().any(|row|row["code"]==code),"{summary}");
            assert_eq!(semantic["passed"],false,"{summary}");
        }else{
            assert!(diagnostics.is_empty(),"{summary}");
            assert_eq!(semantic["passed"],true,"{summary}");
        }
        let subject_start=text.find("local function subject").unwrap() as u64;
        let subject_rows=coverage.iter().filter(|row|row["bytes"]["start"].as_u64().is_some_and(|start|start>=subject_start)&&row["operation"]==operation).collect::<Vec<_>>();
        let state=if available{"native_checked"}else{"unavailable"};
        assert!(subject_rows.iter().any(|row|row["state"]==state),"{summary}");
        if !available{
            assert_eq!(semantic["complete"],false,"{summary}");
            assert!(!subject_rows.iter().any(|row|row["state"]=="native_checked"),"{summary}");
        }
        if name=="explicit_missing"{
            let diagnostic=diagnostics.iter().find(|row|row["code"]=="missing-return-value").unwrap();
            assert_eq!(diagnostic["bytes"]["start"],text.rfind("return end").unwrap());
            assert_eq!(diagnostic["bytes"]["end"],text.rfind("return end").unwrap()+6);
        }
        if name=="redundant"{
            let diagnostic=diagnostics.iter().find(|row|row["code"]=="redundant-return-value").unwrap();
            assert_eq!(diagnostic["bytes"]["start"],text.rfind('2').unwrap());
            assert_eq!(diagnostic["bytes"]["end"],text.rfind('2').unwrap()+1);
        }
        reports.push(json!({"case":name,"report":report}));
    }
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"pin":source,"apiZeroReturnDeclaration":after,"cases":reports,"execution":"actual SDK dispatcher with fixed API catalog and bundled luac"})).unwrap()).unwrap();
}
