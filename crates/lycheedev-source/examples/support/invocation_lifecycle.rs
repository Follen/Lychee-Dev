type LifecycleQuery = (&'static str, &'static str, &'static str, &'static str);
type LifecycleCase = (&'static str, &'static [&'static str], &'static [LifecycleQuery]);
const LIFECYCLE_CASES: &[LifecycleCase] = &[
    ("captures",&["profile.lua","invocation-captures.lua"],&[
        ("first_before","local observedFirstBefore = firstBefore","firstBefore","number"),
        ("second_before","local observedSecondBefore = secondBefore","secondBefore","string"),
        ("first_after","local observedFirstAfter = firstAfter","firstAfter","string"),
        ("second_after","local observedSecondAfter = secondAfter","secondAfter","string"),
        ("first_call","local firstBefore = first(nil)","first(nil)","number"),
        ("second_call","local secondBefore = second(nil)","second(nil)","string"),
    ]),
    ("reassign",&["profile.lua","invocation-reassign.lua"],&[
        ("original","local afterOriginal = original.value","original.value","number"),
        ("rebound","local afterRebound = rebound.value","rebound.value","string"),
        ("method_self","local afterSelf = same.value","same.value","number"),
    ]),
    ("short-circuit",&["profile.lua","invocation-short-circuit.lua"],&[
        ("before_right","local beforeRight = Trace.b","Trace.b","number"),
        ("after_right","local afterRight = Trace.b","Trace.b","string"),
        ("false_and","local observedAnd = selectedAnd","selectedAnd","boolean"),
        ("zero_or","local observedZero = zeroTruthy","zeroTruthy","number"),
    ]),
    ("selector",&["profile.lua","invocation-selector.lua"],&[
        ("first","local observedFirst = first","first","number"),
        ("second","local observedSecond = second","second","string"),
    ]),
    ("escape",&["profile.lua","invocation-escape.lua"],&[
        ("retained_object","local afterEscaped = t.value","t.value","unknown"),
        ("unescaped_local","local afterUntouched = untouched.value","untouched.value","number"),
    ]),
    ("escape-closure",&["profile.lua","invocation-escape-closure.lua"],&[
        ("retained_closure","local afterEscapedClosure = hidden.value","hidden.value","unknown"),
    ]),
    ("escape-rebind",&["profile.lua","invocation-escape-rebind.lua"],&[
        ("new_captured_object","local escapedCurrent = hidden.value","hidden.value","unknown"),
        ("unescaped_local","local untouchedCurrent = untouched.value","untouched.value","number"),
    ]),
    ("call-capture",&["profile.lua","invocation-call-capture.lua"],&[
        ("first_call","local first = f()","f()","number"),
        ("second_call","local second = f()","f()","string"),
        ("first_read","local firstRead = first","first","number"),
        ("second_read","local secondRead = second","second","string"),
    ]),
];
fn native_invocation_lifecycle(out:&mut NativeOutput)->Result<()> {
    native_invocation_lifecycle_selected(out,"")
}
fn lifecycle_site(file:&str,marker:&str,expression:&str)->Result<Site> {
    let text=source(file);let start=text.find(marker).ok_or("lifecycle marker")?;
    if text[start+marker.len()..].contains(marker){return Err("lifecycle marker not unique".into())}
    let offset=marker.rfind(expression).ok_or("lifecycle expression")?;
    Ok(Site{file:file.into(),start:(start+offset)as u32,end:(start+offset+expression.len())as u32})
}
fn native_invocation_lifecycle_selected(out:&mut NativeOutput,only:&str)->Result<()> {
    let patch=option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256").ok_or("missing compiled patch")?;
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":patch,"scope":"invocation-lifecycle"}))?;
    for name in ["profile.lua","invocation-captures.lua","invocation-reassign.lua","invocation-short-circuit.lua","invocation-selector.lua","invocation-escape.lua","invocation-escape-closure.lua","invocation-escape-rebind.lua","invocation-call-capture.lua"]{
        out.emit(json!({"event":"input_identity","name":name,"file":name,"bytes":source(name).len(),"sha256":hash(source(name).as_bytes())}))?;
    }
    for(name,files,queries)in LIFECYCLE_CASES{
        if !only.is_empty()&&*name!=only{continue}
        let input=NativeInput::from_files(files,&out.control)?;
        let graph=input.graph_for_scope(name,"after_load",files,patch,Some("invocation-lifecycle-v1"))?;
        let provider=input.freeze(&graph)?;let session=BindingSession::new(provider);let file=files[1];
        let model=input.analysis().semantic_model_with_context(input.files()[file],session.clone());
        for(label,marker,expression,expected)in *queries{
            let site=lifecycle_site(file,marker,expression)?;let fact=object_fact(&graph,&site)?;
            let ty=model.type_of_expr(input.binding_site(&site)?.syntax);
            out.emit(json!({"event":"lifecycle_query","scenario":name,"query":label,"sourceSite":site,"factId":fact.id,"proofId":fact.proof_id,"decision":fact.decision,"nativeLuaType":format!("{ty:?}"),"nativeFamily":object_type_label(&ty),"contextIdentity":model.binding_context_identity()}))?;
            out.check(&format!("lifecycle:{name}:{label}:native_value"),json!(expected),json!(object_type_label(&ty)))?;
        }
        if *name=="selector"{
            let body=marker_site(file,"return copy","copy")?;
            for(label,marker,expression,expected)in [("first","local first = Identity(1)","Identity(1)","number"),("second","local second = Identity(\"two\")","Identity(\"two\")","string")]{
                let site=marker_site(file,marker,expression)?;let call=graph.calls.iter().find(|call|call.site==site&&call.context.starts_with("load:")).ok_or("selected lifecycle call")?;
                let frames=&graph.native_invocations.as_ref().ok_or("invocations")?.frames;
                let frame=frames.iter().find(|frame|frame.call==Some(call.id)).ok_or("selected lifecycle frame")?;
                let provider=input.freeze_for_view(&graph,NativeViewSelection::Invocation(frame.id))?;let selected_session=BindingSession::new(provider);
                let selected=input.analysis().semantic_model_with_context(input.files()[file],selected_session.clone());
                let ty=selected.type_of_expr(input.binding_site(&body)?.syntax);
                out.emit(json!({"event":"selected_frame_query","scenario":name,"query":label,"frame":frame.id,"call":call.id,"sourceSite":body,"nativeLuaType":format!("{ty:?}"),"nativeFamily":object_type_label(&ty),"contextIdentity":selected.binding_context_identity()}))?;
                out.check(&format!("lifecycle:selector:{label}:selected_body_value"),json!(expected),json!(object_type_label(&ty)))?;
                out.check(&format!("lifecycle:selector:{label}:separate_cache_identity"),json!(true),json!(selected.binding_context_identity()!=model.binding_context_identity()))?;
            }
        }
        out.emit(json!({"event":"scenario","name":name,"snapshotKey":graph.snapshot_key,"graph":graph,"fullSourceAcceptance":false}))?;
    }
    Ok(())
}
