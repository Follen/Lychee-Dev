use emmylua_code_analysis::{LuaType, CheckConfig, DiagnosticCode, humanize_semantic_type};
use emmylua_code_analysis::binding_context::{BindingContext, BindingDecision, BindingSite, BoundValue, SelectedValue, ObjectRef, ObjectOrigin, BindingSession};
use emmylua_parser::{LuaAstNode, LuaCallExpr};

const NATIVE_FILES: &[&str] = &["profile.lua", "generic-defs.lua", "generic-calls.lua", "generic-replace.lua"];
const COVERAGE_FILES: &[&str] = &["profile.lua", "generic-defs.lua", "generic-calls.lua", "generic-replace.lua", "coverage-unknown-argument.lua"];

struct NativeInput { prepared:PreparedSnapshot }
impl std::ops::Deref for NativeInput { type Target=PreparedSnapshot;fn deref(&self)->&Self::Target{&self.prepared} }
impl NativeInput {
    fn new(control:&lycheedev_core::control::OperationControl)->Result<Self>{Self::from_files(&["profile.lua","generic-defs.lua","generic-calls.lua","generic-replace.lua"],control)}
    fn from_files(names:&[&str],control:&lycheedev_core::control::OperationControl)->Result<Self>{Ok(Self{prepared:prepare(fixture_inputs(names,true,control)?,control).map_err(|e|e.to_string())?})}
    fn freeze(&self,graph:&Graph)->Result<Arc<FrozenContext>>{self.freeze_for_view(graph,NativeViewSelection::SourceTemplates)}
    fn freeze_for_view(&self,graph:&Graph,selection:NativeViewSelection)->Result<Arc<FrozenContext>>{self.prepared.freeze(graph,selection,self.operation_control()).map_err(|e|e.to_string())}
    fn graph(&self,name:&str,phase:&str,ordered:&[&str],patch:&str)->Result<Graph>{self.graph_for_scope(name,phase,ordered,patch,None)}
    fn graph_for_scope(&self,name:&str,phase:&str,ordered:&[&str],patch:&str,scope:Option<&str>)->Result<Graph>{
        if Some(patch)!=option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256"){return Err("fixture patch differs from compiled engine".into())}
        self.build_execution(name,phase,ordered,EvaluatorProfile::Native,scope,self.operation_control()).map_err(|e|e.to_string())
    }
}
struct NativeOutput {bytes:usize,checked:usize,failed:usize,control:lycheedev_core::control::OperationControl}
impl NativeOutput {
    fn emit(&mut self,mut value:Value)->Result<()> {
        value["schema"]=json!("lycheedev.emmy-native-context-probe.v1");
        let mut bytes=serialize_json(&value,&self.control,2097152).map_err(|e|e.to_string())?;bytes.push(b'\n');self.bytes+=bytes.len();
        if self.bytes>2*1024*1024{return Err("2 MiB native output budget exceeded".into())}
        std::io::stdout().lock().write_all(&bytes).map_err(|e|e.to_string())
    }
    fn check(&mut self,name:&str,expected:Value,actual:Value)->Result<()> {
        let passed=expected==actual;self.checked+=1;if !passed{self.failed+=1}
        self.emit(json!({"event":"assertion","name":name,"expected":expected,"actual":actual,"passed":passed}))
    }
}
fn native_family(ty:&LuaType)->&'static str {
    match ty {
        LuaType::Number|LuaType::Integer|LuaType::IntegerConst(_)|LuaType::FloatConst(_)=>"number",
        LuaType::String|LuaType::StringConst(_)=>"string",
        LuaType::Unknown=>"unknown",LuaType::Any=>"any",LuaType::Nil=>"nil",
        _=>"other",
    }
}
fn marker_site(file:&str,marker:&str,expression:&str)->Result<Site> {
    let text=source(file);
    let crlf_marker;
    let needle=if text.contains(marker){marker}else{
        crlf_marker=marker.replace("\r\n","\n").replace('\n',"\r\n");
        &crlf_marker
    };
    let start=text.find(needle).ok_or_else(||format!("marker {marker}"))?;
    let relative=needle.find(expression).ok_or("marker expression")?;
    Ok(Site{file:file.into(),start:(start+relative)as u32,end:(start+relative+expression.len())as u32})
}
fn coverage_json(input:&NativeInput,session:&BindingSession)->Vec<Value> {
    session.coverage().iter().map(|record|json!({"site":{"file":input.files().iter().find(|(_,id)|**id==record.site.file_id).map(|(name,_)|name),"start":u32::from(record.site.syntax.get_range().start()),"end":u32::from(record.site.syntax.get_range().end())},"operation":record.operation,"state":record.state,"proofId":record.proof,"reason":record.reason})).collect()
}
fn binding_site_json(input:&NativeInput,site:BindingSite)->Value {
    json!({"file":input.files().iter().find(|(_,id)|**id==site.file_id).map(|(file,_)|file),"start":u32::from(site.syntax.get_range().start()),"end":u32::from(site.syntax.get_range().end())})
}
fn object_origin_json(input:&NativeInput,origin:&ObjectOrigin)->Value {
    match origin {
        ObjectOrigin::Lua(site)=>binding_site_json(input,*site),
        ObjectOrigin::Xml(span)=>json!({"file":span.path,"contentSHA256":span.content_sha256,"start":u32::from(span.range.start()),"end":u32::from(span.range.end())}),
    }
}
fn decision_json(input:&NativeInput,decision:&BindingDecision<BoundValue>)->Value {
    match decision {
        BindingDecision::Bound(value)=> {
            let selected=match &value.value {
                SelectedValue::Function(function)=>json!({"kind":"function","closure":binding_site_json(input,BindingSite{file_id:function.file_id,syntax:function.closure}),"declaration":format!("{:?}",function.declaration)}),
                SelectedValue::CallableSignature(signature)=>json!({"kind":"callable_signature","signature":format!("{signature:?}")}),
                SelectedValue::Object(object)=>json!({"kind":"object","objectId":object.identity,"epoch":object.epoch,"origin":object_origin_json(input,&object.origin)}),
                SelectedValue::Expression(site)=>json!({"kind":"expression","origin":binding_site_json(input,*site)}),
                SelectedValue::Parameter(decl)=>json!({"kind":"parameter","declaration":format!("{decl:?}")}),
                SelectedValue::DerivedNumber(number)=>json!({"kind":"derived_number","value":number.value,"origin":{"file":input.files().iter().find(|(_,id)|**id==number.origin.file_id).map(|(file,_)|file),"start":u32::from(number.origin.range.start()),"end":u32::from(number.origin.range.end())},"occurrence":number.occurrence_identity,"proof":number.proof}),
                SelectedValue::EnvironmentCallable(function)=>json!({"kind":"environment_callable","material":function.origin.material,"materialSHA256":function.origin.material_sha256,"pointer":function.origin.pointer,"identity":function.origin.identity}),
                SelectedValue::EnvironmentReturn(value)=>json!({"kind":"environment_return","material":value.callable.origin.material,"materialSHA256":value.callable.origin.material_sha256,"pointer":value.callable.origin.pointer,"slot":value.slot,"invocation":value.invocation_identity}),
            };
            json!({"state":"bound","valueIdentity":value.identity,"proofId":value.proof,"selectedValue":selected})
        },
        BindingDecision::KnownAbsent{proof}=>json!({"state":"known_absent","proofId":proof}),
        BindingDecision::Unavailable{proof,reason}=>json!({"state":"unavailable","proofId":proof,"reason":reason}),
        BindingDecision::NotManaged=>json!({"state":"not_managed"}),
    }
}
fn native_golden(out:&mut NativeOutput)->Result<()> {
    let patch=option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256").ok_or("missing compile-time context patch identity")?;
    if patch.len()!=64 || !patch.bytes().all(|b|b.is_ascii_hexdigit()){return Err("invalid compile-time patch identity".into())}
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":patch}))?;
    for name in NATIVE_FILES{out.emit(json!({"event":"input_identity","name":name,"file":name,"bytes":source(name).len(),"sha256":hash(source(name).as_bytes())}))?;}
    out.emit(json!({"event":"capabilities","nativeTypingProfile":NATIVE_PROFILE,"sourceExecution":false,"configExecution":false,"scope":"explicit native generic signatures, value-selected closures, copied fields, colon/dot self mapping; two native checker families", "notImplemented":["general flow fixpoint","general effects","native WoW helper implementation proof","general higher-order foreign callback inference","other native checker families","full Source acceptance"]}))?;
    let input=NativeInput::new(&out.control)?;
    let order_a=["profile.lua","generic-defs.lua","generic-calls.lua","generic-replace.lua"];
    let order_b=["profile.lua","generic-replace.lua","generic-defs.lua","generic-calls.lua"];
    let graphs=[input.graph("after-a","after_load",&order_a,patch)?,input.graph("unknown-a","unknown",&order_a,patch)?,input.graph("after-b","after_load",&order_b,patch)?];
    let providers:Vec<_>=graphs.iter().map(|g|input.freeze(g)).collect::<Result<_>>()?;
    let sessions:Vec<_>=providers.iter().map(|p|BindingSession::new(p.clone())).collect();
    let file_id=input.files()["generic-calls.lua"];
    // Same analysis database and live, separate model caches; interleave queries.
    let models:Vec<_>=sessions.iter().map(|session|input.analysis().semantic_model_with_context(file_id,session.clone())).collect();
    let queries=[
        ("identity_number","local numberResult = Identity(7)","Identity(7)","number"),
        ("identity_string","local stringResult = Identity(\"seven\")","Identity(\"seven\")","string"),
        ("copied_number","local copiedNumber = Copied.echo(7)","Copied.echo(7)","number"),
        ("replaced_string","local replacedString = Source.echo(\"seven\")","Source.echo(\"seven\")","string"),
        ("colon_number","local colonNumber = Methods:Echo(\"label\", 7)","Methods:Echo(\"label\", 7)","number"),
        ("dot_string","local dotString = Methods.Echo(Methods, \"label\", \"seven\")","Methods.Echo(Methods, \"label\", \"seven\")","string"),
        ("copied_return","function WrongCopiedReturn()\n    return Copied.echo(7)","Copied.echo(7)","number"),
        ("method_return","function WrongMethodReturn()\n    return Methods:Echo(\"label\", 7)","Methods:Echo(\"label\", 7)","number"),
        ("late_return","function WrongLateReturn()\n    return Identity(7)","Identity(7)","string"),
    ];
    for (label,marker,expression,base_expected) in queries {
        let source_site=marker_site("generic-calls.lua",marker,expression)?;let site=input.binding_site(&source_site)?;
        for (interleave_step,index) in [0usize,1,2,0,2,1].into_iter().enumerate() {
            let graph=&graphs[index];let model=&models[index];let ty=model.type_of_expr(site.syntax);
            let expected=if index==1 && label.ends_with("return"){"unknown"}else if index==2&&label=="late_return"{"number"}else{base_expected};
            let call=graph.calls.iter().find(|c|c.site==source_site).ok_or("graph call")?;
            let callee=&graph.uses[call.callee];let native_callee=input.binding_site(&callee.site)?;
            let tree=model.syntax_tree().ok_or("tree")?;let call_ast=LuaCallExpr::cast(site.syntax.to_node_from_root(&tree.get_red_root()).ok_or("call node")?).ok_or("call ast")?;
            let candidates=model.callable_candidates_for_expr(&call_ast.get_prefix_expr().ok_or("callee")?);
            let selected=providers[index].resolve(native_callee);
            let selected_json=decision_json(&input,&selected);
            let result_fact=&graph.uses[call.result_fact.ok_or("result")?];
            let query_coverage:Vec<_>=sessions[index].coverage().into_iter().filter(|r|r.site==site).collect();
            let result_coverage=query_coverage.iter().find(|record|record.operation=="call_result");
            let gate_coverage=query_coverage.iter().find(|record|record.operation=="call"&&record.state=="unavailable");
            let unknown_result=matches!(ty,LuaType::Unknown|LuaType::Any);
            let unavailable=unknown_result||result_coverage.is_none_or(|record|record.state!="native_projected");
            let unavailable_reason=if unavailable {
                result_coverage.and_then(|record|record.reason.clone())
                    .or_else(||gate_coverage.and_then(|record|record.reason.clone()))
                    .or_else(||Some(if unknown_result{"native_result_unknown_or_any"}else{"native_result_coverage_missing_or_unavailable"}.into()))
            }else{None};
            out.emit(json!({"event":"native_query","scenario":graph.scenario,"query":label,"interleaveStep":interleave_step,"snapshotKey":graph.snapshot_key,"sourceSite":source_site,"calleeDecision":selected_json,"calleeProofId":callee.proof_id,"nativeLuaType":format!("{ty:?}"),"nativeDisplay":humanize_semantic_type(model,&ty),"nativeFamily":native_family(&ty),
                "typeCoverage":{"state":if unavailable{"unavailable"}else{"native_projected"},"reason":unavailable_reason,"operation":"call_result","proofId":result_coverage.map(|record|&record.proof),"proofIds":[callee.proof_id,result_fact.proof_id]},
                "nativeCandidates":candidates.iter().map(|c|json!({"originFunction":selected_json["selectedValue"]["closure"],"parameters":c.get_params().iter().map(|(name,ty)|json!({"name":name,"nativeLuaType":ty.as_ref().map(|ty|format!("{ty:?}"))})).collect::<Vec<_>>(),"returnNativeLuaType":format!("{:?}",c.get_ret()),"isMethod":c.is_colon_define(),"genericParameters":c.get_generic_params().iter().map(|g|g.get_name()).collect::<Vec<_>>(),"nativeDebug":format!("{c:?}")})).collect::<Vec<_>>(),
                "bindingCoverage":selected_json["state"],"effectCoverage":call.effect_coverage,"nativeTypingProfile":NATIVE_PROFILE,"callFactId":call.id,"resultFactId":call.result_fact,"resultProofId":result_fact.proof_id,"contextIdentity":model.binding_context_identity()}))?;
            out.check(&format!("{}:{label}:interleave:{interleave_step}:native_type",graph.scenario),json!(expected),json!(native_family(&ty)))?;
            out.check(&format!("{}:{label}:interleave:{interleave_step}:model_context",graph.scenario),json!(graph.snapshot_key),json!(model.binding_context_identity()))?;
            out.check(&format!("{}:{label}:interleave:{interleave_step}:managed_candidate_count",graph.scenario),json!(if expected=="unknown"{0}else{1}),json!(candidates.len()))?;
        }
    }
    for(index,graph)in graphs.iter().enumerate(){
        let foreign=models[index].model_for(input.files()["generic-defs.lua"]);
        let parameter_site=marker_site("generic-defs.lua","function Identity(value)\n    return value","value")?;
        // Select the return use, not the parameter declaration occurrence.
        let return_start=source("generic-defs.lua").find("    return value").ok_or("foreign return")?+"    return ".len();
        let parameter_use=Site{file:parameter_site.file,start:return_start as u32,end:(return_start+5)as u32};
        let native_parameter=input.binding_site(&parameter_use)?;
        let parameter_decision=foreign.binding_decision(native_parameter.syntax);
        let parameter_type=foreign.type_of_expr(native_parameter.syntax);
        out.emit(json!({"event":"native_foreign_query","scenario":graph.scenario,"contextIdentity":foreign.binding_context_identity(),"sourceSite":parameter_use,"decision":decision_json(&input,&parameter_decision),"nativeLuaType":format!("{parameter_type:?}")}))?;
        out.check(&format!("{}:foreign_model_inherits_context",graph.scenario),json!(graph.snapshot_key),json!(foreign.binding_context_identity()))?;
        out.check(&format!("{}:foreign_formal_parameter_identity",graph.scenario),json!(true),json!(matches!(parameter_decision,BindingDecision::Bound(BoundValue{value:SelectedValue::Parameter(_),..}))))?;
        let diagnostics=input.analysis().diagnose_file_with_context(file_id,Arc::new(CheckConfig::new(input.config())),sessions[index].clone());
        let diagnostic_rows:Vec<_>=diagnostics.iter().map(|d|json!({"code":d.code.get_name(),"sourceSite":{"file":"generic-calls.lua","start":u32::from(d.range.start()),"end":u32::from(d.range.end())},"message":d.message})).collect();
        out.emit(json!({"event":"native_diagnostics","scenario":graph.scenario,"snapshotKey":graph.snapshot_key,"coordinate":"original_utf8_bytes","diagnostics":diagnostic_rows,"coverage":coverage_json(&input,&sessions[index])}))?;
        for(name,marker,expression,code,enabled)in [
            ("copied_wrong_return","function WrongCopiedReturn()\n    return Copied.echo(7)","Copied.echo(7)",DiagnosticCode::ReturnTypeMismatch,index!=1),
            ("method_wrong_return","function WrongMethodReturn()\n    return Methods:Echo(\"label\", 7)","Methods:Echo(\"label\", 7)",DiagnosticCode::ReturnTypeMismatch,index!=1),
            ("late_wrong_return","function WrongLateReturn()\n    return Identity(7)","Identity(7)",DiagnosticCode::ReturnTypeMismatch,index==0),
            ("wrong_label","local wrongLabel = Methods:Echo(99, 7)","99",DiagnosticCode::ParamTypeMismatch,true),
            ("late_wrong_argument","function WrongLateReturn()\n    return Identity(7)","7",DiagnosticCode::ParamTypeMismatch,index==0),
        ] {
            let site=marker_site("generic-calls.lua",marker,expression)?;
            let count=diagnostics.iter().filter(|d|d.code==code&&u32::from(d.range.start())==site.start&&u32::from(d.range.end())==site.end).count();
            out.check(&format!("{}:{name}:native_diagnostic_original_range",graph.scenario),json!(usize::from(enabled)),json!(count))?;
        }
        out.check(&format!("{}:native_diagnostic_total",graph.scenario),json!(match index{0=>5,1=>1,_=>3}),json!(diagnostics.len()))?;
        let coverage=sessions[index].coverage();
        let late=marker_site("generic-calls.lua","function WrongLateReturn()\n    return Identity(7)","Identity(7)")?;
        let late_site=input.binding_site(&late)?;
        out.check(&format!("{}:late_return_check_coverage",graph.scenario),json!(if index==1{"unavailable"}else{"native_checked"}),json!(coverage.iter().find(|r|r.site==late_site&&r.operation=="return_check").map(|r|r.state)))?;
        // Reference projection is exclusively the frozen value overlay.
        let copied_callee=marker_site("generic-calls.lua","local copiedNumber = Copied.echo(7)","Copied.echo")?;
        if let BindingDecision::Bound(value)=providers[index].resolve(input.binding_site(&copied_callee)?) {
            let references=sessions[index].references(&value.identity);
            out.emit(json!({"event":"native_references","scenario":graph.scenario,"valueIdentity":value.identity,"references":match &references{BindingDecision::Bound(refs)=>json!({"state":"bound","items":refs.iter().map(|r|json!({"site":binding_site_json(&input,r.site),"proofId":r.proof,"executionContext":r.execution_context})).collect::<Vec<_>>()}),_=>json!({"state":"unavailable"})},"rawIndexFallback":false}))?;
            let contains=match references{BindingDecision::Bound(refs)=>refs.iter().any(|r|r.site==input.binding_site(&copied_callee).expect("validated site")),_=>false};
            out.check(&format!("{}:copied_value_reference_overlay",graph.scenario),json!(true),json!(contains))?;
        }else{return Err("copied callee identity missing".into())}
        out.emit(json!({"event":"scenario","name":graph.scenario,"snapshotKey":graph.snapshot_key,"graphSHA256":digest(graph),"graph":graph,"fullSourceAcceptance":false}))?;
    }
    out.check("same_database_phase_order_context_keys_distinct",json!(3),json!(graphs.iter().map(|g|&g.snapshot_key).collect::<std::collections::BTreeSet<_>>().len()))?;
    Ok(())
}
fn native_coverage_red(out:&mut NativeOutput)->Result<()> {
    let patch=option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256").ok_or("missing compile-time context patch identity")?;
    if patch.len()!=64 || !patch.bytes().all(|b|b.is_ascii_hexdigit()){return Err("invalid compile-time patch identity".into())}
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":patch,"scope":"coverage-red"}))?;
    for name in COVERAGE_FILES{out.emit(json!({"event":"input_identity","name":name,"file":name,"bytes":source(name).len(),"sha256":hash(source(name).as_bytes())}))?;}
    out.emit(json!({"event":"capabilities","scope":"coverage-red","nativeTypingProfile":NATIVE_PROFILE,"sourceExecution":false,"configExecution":false,"fullSourceAcceptance":false}))?;
    let input=NativeInput::from_files(COVERAGE_FILES,&out.control)?;
    let graph=input.graph_for_scope("fixed-return-unknown-argument","after_load",COVERAGE_FILES,patch,Some("fixed-return-unknown-argument-v1"))?;
    let provider=input.freeze(&graph)?;
    let session=BindingSession::new(provider.clone());
    let file="coverage-unknown-argument.lua";let file_id=input.files()[file];
    let model=input.analysis().semantic_model_with_context(file_id,session.clone());
    let source_site=Site{file:file.into(),start:98,end:122};
    if source_site.text(input.inputs()).map_err(|e|e.to_string())?!="Replacement(object[key])"{return Err("coverage fixture call byte range mismatch".into())}
    let site=input.binding_site(&source_site)?;
    let call=graph.calls.iter().find(|call|call.site==source_site).ok_or("coverage graph call")?;
    let callee=&graph.uses[call.callee];let result=&graph.uses[call.result_fact.ok_or("coverage result fact")?];
    let argument=&graph.uses[*call.arguments.first().ok_or("coverage argument")?];
    let selected=provider.resolve(input.binding_site(&callee.site)?);
    let selected_json=decision_json(&input,&selected);
    let argument_decision=provider.resolve(input.binding_site(&argument.site)?);
    let ty=model.type_of_expr(site.syntax);
    let call_ast=LuaCallExpr::cast(site.syntax.to_node_from_root(&model.syntax_tree().ok_or("coverage tree")?.get_red_root()).ok_or("coverage call node")?).ok_or("coverage call AST")?;
    let candidates=model.callable_candidates_for_expr(&call_ast.get_prefix_expr().ok_or("coverage callee")?);
    let diagnostics=input.analysis().diagnose_file_with_context(file_id,Arc::new(CheckConfig::new(input.config())),session.clone());
    let coverage=session.coverage();
    let call_coverage=|operation:&str|coverage.iter().find(|record|record.site==site&&record.operation==operation);
    let result_coverage=call_coverage("call_result");
    out.emit(json!({"event":"native_query","scope":"coverage-red","scenario":graph.scenario,"query":"fixed_return_unknown_argument","snapshotKey":graph.snapshot_key,
        "sourceSite":source_site,"calleeDecision":selected_json,"calleeProofId":callee.proof_id,"argumentDecision":decision_json(&input,&argument_decision),"argumentFactId":argument.id,"argumentProofId":argument.proof_id,
        "nativeLuaType":format!("{ty:?}"),"nativeFamily":native_family(&ty),"nativeDisplay":humanize_semantic_type(&model,&ty),
        "typeCoverage":{"state":result_coverage.map(|record|record.state),"reason":result_coverage.and_then(|record|record.reason.clone()),"operation":"call_result","proofId":result_coverage.map(|record|&record.proof),"proofIds":[callee.proof_id,result.proof_id]},
        "nativeCandidates":candidates.iter().map(|candidate|json!({"originFunction":selected_json["selectedValue"]["closure"],"parameters":candidate.get_params().iter().map(|(name,ty)|json!({"name":name,"nativeLuaType":ty.as_ref().map(|ty|format!("{ty:?}"))})).collect::<Vec<_>>(),"returnNativeLuaType":format!("{:?}",candidate.get_ret()),"isMethod":candidate.is_colon_define(),"genericParameters":candidate.get_generic_params().iter().map(|g|g.get_name()).collect::<Vec<_>>(),"nativeDebug":format!("{candidate:?}")})).collect::<Vec<_>>(),
        "bindingCoverage":selected_json["state"],"effectCoverage":call.effect_coverage,"nativeTypingProfile":NATIVE_PROFILE,"callFactId":call.id,"resultFactId":result.id,"resultProofId":result.proof_id,"contextIdentity":model.binding_context_identity()}))?;
    let diagnostic_rows:Vec<_>=diagnostics.iter().map(|diagnostic|json!({"code":diagnostic.code.get_name(),"sourceSite":{"file":file,"start":u32::from(diagnostic.range.start()),"end":u32::from(diagnostic.range.end())},"message":diagnostic.message})).collect();
    out.emit(json!({"event":"native_diagnostics","scope":"coverage-red","scenario":graph.scenario,"snapshotKey":graph.snapshot_key,"coordinate":"original_utf8_bytes","diagnostics":diagnostic_rows,"coverage":coverage_json(&input,&session)}))?;
    out.check("coverage_red:fixture_bytes",json!(127),json!(source(file).len()))?;
    out.check("coverage_red:input_file_count",json!(5),json!(input.files().len()))?;
    out.check("coverage_red:callee_bound_closure",json!(true),json!(matches!(selected,BindingDecision::Bound(BoundValue{value:SelectedValue::Function(_),..}))))?;
    out.check("coverage_red:argument_binding",json!("unavailable"),decision_json(&input,&argument_decision)["state"].clone())?;
    out.check("coverage_red:native_result_type",json!("String"),json!(format!("{ty:?}")))?;
    out.check("coverage_red:native_result_coverage",json!("native_projected"),json!(result_coverage.map(|record|record.state)))?;
    out.check("coverage_red:native_argument_coverage",json!("unavailable"),json!(call_coverage("argument_check").map(|record|record.state)))?;
    out.check("coverage_red:native_return_coverage",json!("native_checked"),json!(call_coverage("return_check").map(|record|record.state)))?;
    out.check("coverage_red:native_diagnostic_exact",json!([{"code":"return-type-mismatch","start":98,"end":122}]),json!(diagnostics.iter().map(|diagnostic|json!({"code":diagnostic.code.get_name(),"start":u32::from(diagnostic.range.start()),"end":u32::from(diagnostic.range.end())})).collect::<Vec<_>>()))?;
    out.check("coverage_red:coverage_same_result_proof",json!(true),json!(["call_result","argument_check","return_check"].iter().all(|operation|call_coverage(operation).is_some_and(|record|record.proof==result.proof_id))))?;
    out.check("coverage_red:model_context",json!(graph.snapshot_key),json!(model.binding_context_identity()))?;
    out.check("coverage_red:single_native_string_candidate",json!(true),json!(candidates.len()==1&&matches!(candidates[0].get_ret(),LuaType::String)&&candidates[0].get_generic_params().is_empty()))?;
    out.emit(json!({"event":"scenario","name":graph.scenario,"scope":"coverage-red","snapshotKey":graph.snapshot_key,"graphSHA256":digest(&graph),"graph":graph,"fullSourceAcceptance":false}))?;
    Ok(())
}
pub fn native_main() {
    let mut out=NativeOutput{bytes:0,checked:0,failed:0,control:fixture_control()};
    let args:Vec<_>=std::env::args().skip(1).collect();
    let result=if args==["golden"]{native_golden(&mut out)}else if args==["coverage-red"]{native_coverage_red(&mut out)}else if args==["object-red"]{native_object_red(&mut out)}else if args==["object-audit-red"]{native_object_audit_red(&mut out)}else if args==["invocation-lifecycle"]{native_invocation_lifecycle(&mut out)}else if args==["invocation-callsite-red"]{native_invocation_lifecycle_selected(&mut out,"call-capture")}else if args==["invocation-return-slots"]{native_return_slots(&mut out)}else{Err("usage: emmy_native_context_probe golden|coverage-red|object-red|object-audit-red|invocation-lifecycle|invocation-callsite-red|invocation-return-slots".into())};
    let complete=result.is_ok();let error=result.err();
    if let Err(error)=out.emit(json!({"event":"end","complete":complete,"checked":out.checked,"failed":out.failed,"businessBaselinePassed":complete&&out.failed==0,"fullSourceAcceptance":false,"error":error})){eprintln!("{error}");std::process::exit(1)}
    if !complete||out.failed!=0{std::process::exit(1)}
}
