// Admission-only checks. No investigated source or executable config is run.
use std::{path::PathBuf,sync::{Arc,atomic::AtomicBool},time::{Duration,Instant}};
use emmylua_code_analysis::{Emmyrc,EmmyrcLuaVersion};
use lycheedev_core::control::OperationControl;
use lycheedev_source::native::{ENGINE_ARCHIVE_SHA256,ENGINE_COMMIT,SourceInputs,SourceInputSpec,SourceFileInput,SourceMap,SourceLimits,TrustedHostEnvironment,LuacIdentity,hash,prepare};
use rowan::TextRange;
use serde_json::{Value,json};

fn control()->OperationControl { OperationControl::uncancelable(Instant::now()+Duration::from_secs(30)) }
fn file(path:&str,text:&str)->SourceFileInput {SourceFileInput{path:path.into(),original_utf8:text.into(),content_sha256:hash(text.as_bytes()),analysis_utf8:text.into(),map:SourceMap::Identity}}
fn spec(c:&OperationControl)->SourceInputSpec {
    let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;
    SourceInputSpec{files:vec![file("owned.lua","local value = 1\n")],workspace_root:PathBuf::from("C:/lycheedev-owned-input-probe"),source_identity:json!({"kind":"fixed-probe-source","identity":"one"}),phase:"after_load".into(),ordered_occurrences:vec!["owned.lua".into()],config,host:TrustedHostEnvironment::from_host_material("closed-input-probe".into(),true,vec![],vec![],c).expect("fixed host"),luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7".into(),executable_bytes:487424},limits:SourceLimits::default()}
}
fn error(s:SourceInputSpec,c:&OperationControl)->Value {match SourceInputs::admit(s,c){Ok(_)=>json!(null),Err(e)=>json!({"code":e.code,"exit":e.exit_code})}}
fn assertion(name:&str,expected:Value,actual:Value,failed:&mut usize) {
    let passed=expected==actual;if !passed{*failed+=1;}
    println!("{}",json!({"schema":"lycheedev.native-source-input-probe.v1","event":"assertion","name":name,"expected":expected,"actual":actual,"passed":passed}));
}
fn main(){
    let mut failed=0;let c=control();let good=SourceInputs::admit(spec(&c),&c).expect("valid fixed owned inputs");
    println!("{}",json!({"schema":"lycheedev.native-source-input-probe.v1","event":"admission","identity":good.identity(),"identitySHA256":good.identity_sha256(),"engineCommit":ENGINE_COMMIT,"engineArchiveSHA256":ENGINE_ARCHIVE_SHA256}));
    let invalid=json!({"code":"source.native_invalid_input","exit":4});
    assertion("input:actual_original_bytes",json!("local value = 1\n"),json!(good.original("owned.lua").unwrap()),&mut failed);
    assertion("input:syntax_is_pending",json!("pending"),good.identity()["syntaxCheck"]["state"].clone(),&mut failed);
    assertion("input:never_executes_source",json!(false),good.identity()["sourceExecution"].clone(),&mut failed);
    assertion("input:never_executes_config",json!(false),good.identity()["configExecution"].clone(),&mut failed);
    let mut s=spec(&c);s.files[0].content_sha256="0".repeat(64);assertion("input:hash_tamper",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files[0].analysis_utf8="local value = 2\n".into();assertion("input:unmapped_analysis_tamper",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files[0].map=SourceMap::Utf8BomPrefix;assertion("input:fake_bom_map",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files.push(file("owned.lua","local second = 2\n"));assertion("input:duplicate_path",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files.push(file("OWNED.lua","local second = 2\n"));assertion("input:windows_case_collision",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files[0].path="../owned.lua".into();assertion("input:path_escape",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files.clear();assertion("input:empty_input",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.limits.input_bytes=1;assertion("input:byte_budget",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.limits.files=17;assertion("input:prototype_ceiling",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.ordered_occurrences=vec!["missing.lua".into()];assertion("input:missing_load_file",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.luac.version="5.4".into();assertion("input:wrong_luac_version",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.workspace_root=PathBuf::from("relative");assertion("input:relative_logical_root",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.config.runtime.version=EmmyrcLuaVersion::Lua54;assertion("input:wrong_analysis_lua_version",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.config.workspace.library.push(serde_json::from_value(json!("C:/unverified-library")).unwrap());assertion("input:unverified_external_library",invalid.clone(),error(s,&c),&mut failed);
    let mut s=spec(&c);s.files[0]=file("owned.lua","\u{feff}local 值 = 1\r\n");s.files[0].analysis_utf8="local 值 = 1\r\n".into();s.files[0].map=SourceMap::Utf8BomPrefix;
    let bom=SourceInputs::admit(s,&c).expect("valid BOM map");let mapped=bom.original_range("owned.lua",TextRange::new(6.into(),9.into())).unwrap();
    assertion("input:bom_unicode_original_range",json!([9,12]),json!([u32::from(mapped.start()),u32::from(mapped.end())]),&mut failed);
    let reversed=bom.analysis_range("owned.lua",mapped).unwrap();assertion("input:bom_reverse_range",json!([6,9]),json!([u32::from(reversed.start()),u32::from(reversed.end())]),&mut failed);
    assertion("input:unicode_boundary_rejected",json!(4),json!(bom.original_range("owned.lua",TextRange::new(7.into(),8.into())).unwrap_err().exit_code),&mut failed);
    assertion("input:removed_bom_not_analysis",json!(4),json!(bom.analysis_range("owned.lua",TextRange::new(0.into(),3.into())).unwrap_err().exit_code),&mut failed);
    let mut s=spec(&c);s.phase="unknown".into();let changed=SourceInputs::admit(s,&c).unwrap();assertion("input:phase_invalidates_key",json!(true),json!(changed.identity_sha256()!=good.identity_sha256()),&mut failed);
    let mut s=spec(&c);s.ordered_occurrences.push("owned.lua".into());let changed=SourceInputs::admit(s,&c).unwrap();assertion("input:duplicate_load_invalidates_key",json!(true),json!(changed.identity_sha256()!=good.identity_sha256()),&mut failed);
    assertion("input:duplicate_load_preserved",json!(2),json!(changed.identity()["orderedScriptLoadOccurrences"].as_array().unwrap().len()),&mut failed);
    let mut s=spec(&c);s.source_identity["identity"]=json!("two");let changed=SourceInputs::admit(s,&c).unwrap();assertion("input:source_identity_invalidates_key",json!(true),json!(changed.identity_sha256()!=good.identity_sha256()),&mut failed);
    let mut s=spec(&c);s.limits.construction_steps-=1;let changed=SourceInputs::admit(s,&c).unwrap();assertion("input:actual_limits_in_key",json!(true),json!(changed.identity_sha256()!=good.identity_sha256()),&mut failed);
    let mut s=spec(&c);s.luac.executable_sha256="1".repeat(64);let changed=SourceInputs::admit(s,&c).unwrap();assertion("input:luac_identity_invalidates_key",json!(true),json!(changed.identity_sha256()!=good.identity_sha256()),&mut failed);
    let cancelled_signal=lycheedev_core::control::TrackedCancellation::new();cancelled_signal.cancel();
    let cancelled=OperationControl::tracked(Instant::now()+Duration::from_secs(30),cancelled_signal);
    assertion("input:cancel_classification",json!({"code":"command.cancelled","exit":7}),error(spec(&c),&cancelled),&mut failed);
    let expired=OperationControl::uncancelable(Instant::now()-Duration::from_millis(1));
    assertion("input:deadline_classification",json!({"code":"command.deadline","exit":5}),error(spec(&c),&expired),&mut failed);
    let excessive=OperationControl::uncancelable(Instant::now()+Duration::from_secs(31));
    assertion("input:larger_control_budget_refused",invalid.clone(),error(spec(&c),&excessive),&mut failed);
    let prepared=prepare(good.clone(),&c).expect("same operation prepares exact admitted inputs");
    assertion("input:prepare_uses_admitted_file",json!(["owned.lua"]),json!(prepared.files().keys().collect::<Vec<_>>()),&mut failed);
    let replacement=control();let result=prepare(good.clone(),&replacement).err().map(|e|json!({"code":e.code,"exit":e.exit_code}));
    assertion("input:prepare_clock_reset_refused",invalid.clone(),json!(result),&mut failed);
    let foreign_token=OperationControl::uncancelable(c.deadline());let result=prepare(good,&foreign_token).err().map(|e|json!({"code":e.code,"exit":e.exit_code}));
    assertion("input:prepare_foreign_token_refused",invalid,json!(result),&mut failed);
    let cancelled=control();let admitted=SourceInputs::admit(spec(&cancelled),&cancelled).unwrap();cancelled.tracked_cancel_handle().unwrap().cancel();
    let result=prepare(admitted,&cancelled).err().map(|e|json!({"code":e.code,"exit":e.exit_code}));
    assertion("input:cancel_after_admit",json!({"code":"command.cancelled","exit":7}),json!(result),&mut failed);
    let short=OperationControl::uncancelable(Instant::now()+Duration::from_millis(500));let admitted=SourceInputs::admit(spec(&short),&short).unwrap();std::thread::sleep(Duration::from_millis(600));
    let result=prepare(admitted,&short).err().map(|e|json!({"code":e.code,"exit":e.exit_code}));
    assertion("input:deadline_after_admit",json!({"code":"command.deadline","exit":5}),json!(result),&mut failed);
    println!("{}",json!({"schema":"lycheedev.native-source-input-probe.v1","event":"end","complete":true,"failed":failed,"fullSourceAcceptance":false}));
    if failed!=0{std::process::exit(1);}
}
