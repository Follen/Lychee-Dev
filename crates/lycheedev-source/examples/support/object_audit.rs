// Execute frozen audit inputs through the real graph and native semantic APIs.
const AUDIT_CASES: &[(&str,&str,&[&str])] = &[
    ("rebind","number",&["profile.lua","audit-rebind.lua"]),
    ("colon","string",&["profile.lua","audit-colon.lua"]),
    ("field-closure","string",&["profile.lua","audit-field-closure.lua"]),
    ("expression","string",&["profile.lua","audit-expression.lua"]),
];
const AUDIT_UNION_FILES: &[&str] = &["profile.lua","audit-record-union.lua"];
fn native_object_audit_red(out:&mut NativeOutput)->Result<()> {
    let patch=option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256").ok_or("missing compiled patch")?;
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":patch,"scope":"object-audit-red"}))?;
    for name in ["profile.lua","audit-rebind.lua","audit-colon.lua","audit-field-closure.lua","audit-expression.lua","audit-record-union.lua"] {
        out.emit(json!({"event":"input_identity","name":name,"file":name,"bytes":source(name).len(),"sha256":hash(source(name).as_bytes())}))?;
    }
    for(name,expected,files)in AUDIT_CASES {
        let input=NativeInput::from_files(files,&out.control)?;
        let graph=input.graph_for_scope(name,"after_load",files,patch,Some("independent-object-audit-v1"))?;
        let file=files[1];let expression=if *name=="rebind"{"Alias.value"}else if *name=="field-closure"{"hidden.value"}else{"o.value"};
        let site=marker_site(file,&format!("local after = {expression}"),expression)?;
        let fact=object_fact(&graph,&site)?;
        let provider=input.freeze(&graph)?;let session=BindingSession::new(provider);
        let model=input.analysis().semantic_model_with_context(input.files()[file],session.clone());
        let ty=model.type_of_expr(input.binding_site(&site)?.syntax);
        out.emit(json!({"event":"audit_query","case":name,"sourceSite":site,"factId":fact.id,"decision":fact.decision,"proofId":fact.proof_id,"nativeLuaType":format!("{ty:?}"),"nativeFamily":object_type_label(&ty),"coverage":coverage_json(&input,&session)}))?;
        out.check(&format!("audit:{name}:exact_after_value"),json!(expected),json!(object_type_label(&ty)))?;
        out.emit(json!({"event":"scenario","name":name,"snapshotKey":graph.snapshot_key,"graph":graph,"fullSourceAcceptance":false}))?;
    }
    let input=NativeInput::from_files(AUDIT_UNION_FILES,&out.control)?;
    let graph=input.graph_for_scope("record-union","after_load",AUDIT_UNION_FILES,patch,Some("independent-object-audit-v1"))?;
    let provider=input.freeze(&graph)?;let session=BindingSession::new(provider);let file="audit-record-union.lua";let id=input.files()[file];
    let model=input.analysis().semantic_model_with_context(id,session.clone());
    for(name,function,marker)in [("union","NeedUnion","local unionBad = NeedUnion(bad)"),("nullable","NeedNullable","local nullableBad = NeedNullable(bad)")] {
        let declaration=graph.functions.iter().find(|value|value.name==function&&value.declaration.file==file).ok_or("audit signature")?;
        let signature=model.type_of_signature_in_file(id,input.binding_site(&declaration.closure)?.syntax).ok_or("audit native signature")?;
        let expected=signature.get_params().first().and_then(|(_,ty)|ty.clone()).ok_or("audit native parameter")?;
        let bad=marker_site(file,marker,"bad")?;
        let good=marker_site(file,&format!("local {name}Good = {function}(good)"),"good")?;
        let bad_type=model.type_of_expr(input.binding_site(&bad)?.syntax);let good_type=model.type_of_expr(input.binding_site(&good)?.syntax);
        out.emit(json!({"event":"audit_record_type","case":name,"nativeParameter":format!("{expected:?}"),"nativeSignature":format!("{signature:?}"),"bad":format!("{bad_type:?}"),"good":format!("{good_type:?}")}))?;
        for(label,ty,want)in [("bad",bad_type,false),("good",good_type,true),("nil",LuaType::Nil,true)]{
            out.check(&format!("audit:{name}:{label}:native_compatibility"),json!(want),json!(model.type_check(&ty,&expected)))?;
        }
    }
    let diagnostics=input.analysis().diagnose_file_with_context(id,Arc::new(CheckConfig::new(input.config())),session.clone());
    let actual:Vec<_>=diagnostics.iter().map(|row|json!({"code":row.code.get_name(),"start":u32::from(row.range.start()),"end":u32::from(row.range.end())})).collect();
    let expected:Vec<_>=["local unionBad = NeedUnion(bad)","local nullableBad = NeedNullable(bad)"].iter().map(|marker|{let site=marker_site(file,marker,"bad").expect("fixed audit marker");json!({"code":"param-type-mismatch","start":site.start,"end":site.end})}).collect();
    out.emit(json!({"event":"audit_diagnostics","case":"record-union","actual":actual,"coverage":coverage_json(&input,&session)}))?;
    out.check("audit:record_union:exact_two_argument_diagnostics",json!(expected),json!(actual))?;
    out.emit(json!({"event":"scenario","name":"record-union","snapshotKey":graph.snapshot_key,"graph":graph,"fullSourceAcceptance":false}))?;
    Ok(())
}
