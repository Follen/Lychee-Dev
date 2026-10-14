// Fixed abstract load/template facts only. No Lua or repository config execution.
use std::{path::PathBuf,sync::{Arc,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
use emmylua_code_analysis::{Emmyrc,EmmyrcLuaVersion,LuaType};
use lycheedev_core::control::OperationControl;
use lycheedev_source::native::*;
use serde_json::{json,Value};
type Result<T> = std::result::Result<T,String>;
const DATA:&str=include_str!("../../../tests/rust-source-engine/host-facts-materials.json");
const FILES:&[(&str,&str,&str,bool)]=&[
 ("direct","host-direct.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-direct.lua"),false),
 ("shadow","host-shadow.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-shadow.lua"),false),
 ("colon","host-colon.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-colon.lua"),false),
 ("slots","host-slots.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-slots.lua"),false),
 ("transfers","host-transfers.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-transfers.lua"),false),
 ("phi","host-phi.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-phi.lua"),false),
 ("open-arity","host-open-arity.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-open-arity.lua"),true),
 ("unknown-effects","host-unknown.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-unknown.lua"),false),
 ("capture-mutation","host-capture-mutation.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-capture-mutation.lua"),false),
 ("cardinality","host-cardinality.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-cardinality.lua"),false),
 ("return-join","host-return-join.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-return-join.lua"),false),
 ("unknown-tail","host-unknown-tail.lua",include_str!("../../../tests/rust-source-engine/wow/native/host-unknown-tail.lua"),false),
];
fn reference(value:&Value)->HostMaterialRef{HostMaterialRef{material:value["material"].as_str().expect("fixed material").into(),pointer:value["pointer"].as_str().expect("fixed pointer").into()}}
fn prepare_case(file:&str,source:&str,open:bool,c:&OperationControl)->Result<PreparedSnapshot>{prepare_files(&[(file,source)],open,c)}
fn prepare_files(sources:&[(&str,&str)],open:bool,c:&OperationControl)->Result<PreparedSnapshot>{
 let data:Value=serde_json::from_str(DATA).map_err(|error|error.to_string())?;
 let materials=data["materials"].as_array().ok_or("fixed materials")?.iter().map(|row|HostMaterialInput{name:row["name"].as_str().expect("material name").into(),bytes:row["bytes"].as_str().expect("material bytes").as_bytes().to_vec(),expected_sha256:row["sha256"].as_str().expect("material hash").into()}).collect();
 let bindings=data["bindings"].as_array().ok_or("fixed bindings")?.iter().map(|row|{let mut declaration=reference(&row["declaration"]);if open{declaration.material="fixed-host-open-arity-v1".into();}match row["kind"].as_str().expect("binding kind"){
  "callable"=>HostBindingInput::Callable{declaration,rules:Some(reference(&row["rules"]))},"predicate"=>HostBindingInput::Predicate{declaration},"empty_object"=>HostBindingInput::EmptyObject{declaration},_=>unreachable!(),}}).collect();
 let host=TrustedHostEnvironment::from_host_material("fixed-host-common-facts-probe".into(),false,materials,vec![],c).and_then(|host|host.with_bindings(bindings,c)).map_err(|error|error.to_string())?;
 let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;
 let spec=SourceInputSpec{files:sources.iter().map(|(file,source)|SourceFileInput{path:(*file).into(),original_utf8:(*source).into(),content_sha256:hash(source.as_bytes()),analysis_utf8:(*source).into(),map:SourceMap::Identity}).collect(),workspace_root:PathBuf::from("C:/lycheedev-source-host-execution-probe"),source_identity:json!({"kind":"fixed-host-common-facts-probe"}),phase:"after_load".into(),ordered_occurrences:sources.iter().map(|(file,_)|(*file).into()).collect(),config,host,luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7".into(),executable_bytes:487424},limits:SourceLimits::default()};
 prepare(SourceInputs::admit(spec,c).map_err(|error|error.to_string())?,c).map_err(|error|error.to_string())
}
struct Output{bytes:usize,checked:usize,failed:usize}
impl Output{
 fn emit(&mut self,mut frame:Value)->Result<()>{frame["schema"]=json!("lycheedev.source-host-execution-probe.v1");let line=serde_json::to_string(&frame).map_err(|error|error.to_string())?;self.bytes+=line.len()+1;if self.bytes>2*1024*1024{return Err("host execution output byte budget".into())}println!("{line}");Ok(())}
 fn check(&mut self,name:&str,expected:Value,actual:Value)->Result<()>{let passed=expected==actual;self.checked+=1;if !passed{self.failed+=1}self.emit(json!({"event":"assertion","name":name,"expected":expected,"actual":actual,"passed":passed}))}
}
fn run(out:&mut Output)->Result<()>{
 out.emit(json!({"event":"build_identity","compiledPatchSHA256":option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256")}))?;
 for(name,file,source,open)in FILES{
  out.emit(json!({"event":"input_identity","case":name,"file":file,"bytes":source.len(),"sha256":hash(source.as_bytes())}))?;
  let c=OperationControl::new(Instant::now()+Duration::from_secs(30),Arc::new(AtomicBool::new(false)));
  let prepared=prepare_case(file,source,*open,&c)?;
  let graph=prepared.build(name,EvaluatorProfile::Native,Some("host-common-facts-v1"),&c).map_err(|error|error.to_string())?;
  let native=graph.native_invocations.as_ref().ok_or("native invocation facts")?;
  let host=graph.host_runtime.as_ref().ok_or("host common facts")?;
  out.check(&format!("{name}:complete_bounded_analysis"),json!(true),json!(native.control.complete))?;
  out.check(&format!("{name}:initial_environment_origins"),json!(7),json!(host.bindings.iter().filter(|binding|graph.values[binding.value].origin.environment()==Some(binding.declaration.declaration())).count()))?;
  out.check(&format!("{name}:all_call_slots_recorded"),json!(graph.calls.len()),json!(host.calls.len()))?;
  out.check(&format!("{name}:capture_reference_count"),json!(native.capture_facts.len()),json!(host.capture_references.len()))?;
  out.check(&format!("{name}:captures_not_value_transfers"),json!(true),json!(host.transfers.iter().all(|edge|edge.kind!="capture_cell_reference")))?;
  let mut values=std::collections::BTreeSet::new();let mut fresh=true;
  for call in &host.calls{if let Some(binding)=call.host_binding{for slot in &call.returns{let Decision::Bound(value)=slot.selected else{fresh=false;continue};fresh&=values.insert(value)&&matches!(graph.values[value].kind,ValueKind::HostReturn{binding:b,call:c,slot:s} if b==binding&&c==call.call&&s==slot.slot);}}}
  out.check(&format!("{name}:fresh_host_return_slots"),json!(true),json!(fresh))?;
  out.check(&format!("{name}:host_type_unavailable"),json!(true),json!(graph.uses.iter().filter(|fact|matches!(fact.decision,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostCallable(_)|ValueKind::HostReturn{..}))).all(|fact|Facade{graph:&graph}.type_view(fact)["state"]=="unavailable")))?;
  if !matches!(*name,"unknown-effects"|"unknown-tail"){out.check(&format!("{name}:nonretaining_host_calls"),json!(0),json!(native.escape_events.len()))?;}
  if *name=="capture-mutation"{
   let call=graph.calls.iter().find(|call|call.site.text(prepared.inputs()).is_ok_and(|text|text=="f()")).ok_or("capture mutation invocation")?;
   let returned=&graph.uses[call.result_fact.ok_or("capture mutation result")?];
   let nil=|decision:&Decision|matches!(decision,Decision::KnownAbsent(_))||matches!(decision,Decision::Bound(value) if matches!(&graph.values[*value].kind,ValueKind::Primitive(kind) if kind=="nil"));
   out.check("capture-mutation:invocation_returns_current_nil",json!(true),json!(nil(&returned.decision)))?;
   let reference=host.capture_references.first().ok_or("captured cell reference")?;
   let frame=native.frames.iter().find(|frame|frame.call==Some(call.id)).ok_or("capture mutation frame")?;
   let read=frame.return_facts.first().map(|fact|&graph.uses[*fact]).ok_or("capture return read")?;
   let latest=graph.writes.iter().rev().find(|write|write.cell==reference.cell).ok_or("capture current write")?;
   out.check("capture-mutation:read_uses_latest_cell_write",json!(true),json!(read.writes==vec![latest.id]&&nil(&latest.decision)))?;
   let consumer=host.calls.iter().find(|call|call.host_binding.is_some_and(|binding|host.bindings[binding].declaration.name()=="Use")).ok_or("capture consumer")?;
   out.check("capture-mutation:consumer_receives_nil",json!(true),json!(consumer.arguments.first().is_some_and(|slot|nil(&slot.selected))))?;
  }
  if *name=="transfers"{
   let cells=host.capture_references.iter().map(|reference|reference.cell.as_str()).collect::<std::collections::BTreeSet<_>>();
   out.check("transfers:factory_capture_cells_distinct",json!(2),json!(cells.len()))?;
   let mut selected=Vec::new();for expression in ["a()","b()"]{let call=graph.calls.iter().find(|call|call.site.text(prepared.inputs()).is_ok_and(|text|text==expression)).ok_or("factory closure call")?;let result=&graph.uses[call.result_fact.ok_or("factory result")?].decision;selected.push(match result{Decision::Bound(value)=>match graph.values[*value].kind{ValueKind::HostReturn{binding,slot,..}=>json!({"host":host.bindings[binding].declaration.name(),"slot":slot}),_=>Value::Null},_=>Value::Null});}
   out.check("transfers:factory_invocations_read_own_values",json!([{"host":"Source","slot":1},{"host":"Pair","slot":1}]),json!(selected))?;
  }
  if *name=="cardinality"{
   let variable=host.calls.iter().filter(|call|graph.calls[call.call].site.text(prepared.inputs()).is_ok_and(|text|text=="Variable()")).collect::<Vec<_>>();
   out.check("cardinality:bounded_zero_or_two_returns",json!(true),json!(variable.len()==2&&variable.iter().all(|call|call.return_arity.is_none()&&call.return_arities==Some(vec![0,2])&&call.return_open_tail.is_none()&&call.returns.iter().all(|slot|slot.presence=="unknown"))))?;
   let outer=host.calls.iter().filter(|call|call.host_binding.is_some_and(|binding|host.bindings[binding].declaration.name()=="UseSecond")).map(|call|call.argument_arities.clone()).collect::<Vec<_>>();
   out.check("cardinality:bare_and_parenthesized_arguments",json!([[0,2],[1]]),json!(outer))?;
  }
  if *name=="return-join"{
   let choose=host.calls.iter().find(|call|graph.calls[call.call].site.text(prepared.inputs()).is_ok_and(|text|text=="Choose()")).ok_or("return join call")?;
   out.check("return-join:bounded_one_or_two_returns",json!([1,2]),json!(choose.return_arities))?;
   let slot=choose.returns.first().ok_or("joined slot")?;
   let edge=host.transfers.iter().find(|edge|matches!(edge.destination,TransferDestination::Use{use_fact} if use_fact==slot.use_fact)&&edge.kind=="conditional_return_candidates").ok_or("conditional return candidate edge")?;
   let candidates=edge.return_candidates.as_ref().ok_or("conditional return candidates")?;
   out.check("return-join:first_slot_preserves_both_host_sources",json!(true),json!(candidates.len()==2&&edge.source_uses.len()==2&&candidates.iter().all(|candidate|candidate.use_fact.is_some()&&candidate.paths.iter().any(|path|!path.is_empty())&&matches!(candidate.selected,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostReturn{slot:1,..})))))?;
   let polarities=candidates.iter().flat_map(|candidate|candidate.paths.iter().flatten().map(|predicate|predicate.outcome)).collect::<std::collections::BTreeSet<_>>();
   out.check("return-join:candidate_path_polarities",json!([false,true]),json!(polarities))?;
  }
  if *name=="unknown-tail"{
   let outer=host.calls.iter().filter(|call|call.host_binding.is_some_and(|binding|host.bindings[binding].declaration.name()=="Use")).collect::<Vec<_>>();
   out.check("unknown-tail:bare_parenthesis_relay_arities",json!([null,[1],[2]]),json!(outer.iter().map(|call|call.argument_arities.clone()).collect::<Vec<_>>()))?;
   let relay=host.calls.iter().find(|call|graph.calls[call.call].site.text(prepared.inputs()).is_ok_and(|text|text=="Relay(UnknownCall())")).ok_or("unknown tail relay")?;
   out.check("unknown-tail:relay_argument_tail_preserved",json!(true),json!(relay.argument_open_tail.is_some()&&relay.argument_arities.is_none()))?;
   out.check("unknown-tail:relay_missing_formal_unavailable",json!(true),json!(graph.calls[relay.call].parameter_bindings.get(1).is_some_and(|parameter|matches!(parameter.selected_value,Decision::Unavailable(_)))))?;
   out.check("unknown-tail:relay_returns_two_unknown_values",json!(true),json!(relay.return_arities==Some(vec![2])&&relay.returns.len()==2&&relay.returns.iter().all(|slot|matches!(slot.selected,Decision::Unavailable(_)))))?;
  }
  let syntax=prepared.source_queries(&c).map_err(|error|error.to_string())?;
  let queries=syntax.with_execution(&graph,QuerySelector::AllOccurrences,&c).map_err(|error|error.to_string())?;
  let environment_value_id=digest(&(Some(graph.snapshot_key.as_str()),"value",host.bindings[0].value));
  let environment_context=queries.context(QueryTarget::Value(environment_value_id),ContextOptions{depth:0,snippet_bytes:128},PageRequest::default()).map_err(|error|error.to_string())?;
  let environment_rows=environment_context.rows.iter().filter(|row|row.origin.environment().is_some()).collect::<Vec<_>>();
  out.check(&format!("{name}:environment_no_source_snippet"),json!(true),json!(environment_rows.len()==1&&environment_rows[0].exhibited.is_none()&&environment_rows[0].snippet.is_none()))?;
  let selected_host=graph.uses.iter().find(|fact|matches!(fact.decision,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostCallable(_)))).ok_or("host callable use")?;
  let binding=prepared.binding_site(&selected_host.site)?;
  let native_view=match native.query_views.get(&selected_host.context).ok_or("selected host view")?{NativeQueryView::Load{occurrence}=>NativeViewSelection::Load(*occurrence),NativeQueryView::Invocation{frame}=>NativeViewSelection::Invocation(*frame),NativeQueryView::Entry{frame}=>NativeViewSelection::Entry(*frame),NativeQueryView::Static{..}=>NativeViewSelection::SourceTemplates,NativeQueryView::Xml{..}=>NativeViewSelection::Occurrence(*native.control.context_occurrences.get(&selected_host.context).ok_or("XML host control occurrence missing")?)};
  let native_type=prepared.query(&graph,native_view,&c,|analysis,session,control|{control.check()?;let model=analysis.semantic_model_with_context(binding.file_id,session);let ty=model.type_of_expr(binding.syntax);Ok(json!({"unknown":matches!(ty,LuaType::Unknown),"type":format!("{ty:?}")}))}).map_err(|error|error.to_string())?;
  out.check(&format!("{name}:native_host_type_unknown"),json!(true),native_type["unknown"].clone())?;
  out.emit(json!({"event":"native_host_type","case":name,"site":selected_host.site,"fact":selected_host.id,"result":native_type}))?;
  let freeze=prepared.freeze(&graph,NativeViewSelection::Load(0),&c).map(|_|json!({"accepted":true})).unwrap_or_else(|error|json!({"accepted":false,"code":error.code,"exit":error.exit_code,"error":error.to_string()}));
  out.emit(json!({"event":"scenario","case":name,"file":file,"graph":graph,"environmentContext":environment_context,"inputIdentity":prepared.inputs().identity(),"freeze":freeze,"views":"explicit_ordered_load_and_source_templates; no_explicit_entry","fullSourceAcceptance":false,"sourceExecuted":false}))?;
  c.cancel_handle().store(true,Ordering::Release);
  out.check(&format!("{name}:same_control_cancel"),json!(7),json!(prepared.freeze(&graph,NativeViewSelection::Load(0),&c).err().map(|error|error.exit_code)))?;
 }
 run_entry_cases(out)?;
 Ok(())
}
include!("support/host_entry_cases.rs");
include!("support/host_entry_boundaries.rs");
fn main(){let mut out=Output{bytes:0,checked:0,failed:0};let args=std::env::args().skip(1).collect::<Vec<_>>();let boundary_mode=args.len()==1&&args[0]=="--entry-boundaries";let result=if args.is_empty(){run(&mut out)}else if boundary_mode{run_entry_boundaries(&mut out)}else{Err("unsupported probe arguments".into())};let complete=result.is_ok();let error=result.err();if let Err(error)=out.emit(json!({"event":"end","mode":if boundary_mode{"entry-boundaries"}else{"host-entry-regression"},"complete":complete,"error":error,"checked":out.checked,"failed":out.failed,"fullSourceAcceptance":false,"explicitEntryImplemented":true})){eprintln!("{error}");std::process::exit(1)}
 if !complete||out.failed!=0{std::process::exit(1)}}
