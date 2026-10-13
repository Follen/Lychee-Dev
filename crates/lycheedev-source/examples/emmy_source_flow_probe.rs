// One fixed request through the production native query seam. No source execution.
use std::{path::PathBuf,sync::{Arc,atomic::AtomicBool},time::{Duration,Instant}};
use emmylua_code_analysis::{Emmyrc,EmmyrcLuaVersion};
use lycheedev_core::control::OperationControl;
use lycheedev_source::native::*;
use serde_json::{json,Value};
type Result<T>=std::result::Result<T,String>;

fn string<'a>(value:&'a Value,key:&str)->Result<&'a str>{value.get(key).and_then(Value::as_str).ok_or_else(||format!("missing string {key}"))}
fn reference(value:&Value)->Result<HostMaterialRef>{Ok(HostMaterialRef{material:string(value,"material")?.into(),pointer:string(value,"pointer")?.into()})}
fn emit(value:Value,control:&OperationControl)->Result<()> {
    let mut value=value;value["schema"]=json!("lycheedev.source-flow-probe.v1");
    let bytes=serialize_json(&value,control,2*1024*1024).map_err(|error|error.to_string())?;
    use std::io::Write;
    let mut stdout=std::io::stdout().lock();stdout.write_all(&bytes).and_then(|_|stdout.write_all(b"\n")).map_err(|error|error.to_string())
}
fn execute(spec:&Value,control:&OperationControl)->Result<bool>{
    let case=&spec["case"];let id=string(case,"id")?;
    let originals=case["originals"].as_array().ok_or("case originals missing")?;
    let materials=spec["materials"].as_array().ok_or("materials missing")?.iter().map(|row|Ok(HostMaterialInput{name:string(row,"name")?.into(),bytes:string(row,"bytes")?.as_bytes().to_vec(),expected_sha256:string(row,"sha256")?.into()})).collect::<Result<Vec<_>>>()?;
    let bindings=spec["bindings"].as_array().ok_or("bindings missing")?.iter().map(|row|{
        let declaration=reference(&row["declaration"])?;
        Ok(match string(row,"kind")?{
            "callable"=>HostBindingInput::Callable{declaration,rules:Some(reference(&row["rules"])?)},
            "predicate"=>HostBindingInput::Predicate{declaration},
            "empty_object"=>HostBindingInput::EmptyObject{declaration},
            other=>return Err(format!("unknown binding kind {other}")),
        })
    }).collect::<Result<Vec<_>>>()?;
    let host=TrustedHostEnvironment::from_host_material("fixed-common-flow-v2".into(),false,materials,vec![],control)
        .and_then(|host|host.with_bindings(bindings,control)).map_err(|error|error.to_string())?;
    let budget:FlowBudget=serde_json::from_value(case.get("budget").cloned().unwrap_or_else(||json!({}))).map_err(|error|error.to_string())?;
    let mut limits=SourceLimits::default();limits.total_milliseconds=budget.max_millis;limits.output_bytes=budget.max_output_bytes;limits.flow=Some(budget);
    let files=originals.iter().map(|row|{let text=string(row,"text")?;Ok(SourceFileInput{path:string(row,"path")?.into(),original_utf8:text.into(),content_sha256:string(row,"sha256")?.into(),analysis_utf8:text.into(),map:SourceMap::Identity})}).collect::<Result<Vec<_>>>()?;
    let plan=if case["load"]["state"]=="unknown"{
        let mut names=files.iter().map(|file|file.path.clone()).collect::<Vec<_>>();names.sort();names.dedup();LoadPlan::Unknown(names)
    }else{LoadPlan::Known(case["load"]["ordered"].as_array().ok_or("ordered load plan missing")?.iter().map(|name|name.as_str().map(str::to_string).ok_or_else(||"load path missing".into())).collect::<Result<Vec<_>>>()?)};
    let ordered=match &plan{LoadPlan::Known(names)=>names.clone(),LoadPlan::Unknown(_)=>vec![]};
    let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;
    let inputs=SourceInputs::admit(SourceInputSpec{files,workspace_root:PathBuf::from("C:/lycheedev-common-flow-probe"),source_identity:json!({"kind":"fixed-common-flow","case":id}),phase:"after_load".into(),ordered_occurrences:ordered,config,host,luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7".into(),executable_bytes:487424},limits},control).map_err(|error|error.to_string())?;
    emit(json!({"event":"input_identity","case":id,"inputs":inputs.identity()}),control)?;
    let prepared=prepare(inputs,control).map_err(|error|error.to_string())?;
    let mut entry=None;let mut resolution=None;
    let target=&case["target"];let path=string(target,"path")?;
    if let Some(symbol)=target["symbol"].as_str(){
        let result=prepared.entry_symbol_candidates(path,symbol,control).map_err(|error|error.to_string())?;
        if let EntryResolution::Unique{candidate}=&result{entry=Some(EntryRequest{candidate:candidate.id.clone(),load_occurrence:None,creation_context:None,arguments:EntryArguments::Unspecified});}
        resolution=Some(result);
    }else if let Some(line)=target["line"].as_u64(){resolution=Some(prepared.entry_candidates(path,line as usize,control).map_err(|error|error.to_string())?);}
    if resolution.as_ref().is_some_and(|result|!matches!(result,EntryResolution::Unique{..})){
        emit(json!({"event":"scenario","case":id,"entry_resolution":resolution,"graph":null,"flow":null,"selection_complete":false}),control)?;
        return Ok(false)
    }
    if entry.is_none()&&let Some(EntryResolution::Unique{candidate})=&resolution{entry=Some(EntryRequest{candidate:candidate.id.clone(),load_occurrence:None,creation_context:None,arguments:EntryArguments::Unspecified});}
    let graph=prepared.build_plan(id,"after_load",plan,entry,Some(FLOW_RULES),control).map_err(|error|error.to_string())?;
    let queries=prepared.source_queries(control).and_then(|query|query.with_execution(&graph,QuerySelector::AllOccurrences,control)).map_err(|error|error.to_string())?;
    let flow=queries.flow(FlowOptions::default(),PageRequest::default()).map_err(|error|error.to_string())?;
    let complete=flow.complete;
    emit(json!({"event":"scenario","case":id,"entry_resolution":resolution,"graph":graph,"flow":flow,"selection_complete":true}),control)?;
    Ok(complete)
}

fn main(){
    let started=Instant::now();
    let control=OperationControl::new(started+Duration::from_millis(2000),Arc::new(AtomicBool::new(false)));
    let result=(||{
        let args=std::env::args().skip(1).collect::<Vec<_>>();
        if args.len()!=2||args[0]!="--spec"{return Err("usage: emmy_source_flow_probe --spec <case-spec.json>".into())}
        let bytes=std::fs::read(&args[1]).map_err(|error|error.to_string())?;
        let spec:Value=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
        emit(json!({"event":"build_identity","compiledPatchSHA256":option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256")}),&control)?;
        execute(&spec,&control)
    })();
    // execute's graph, analysis and inputs have dropped before measuring this point.
    let elapsed=started.elapsed().as_millis();
    let result=if elapsed>2000{Err(format!("flow total worker lifetime exceeded 2000ms: {elapsed}"))}else{result};
    let (complete,query_complete,error)=match result{Ok(query)=>(true,query,None),Err(error)=>(false,false,Some(error))};
    println!("{}",json!({"schema":"lycheedev.source-flow-probe.v1","event":"end","complete":complete,"queryComplete":query_complete,"error":error,"elapsedMillisecondsThroughDrop":elapsed,"ownerRetirement":"not_measured_by_worker","sourceExecution":false,"configExecution":false}));
    if !complete{std::process::exit(1)}
}
