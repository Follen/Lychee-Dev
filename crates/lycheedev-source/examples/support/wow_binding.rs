use std::{io::Write, path::PathBuf, sync::Arc};
use emmylua_code_analysis::{Emmyrc, EmmyrcLuaVersion};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;
use lycheedev_source::native::*;


const SOURCES: &[(&str, &str)] = &[
    ("profile.lua", include_str!("../../../../tests/rust-source-engine/wow/profile.lua")),
    ("functions.lua", include_str!("../../../../tests/rust-source-engine/wow/functions.lua")),
    ("future-a.lua", include_str!("../../../../tests/rust-source-engine/wow/future-a.lua")),
    ("future-b.lua", include_str!("../../../../tests/rust-source-engine/wow/future-b.lua")),
    ("mixins.lua", include_str!("../../../../tests/rust-source-engine/wow/mixins.lua")),
    ("shadow.lua", include_str!("../../../../tests/rust-source-engine/wow/shadow.lua")),
    ("unsupported.lua", include_str!("../../../../tests/rust-source-engine/wow/unsupported.lua")),
    ("effect-mutation.lua", include_str!("../../../../tests/rust-source-engine/wow/effect-mutation.lua")),
    ("parallel-assignment.lua", include_str!("../../../../tests/rust-source-engine/wow/parallel-assignment.lua")),
    ("colon-call.lua", include_str!("../../../../tests/rust-source-engine/wow/colon-call.lua")),
    ("conditional-call.lua", include_str!("../../../../tests/rust-source-engine/wow/conditional-call.lua")),
];
const NATIVE_SOURCES: &[(&str,&str)] = &[
    ("generic-defs.lua", include_str!("../../../../tests/rust-source-engine/wow/native/generic-defs.lua")),
    ("generic-calls.lua", include_str!("../../../../tests/rust-source-engine/wow/native/generic-calls.lua")),
    ("generic-replace.lua", include_str!("../../../../tests/rust-source-engine/wow/native/generic-replace.lua")),
    ("coverage-unknown-argument.lua", include_str!("../../../../tests/rust-source-engine/wow/native/coverage-unknown-argument.lua")),
    ("object-versions.lua", include_str!("../../../../tests/rust-source-engine/wow/native/object-versions.lua")),
    ("object-facts.lua", include_str!("../../../../tests/rust-source-engine/wow/native/object-facts.lua")),
    ("audit-rebind.lua", include_str!("../../../../tests/rust-source-engine/wow/native/audit-rebind.lua")),
    ("audit-colon.lua", include_str!("../../../../tests/rust-source-engine/wow/native/audit-colon.lua")),
    ("audit-field-closure.lua", include_str!("../../../../tests/rust-source-engine/wow/native/audit-field-closure.lua")),
    ("audit-expression.lua", include_str!("../../../../tests/rust-source-engine/wow/native/audit-expression.lua")),
    ("audit-record-union.lua", include_str!("../../../../tests/rust-source-engine/wow/native/audit-record-union.lua")),
    ("invocation-captures.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-captures.lua")),
    ("invocation-reassign.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-reassign.lua")),
    ("invocation-short-circuit.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-short-circuit.lua")),
    ("invocation-selector.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-selector.lua")),
    ("invocation-escape.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-escape.lua")),
    ("invocation-escape-closure.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-escape-closure.lua")),
    ("invocation-escape-rebind.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-escape-rebind.lua")),
    ("invocation-call-capture.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-call-capture.lua")),
    ("invocation-return-slots.lua", include_str!("../../../../tests/rust-source-engine/wow/native/invocation-return-slots.lua")),
];
fn source(name: &str) -> &'static str { SOURCES.iter().chain(NATIVE_SOURCES.iter()).find(|(n,_)|*n==name).expect("fixture name").1 }

// The fixture harness supplies immutable host policy; investigated source never chooses intrinsics.
fn fixture_inputs(names:&[&str],native_profile:bool,control:&lycheedev_core::control::OperationControl)->Result<Arc<SourceInputs>> {
    let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;
    if native_profile {
        config.diagnostics.disable=emmylua_code_analysis::DiagnosticCode::all().into_iter().filter(|code|!matches!(code,emmylua_code_analysis::DiagnosticCode::ParamTypeMismatch|emmylua_code_analysis::DiagnosticCode::ReturnTypeMismatch)).collect();
        config.diagnostics.enables=vec![emmylua_code_analysis::DiagnosticCode::ParamTypeMismatch,emmylua_code_analysis::DiagnosticCode::ReturnTypeMismatch];
    }
    let policy=b"Lychee Dev fixed fixture host: explicit Mixin, CreateFromMixins and setmetatable shallow-copy profile v1";
    let mut intrinsics=Vec::new();
    if names.contains(&"profile.lua") {
        let text=source("profile.lua");
        for name in ["Mixin","CreateFromMixins","setmetatable"] {
            let marker=format!("function {name}");let start=text.find(&marker).ok_or("fixture host declaration")?+"function ".len();
            intrinsics.push(IntrinsicDeclaration{file:"profile.lua".into(),source_sha256:hash(text.as_bytes()),start:start as u32,end:(start+name.len())as u32,name:name.into(),rule:PROFILE.into(),host_material:"compiled-fixture-host-v1".into()});
        }
    }
    let host=TrustedHostEnvironment::from_host_material("closed-fixture-initial-globals-v1".into(),true,vec![HostMaterialInput{name:"compiled-fixture-host-v1".into(),bytes:policy.to_vec(),expected_sha256:hash(policy)}],intrinsics,control).map_err(|e|e.to_string())?;
    let luac:Value=serde_json::from_str(include_str!("../../../../release/tools/luac.json")).map_err(|e|e.to_string())?;
    let runtime=&luac["runtime"];
    let inputs=SourceInputSpec{
        files:names.iter().map(|name|{let text=source(name);SourceFileInput{path:(*name).into(),original_utf8:text.into(),content_sha256:hash(text.as_bytes()),analysis_utf8:text.into(),map:SourceMap::Identity}}).collect(),
        workspace_root:PathBuf::from("C:/lycheedev-wow-probe"),source_identity:json!({"kind":"compiled-fixture-inputs","schema":"lycheedev.native-fixture-admission.v1"}),phase:"unknown".into(),ordered_occurrences:names.iter().map(|name|(*name).into()).collect(),config,host,
        luac:LuacIdentity{version:runtime["version"].as_str().ok_or("luac version")?.into(),platform:runtime["platform"].as_str().ok_or("luac platform")?.into(),executable_sha256:runtime["sha256"].as_str().ok_or("luac hash")?.into(),executable_bytes:runtime["bytes"].as_u64().ok_or("luac bytes")?},limits:SourceLimits::default()};
    SourceInputs::admit(inputs,control).map_err(|e|e.to_string())
}
fn fixture_control()->lycheedev_core::control::OperationControl {
    lycheedev_core::control::OperationControl::new(std::time::Instant::now()+std::time::Duration::from_millis(SourceLimits::default().total_milliseconds),Arc::new(std::sync::atomic::AtomicBool::new(false)))
}
fn scenario(name:&str,phase:&str,ordered:&[&str],control:&lycheedev_core::control::OperationControl)->Result<Graph>{
    let mut unique=ordered.to_vec();unique.sort();unique.dedup();
    let input=prepare(fixture_inputs(&unique,false,control)?,control).map_err(|e|e.to_string())?;
    input.build_execution(name,phase,ordered,EvaluatorProfile::LegacyV7,None,control).map_err(|e|e.to_string())
}

struct Output { bytes:usize, checked:usize, failed:usize, control:lycheedev_core::control::OperationControl }
impl Output {
    fn emit(&mut self,mut value:Value)->Result<()> {
        value["schema"]=json!("lycheedev.emmy-wow-binding-probe.v1");
        let mut data=serialize_json(&value,&self.control,2097152).map_err(|e|e.to_string())?;data.push(b'\n');
        self.bytes+=data.len();if self.bytes>2*1024*1024{return Err("2 MiB output budget exceeded".into())}
        std::io::stdout().lock().write_all(&data).map_err(|e|e.to_string())
    }
    fn check(&mut self,name:&str,expected:Value,actual:Value)->Result<()> {
        let passed=expected==actual;self.checked+=1;if !passed{self.failed+=1}
        self.emit(json!({"event":"assertion","name":name,"expected":expected,"actual":actual,"passed":passed}))
    }
}

fn golden(out:&mut Output)->Result<()> {
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256")}))?;
    for (name,text) in SOURCES {out.emit(json!({"event":"input_identity","file":name,"name":name,"bytes":text.len(),"sha256":hash(text.as_bytes())}))?;}
    out.emit(json!({"event":"capabilities","profile":PROFILE,"intrinsicEvidence":"explicit fixture assumption selected by exact profile declaration identity; native implementation not verified","closedInitialEnvironment":true,"sourceExecution":false,"emmyConfigLoader":false,"limits":{"files":16,"inputBytes":65536,"astNodes":6000,"steps":10000,"values":2000,"facts":5000,"depth":64,"stdoutBytes":2097152,"effectSummaryParameters":32,"effectSummaryWrites":32},"supported":"linear fixed script loads, primitive annotated pure functions, bounded parameter static-field write summaries, bounded conditional effect footprints, static fields and assumed shallow-copy intrinsics","notImplemented":["general control-flow fixpoint","general Lua multi-return","general effects","conservative invalidation for unsupported unconditional calls","hidden captures/global effects of unknown conditional calls","conditional local binding flow","native Mixin proof","full Source business matrix"]}))?;
    let specifications:[(&str,&str,&[&str]);10]=[
        ("future-ab-unknown","unknown",&["profile.lua","functions.lua","future-a.lua","future-b.lua"]),
        ("future-ab-afterload","after_load",&["profile.lua","functions.lua","future-a.lua","future-b.lua"]),
        ("future-ba-afterload","after_load",&["profile.lua","functions.lua","future-b.lua","future-a.lua"]),
        ("mixins-afterload","after_load",&["profile.lua","functions.lua","mixins.lua"]),
        ("shadow-afterload","after_load",&["profile.lua","functions.lua","shadow.lua"]),
        ("unsupported-afterload","after_load",&["profile.lua","functions.lua","unsupported.lua"]),
        ("effect-mutation-afterload","after_load",&["profile.lua","functions.lua","effect-mutation.lua"]),
        ("parallel-assignment-afterload","after_load",&["profile.lua","functions.lua","parallel-assignment.lua"]),
        ("colon-call-afterload","after_load",&["profile.lua","functions.lua","colon-call.lua"]),
        ("conditional-call-afterload","after_load",&["profile.lua","functions.lua","conditional-call.lua"]),
    ];
    let mut keys=vec![];
    for (name,phase,ordered) in specifications {
        let graph=scenario(name,phase,ordered,&out.control)?;keys.push(graph.snapshot_key.clone());let facade=Facade{graph:&graph};
        let views=facade.views();
        out.emit(json!({"event":"scenario","name":name,"snapshotKey":graph.snapshot_key,"graphSHA256":digest(&graph),"coordinate":"original_utf8_bytes","graph":graph,"views":views}))?;
        let prefix=format!("{name}:");
        out.check(&(prefix.clone()+"same_fact_and_proof_all_views"),json!(true),json!(views["uses"].as_array().unwrap().iter().all(|v|v["binding"]["factId"]==v["type"]["factId"]&&v["binding"]["proofId"]==v["type"]["proofId"]&&v["definitionFactId"]==v["checkFactId"])))?;
        out.check(&(prefix.clone()+"managed_fallback_count"),json!(0),views["managedFallbackCalls"].clone())?;
        if name.starts_with("future-") {
            let before=facade.site_fact("future-a.lua","local before = Later","Later","load:")?;
            out.check(&(prefix.clone()+"top_level_value"),json!(if name=="future-ba-afterload"{"function"}else{"nil"}),json!(facade.value_type(&before.decision,0).ok()))?;
            for function_name in ["Deferred","WrongDeferredReturn"] {
                let marker=format!("function {function_name}()\n    return Later(\"ok\")");
                let fact=facade.site_fact("future-a.lua",&marker,"Later","function:")?;
                out.check(&(prefix.clone()+function_name+":selected_function"),json!(if phase=="unknown"{None}else{Some("F2")}),json!(facade.selected_function(&fact.decision).map(|f|f.name.as_str())))?;
                if phase=="unknown"{out.check(&(prefix.clone()+function_name+":reason"),json!(Decision::Unavailable("unknown_execution_phase".into())),json!(fact.decision))?;}
                let check=graph.returns.iter().find(|r|graph.functions[r.function].name==function_name).ok_or("return check")?;
                let view=facade.return_view(check);
                out.check(&(prefix.clone()+function_name+":return_coverage"),json!(if phase=="unknown"{"unavailable"}else{"checked"}),view["checkCoverage"].clone())?;
                out.check(&(prefix.clone()+function_name+":return_mismatch"),if phase=="unknown"{Value::Null}else{json!(function_name=="WrongDeferredReturn")},view["mismatch"].clone())?;
            }
        }
        if name=="mixins-afterload" {
            for (marker,token,target,ret,mismatch,context) in [
                ("local first = x.run(7)","x.run","F1","number",false,"load:"),
                ("local wrongFirst = x.run(\"bad\")","x.run","F1","number",true,"load:"),
                ("local second = x.run(\"ok\")","x.run","F2","string",false,"load:"),
                ("local wrongSecond = x.run(7)","x.run","F2","string",true,"load:"),
                ("return copied(7)","copied","F1","number",false,"function:"),
            ] {
                let fact=facade.site_fact("mixins.lua",marker,token,context)?;let call=graph.calls.iter().find(|c|c.callee==fact.id).ok_or("call fact")?;let view=facade.call_view(call);
                out.check(&(prefix.clone()+marker+":selection"),json!(target),json!(facade.selected_function(&fact.decision).map(|f|&f.name)))?;
                out.check(&(prefix.clone()+marker+":return"),json!(ret),view["returnType"].clone())?;
                out.check(&(prefix.clone()+marker+":coverage"),json!("checked"),view["checkCoverage"].clone())?;
                out.check(&(prefix.clone()+marker+":argument_mismatch"),json!(mismatch),json!(view["diagnostics"].as_array().unwrap().iter().any(|d|d["code"]=="argument_type_mismatch")))?;
            }
            let check=graph.returns.iter().find(|r|graph.functions[r.function].name=="WrongReturn").ok_or("copied return check")?;
            out.check(&(prefix.clone()+"copied_return_mismatch"),json!(true),facade.return_view(check)["mismatch"].clone())?;
            out.check(&(prefix.clone()+"two_field_copies"),json!(2),json!(graph.copies.len()))?;
        }
        if name=="shadow-afterload" {
            out.check(&(prefix.clone()+"zero_copy_edges"),json!(0),json!(graph.copies.len()))?;
            let absent=facade.site_fact("shadow.lua","local absent = target.run","target.run","load:")?;
            out.check(&(prefix.clone()+"target_field_absent"),json!("nil"),json!(facade.value_type(&absent.decision,0).ok()))?;
            for(marker,mismatch)in[("local plain = Mixin(1, 2)",false),("local wrong = Mixin(target, source)",true)]{
                let fact=facade.site_fact("shadow.lua",marker,"Mixin","load:")?;let call=graph.calls.iter().find(|c|c.callee==fact.id).ok_or("shadow call")?;let view=facade.call_view(call);
                out.check(&(prefix.clone()+marker+":local_definition"),json!("shadow.lua"),json!(facade.selected_function(&fact.decision).map(|f|&f.declaration.file)))?;
                out.check(&(prefix.clone()+marker+":ordinary"),Value::Null,view["intrinsic"].clone())?;
                out.check(&(prefix.clone()+marker+":return"),json!("number"),view["returnType"].clone())?;
                out.check(&(prefix.clone()+marker+":mismatch"),json!(mismatch),json!(!view["diagnostics"].as_array().unwrap().is_empty()))?;
            }
        }
        if name=="unsupported-afterload" {
            for(marker,token,reason)in[("local dynamic = object[key]","object[key]","dynamic_field_key"),("local uncertain = object.run","object.run","metatable_behavior_unavailable"),("local branchValue = conditional.run","conditional.run","conditional_write")]{
                let fact=facade.site_fact("unsupported.lua",marker,token,"load:")?;
                out.check(&(prefix.clone()+token+":reason"),json!(Decision::Unavailable(reason.into())),json!(fact.decision))?;
                out.check(&(prefix.clone()+token+":type_unavailable"),json!("unavailable"),facade.type_view(fact)["state"].clone())?;
            }
        }
        // RED-only counterexamples. These expectations describe Lua behavior or
        // honest subset coverage; the unchanged Builder/Facade compute actuals.
        if name=="effect-mutation-afterload" {
            let fact=facade.site_fact("effect-mutation.lua","local after = t.run(7)","t.run","load:")?;
            let call=graph.calls.iter().find(|c|c.callee==fact.id).ok_or("effect counterexample call")?;
            let view=facade.call_view(call);
            out.check(&(prefix.clone()+"mutation_reaches_later_field_call"),
                json!({"selectedFunction":"F2","returnType":"string","argumentMismatch":true}),
                json!({"selectedFunction":facade.selected_function(&fact.decision).map(|f|&f.name),"returnType":view["returnType"],"argumentMismatch":view["diagnostics"].as_array().unwrap().iter().any(|d|d["code"]=="argument_type_mismatch")}))?;
        }
        if name=="parallel-assignment-afterload" {
            let old=facade.site_fact("parallel-assignment.lua","local oldCall = old.run(7)","old.run","load:")?;
            let call=graph.calls.iter().find(|c|c.callee==old.id).ok_or("parallel counterexample call")?;
            let view=facade.call_view(call);
            let new=facade.site_fact("parallel-assignment.lua","local newRead = a.run","a.run","load:")?;
            out.check(&(prefix.clone()+"all_lhs_addresses_captured_before_writes"),
                json!({"oldCallFunction":"F1","oldCallReturnType":"number","newFieldType":"nil"}),
                json!({"oldCallFunction":facade.selected_function(&old.decision).map(|f|&f.name),"oldCallReturnType":view["returnType"],"newFieldType":facade.value_type(&new.decision,0).ok()}))?;
        }
        if name=="colon-call-afterload" {
            let callee=facade.site_fact("colon-call.lua","local got = t:run(7)","t:run","load:")?;
            let call=graph.calls.iter().find(|c|c.callee==callee.id).ok_or("colon counterexample call")?;
            let view=facade.call_view(call);
            let expression=facade.site_fact("colon-call.lua","local got = t:run(7)","t:run(7)","load:")?;
            out.check(&(prefix.clone()+"unsupported_invocation_has_honest_check_coverage"),
                json!({"checkCoverage":"unavailable","expressionTypeState":"unavailable","expressionReason":"colon_call_not_supported"}),
                json!({"checkCoverage":view["checkCoverage"],"expressionTypeState":facade.type_view(expression)["state"],"expressionReason":facade.type_view(expression)["reason"]}))?;
        }
        if name=="conditional-call-afterload" {
            let fact=facade.site_fact("conditional-call.lua","local after = t.run(7)","t.run","load:")?;
            let call=graph.calls.iter().find(|c|c.callee==fact.id).ok_or("conditional effect counterexample call")?;
            let view=facade.call_view(call);
            out.check(&(prefix+"unknown_branch_call_invalidates_field_proof"),
                json!({"bindingState":"unavailable","typeState":"unavailable","checkCoverage":"unavailable"}),
                json!({"bindingState":serde_json::to_value(&fact.decision).map_err(|e|e.to_string())?["state"],"typeState":facade.type_view(fact)["state"],"checkCoverage":view["checkCoverage"]}))?;
        }
    }
    let unique:std::collections::BTreeSet<_>=keys.iter().collect();out.check("phase_and_load_order_snapshot_keys_differ",json!(keys.len()),json!(unique.len()))?;
    Ok(())
}

fn adapter_main() {
    let mut out=Output{bytes:0,checked:0,failed:0,control:fixture_control()};
    let args:Vec<_>=std::env::args().skip(1).collect();
    let result=if args==["golden"]{golden(&mut out)}else{Err("usage: emmy_wow_binding_probe golden".into())};
    let complete=result.is_ok();let error=result.err();
    let terminal=json!({"event":"end","complete":complete,"checked":out.checked,"failed":out.failed,"businessBaselinePassed":complete&&out.failed==0,"fullSourceAcceptance":false,"error":error});
    if let Err(error)=out.emit(terminal){eprintln!("{error}");std::process::exit(1)}
    if !complete||out.failed!=0{std::process::exit(1)}
}
