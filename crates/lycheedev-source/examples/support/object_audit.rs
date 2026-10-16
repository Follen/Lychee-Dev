// Execute frozen audit inputs through the real graph and native semantic APIs.
const AUDIT_CASES: &[(&str, &str, &[&str])] = &[
    ("rebind", "number", &["profile.lua", "audit-rebind.lua"]),
    ("colon", "string", &["profile.lua", "audit-colon.lua"]),
    (
        "field-closure",
        "string",
        &["profile.lua", "audit-field-closure.lua"],
    ),
    (
        "expression",
        "string",
        &["profile.lua", "audit-expression.lua"],
    ),
];
const AUDIT_UNION_FILES: &[&str] = &["profile.lua", "audit-record-union.lua"];
fn native_object_audit_red(out: &mut NativeOutput) -> Result<()> {
    let patch =
        option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256").ok_or("missing compiled patch")?;
    out.emit(
        json!({"event":"build_identity","compiledPatchSHA256":patch,"scope":"object-audit-red"}),
    )?;
    for name in [
        "profile.lua",
        "audit-rebind.lua",
        "audit-colon.lua",
        "audit-field-closure.lua",
        "audit-expression.lua",
        "audit-record-union.lua",
    ] {
        out.emit(json!({"event":"input_identity","name":name,"file":name,"bytes":source(name).len(),"sha256":hash(source(name).as_bytes())}))?;
    }
    for (name, expected, files) in AUDIT_CASES {
        let input = NativeInput::from_files(files, &out.control)?;
        let graph = input.graph_for_scope(
            name,
            "after_load",
            files,
            patch,
            Some("independent-object-audit-v1"),
        )?;
        let file = files[1];
        let expression = if *name == "rebind" {
            "Alias.value"
        } else if *name == "field-closure" {
            "hidden.value"
        } else {
            "o.value"
        };
        let site = marker_site(file, &format!("local after = {expression}"), expression)?;
        let fact = object_fact(&graph, &site)?;
        let provider = input.freeze(&graph)?;
        let session = probe_session(provider.clone(), input.operation_control())?;
        let mut check = || input.operation_control().check();
        let mut model = probe_query(&input, &session, input.files()[file], &mut check);
        let ty = model.type_of_expr(input.binding_site(&site)?.syntax)?;
        out.emit(json!({"event":"audit_query","case":name,"sourceSite":site,"factId":fact.id,"decision":fact.decision,"proofId":fact.proof_id,"nativeLuaType":format!("{:?}",ty.structural_type()),"nativeFamily":object_type_label(ty.structural_type()),"coverage":coverage_json(&input,&session)?}))?;
        out.check(
            &format!("audit:{name}:exact_after_value"),
            json!(expected),
            json!(object_type_label(ty.structural_type())),
        )?;
        out.emit(json!({"event":"scenario","name":name,"snapshotKey":graph.snapshot_key,"graph":graph,"fullSourceAcceptance":false}))?;
    }
    let input = NativeInput::from_files(AUDIT_UNION_FILES, &out.control)?;
    let graph = input.graph_for_scope(
        "record-union",
        "after_load",
        AUDIT_UNION_FILES,
        patch,
        Some("independent-object-audit-v1"),
    )?;
    let provider = input.freeze(&graph)?;
    let session = probe_session(provider.clone(), input.operation_control())?;
    let file = "audit-record-union.lua";
    let id = input.files()[file];
    let mut check = || input.operation_control().check();
    let mut model = probe_query(&input, &session, id, &mut check);
    for (name, function, marker) in [
        ("union", "NeedUnion", "local unionBad = NeedUnion(bad)"),
        (
            "nullable",
            "NeedNullable",
            "local nullableBad = NeedNullable(bad)",
        ),
    ] {
        let declaration = graph
            .functions
            .iter()
            .find(|value| value.name == function && value.declaration.file == file)
            .ok_or("audit signature")?;
        let signature = model
            .type_of_signature_in_file(id, input.binding_site(&declaration.closure)?.syntax)?
            .ok_or("audit native signature")?;
        let expected = signature
            .function_parameters()
            .and_then(|params| params.into_iter().next().flatten())
            .ok_or("audit native parameter")?;
        let bad = marker_site(file, marker, "bad")?;
        let good = marker_site(
            file,
            &format!("local {name}Good = {function}(good)"),
            "good",
        )?;
        let bad_type = model.type_of_expr(input.binding_site(&bad)?.syntax)?;
        let good_type = model.type_of_expr(input.binding_site(&good)?.syntax)?;
        out.emit(json!({"event":"audit_record_type","case":name,"nativeParameter":format!("{:?}",expected.structural_type()),"nativeSignature":format!("{:?}",signature.structural_type()),"bad":format!("{:?}",bad_type.structural_type()),"good":format!("{:?}",good_type.structural_type())}))?;
        for (label, ty, want) in [
            ("bad", bad_type, false),
            ("good", good_type, true),
            ("nil", OwnedValue::plain(LuaType::Nil), true),
        ] {
            out.check(
                &format!("audit:{name}:{label}:native_compatibility"),
                json!(want),
                json!(model.type_check(&ty, &expected)?),
            )?;
        }
    }
    let diagnostics = probe_diagnostics(&input, &session, id)?;
    let actual:Vec<_>=diagnostics.iter().map(|row|json!({"code":row.code.get_name(),"start":u32::from(row.range.start()),"end":u32::from(row.range.end())})).collect();
    let expected: Vec<_> = [
        "local unionBad = NeedUnion(bad)",
        "local nullableBad = NeedNullable(bad)",
    ]
    .iter()
    .map(|marker| {
        let site = marker_site(file, marker, "bad").expect("fixed audit marker");
        json!({"code":"param-type-mismatch","start":site.start,"end":site.end})
    })
    .collect();
    out.emit(json!({"event":"audit_diagnostics","case":"record-union","actual":actual,"coverage":coverage_json(&input,&session)?}))?;
    out.check(
        "audit:record_union:exact_two_argument_diagnostics",
        json!(expected),
        json!(actual),
    )?;
    out.emit(json!({"event":"scenario","name":"record-union","snapshotKey":graph.snapshot_key,"graph":graph,"fullSourceAcceptance":false}))?;
    Ok(())
}
