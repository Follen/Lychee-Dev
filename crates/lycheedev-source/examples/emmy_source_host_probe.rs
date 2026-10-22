// Trusted host admission checks only. Source is not executed or abstractly advanced.
use std::{path::PathBuf,sync::{Arc,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
use emmylua_code_analysis::{Emmyrc,EmmyrcLuaVersion};
use lycheedev_core::control::OperationControl;
use lycheedev_source::native::*;
use serde_json::{json,Value};
type Result<T> = std::result::Result<T,String>;
const DATA:&str=include_str!("../../../tests/fixtures/source-native/host-facts-materials.json");
const SOURCE:&str=include_str!("../../../tests/fixtures/source-native/wow/native/host-direct.lua");
fn control()->OperationControl{OperationControl::uncancelable(Instant::now()+Duration::from_secs(30))}
fn reference(value:&Value)->HostMaterialRef{HostMaterialRef{material:value["material"].as_str().expect("fixed material").into(),pointer:value["pointer"].as_str().expect("fixed pointer").into()}}
fn inputs(data:&Value,open:bool)->Vec<HostBindingInput>{data["bindings"].as_array().expect("fixed bindings").iter().map(|row|{let mut declaration=reference(&row["declaration"]);if open{declaration.material="fixed-host-open-arity-v1".into();}match row["kind"].as_str().expect("fixed binding kind"){
 "callable"=>HostBindingInput::Callable{declaration,rules:Some(reference(&row["rules"]))},"predicate"=>HostBindingInput::Predicate{declaration},"empty_object"=>HostBindingInput::EmptyObject{declaration},_=>unreachable!(),}}).collect()}
fn materials(data:&Value)->Vec<HostMaterialInput>{data["materials"].as_array().expect("fixed materials").iter().map(|row|HostMaterialInput{name:row["name"].as_str().expect("fixed material name").into(),bytes:row["bytes"].as_str().expect("fixed bytes").as_bytes().to_vec(),expected_sha256:row["sha256"].as_str().expect("fixed hash").into()}).collect()}
fn environment(data:&Value,open:bool,c:&OperationControl)->lycheedev_core::Result<TrustedHostEnvironment>{TrustedHostEnvironment::from_host_material("fixed-host-admission-probe".into(),false,materials(data),vec![],c)?.with_bindings(inputs(data,open),c)}
fn spec(host:TrustedHostEnvironment)->SourceInputSpec{let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;SourceInputSpec{files:vec![SourceFileInput{path:"host-direct.lua".into(),original_utf8:SOURCE.into(),content_sha256:hash(SOURCE.as_bytes()),analysis_utf8:SOURCE.into(),map:SourceMap::Identity}],workspace_root:PathBuf::from("C:/lycheedev-source-host-admission-probe"),source_identity:json!({"kind":"fixed-host-admission-probe"}),phase:"after_load".into(),ordered_occurrences:vec!["host-direct.lua".into()],config,host,luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7".into(),executable_bytes:487424},limits:SourceLimits::default()}}
fn mutated(data:&Value,change:impl FnOnce(&mut Value))->Value{let mut changed=data.clone();let row=&mut changed["materials"][0];let mut host:Value=serde_json::from_str(row["bytes"].as_str().expect("fixed host JSON")).expect("fixed host");change(&mut host);let bytes=serde_json::to_string(&host).expect("bounded test JSON");row["bytes"]=json!(bytes);row["sha256"]=json!(hash(bytes.as_bytes()));changed}
struct Output{bytes:usize,checked:usize,failed:usize}
impl Output{
 fn emit(&mut self,mut frame:Value)->Result<()>{frame["schema"]=json!("lycheedev.source-host-admission-probe.v1");let bytes=serde_json::to_string(&frame).map_err(|e|e.to_string())?;self.bytes+=bytes.len()+1;if self.bytes>2*1024*1024{return Err("host output budget".into())}println!("{bytes}");Ok(())}
 fn check(&mut self,name:&str,expected:Value,actual:Value)->Result<()>{let passed=expected==actual;self.checked+=1;if !passed{self.failed+=1}self.emit(json!({"event":"assertion","name":name,"expected":expected,"actual":actual,"passed":passed}))}
 fn reject<T>(&mut self,name:&str,result:lycheedev_core::Result<T>,exit:i32)->Result<()>{let error=result.err();self.check(name,json!(exit),json!(error.as_ref().map(|e|e.exit_code)))?;self.emit(json!({"event":"rejection","name":name,"code":error.as_ref().map(|e|&e.code),"error":error.map(|e|e.to_string())}))}
}
fn run(out:&mut Output)->Result<()>{let data:Value=serde_json::from_str(DATA).map_err(|e|e.to_string())?;
 out.emit(json!({"event":"build_identity","engineCommit":ENGINE_COMMIT,"engineArchiveSHA256":ENGINE_ARCHIVE_SHA256}))?;
 for row in data["materials"].as_array().ok_or("fixed material array")?{out.emit(json!({"event":"material_identity","name":row["name"],"sha256":hash(row["bytes"].as_str().ok_or("fixed material bytes")?.as_bytes()),"bytes":row["bytes"].as_str().ok_or("fixed bytes")?.len()}))?;}
 for(open,label)in [(false,"exact"),(true,"open")]{let c=control();let host=environment(&data,open,&c).map_err(|e|e.to_string())?;let bindings=serde_json::to_value(host.bindings()).map_err(|e|e.to_string())?;let admitted=SourceInputs::admit(spec(host),&c).map_err(|e|e.to_string())?;
  out.check(&format!("host:{label}:seven_bindings"),json!(7),json!(bindings.as_array().ok_or("admitted binding array")?.len()))?;
  out.emit(json!({"event":"admitted","profile":label,"bindings":bindings,"inputIdentity":admitted.identity(),"inputIdentitySHA256":admitted.identity_sha256(),"analysisState":"host_facts_admitted_only","sourceExecuted":false,"nativeRuntimeIntegrated":false}))?;
 }
 let c=control();let mut wrong=data.clone();wrong["materials"][0]["sha256"]=json!("0".repeat(64));out.reject("host:hash_mismatch",environment(&wrong,false,&c),4)?;
 let variations:[(&str,fn(&mut Value));8]=[
  ("duplicate_name",|h|h["callables"][1]["name"]=h["callables"][2]["name"].clone()),
  ("duplicate_identity",|h|h["callables"][1]["identity"]=h["callables"][2]["identity"].clone()),
  ("negative_arity",|h|h["callables"][2]["returnArity"]=json!(-1)),
  ("fractional_arity",|h|h["callables"][2]["returnArity"]=json!(1.5)),
  ("oversized_arity",|h|h["callables"][2]["returnArity"]=json!(33)),
  ("risk_slot_exceeds_arity",|h|h["callables"][2]["returnArity"]=json!(0)),
  ("unsupported_effect",|h|h["callables"][2]["effect"]=json!("write_everything")),
  ("retained_arguments",|h|h["callables"][2]["retainsArguments"]=json!(true)),
 ];for(label,change)in variations{let c=control();out.reject(&format!("host:{label}"),environment(&mutated(&data,change),false,&c),4)?;}
 for(label,change)in [("source_execution",false),("config_execution",true)]{let c=control();let changed=mutated(&data,|h|h[if change{"configExecution"}else{"sourceExecution"}]=json!(true));out.reject(&format!("host:{label}"),environment(&changed,false,&c),4)?;}
 for(label,pointer,material)in [("wrong_pointer_kind","/initialObjects/0","fixed-host-v2"),("missing_record","/callables/31","fixed-host-v2"),("noncanonical_index","/callables/02","fixed-host-v2"),("missing_material","/callables/2","unadmitted")]{let c=control();let host=TrustedHostEnvironment::from_host_material("bad-pointer-probe".into(),false,materials(&data),vec![],&c).map_err(|e|e.to_string())?;out.reject(&format!("host:{label}"),host.with_bindings(vec![HostBindingInput::Callable{declaration:HostMaterialRef{material:material.into(),pointer:pointer.into()},rules:None}],&c),4)?;}
 let c=control();let host=TrustedHostEnvironment::from_host_material("wrong-rule-probe".into(),false,materials(&data),vec![],&c).map_err(|e|e.to_string())?;out.reject("host:wrong_rule_record",host.with_bindings(vec![HostBindingInput::Callable{declaration:HostMaterialRef{material:"fixed-host-v2".into(),pointer:"/callables/2".into()},rules:Some(HostMaterialRef{material:"original-flow-rules-v1".into(),pointer:"/calls/Use".into()})}],&c),4)?;
 for(label,bytes)in [("duplicate_json_keys","{\"callables\":[],\"callables\":[],\"sourceExecution\":false,\"configExecution\":false}"),("malformed_json","{not-json")]{let c=control();let host=TrustedHostEnvironment::from_host_material("invalid-json-probe".into(),false,vec![HostMaterialInput{name:"invalid".into(),bytes:bytes.as_bytes().to_vec(),expected_sha256:hash(bytes.as_bytes())}],vec![],&c).map_err(|e|e.to_string())?;out.reject(&format!("host:{label}"),host.with_bindings(vec![HostBindingInput::Callable{declaration:HostMaterialRef{material:"invalid".into(),pointer:"/callables/0".into()},rules:None}],&c),4)?;}
 let c=control();let host=environment(&data,false,&c).map_err(|e|e.to_string())?;out.reject("host:second_binding_admission",host.with_bindings(vec![],&c),4)?;
 let c=control();let foreign=OperationControl::uncancelable(c.deadline());let host=TrustedHostEnvironment::from_host_material("foreign-control-probe".into(),false,materials(&data),vec![],&c).map_err(|e|e.to_string())?;out.reject("host:foreign_binding_control",host.with_bindings(inputs(&data,false),&foreign),4)?;
 let c=control();let foreign=OperationControl::uncancelable(c.deadline());let host=environment(&data,false,&c).map_err(|e|e.to_string())?;out.reject("host:foreign_input_control",SourceInputs::admit(spec(host),&foreign),4)?;
 let c=control();c.tracked_cancel_handle().unwrap().cancel();out.reject("host:cancelled",environment(&data,false,&c),7)?;
 let c=OperationControl::uncancelable(Instant::now());out.reject("host:deadline",environment(&data,false,&c),5)?;
 let c=control();let host=environment(&data,false,&c).map_err(|e|e.to_string())?;let origin=ValueOrigin::Environment{environment:host.bindings()[0].declaration().clone()};out.emit(json!({"event":"environment_origin","origin":origin}))?;
 out.check("host:source_origin_compatible",json!({"file":"host-direct.lua","start":0,"end":5}),serde_json::to_value(ValueOrigin::Source(Site{file:"host-direct.lua".into(),start:0,end:5})).map_err(|e|e.to_string())?)?;
 Ok(())
}
fn main(){let mut out=Output{bytes:0,checked:0,failed:0};let result=run(&mut out);let complete=result.is_ok();let error=result.err();if let Err(e)=out.emit(json!({"event":"end","complete":complete,"error":error,"checked":out.checked,"failed":out.failed,"fullSourceAcceptance":false,"nativeRuntimeIntegrated":false})){eprintln!("{e}");std::process::exit(1)}
 if !complete||out.failed!=0{std::process::exit(1)}}
