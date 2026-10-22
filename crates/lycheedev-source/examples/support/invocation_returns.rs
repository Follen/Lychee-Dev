use emmylua_code_analysis::VariadicType;
const RETURN_QUERIES: &[(&str, &str, &str, &[&str])] = &[
    (
        "scalar_empty",
        "local scalarZero = Empty()",
        "Empty()",
        &["nil"],
    ),
    (
        "tail_empty",
        "local emptyA, emptyB = Empty()",
        "Empty()",
        &[],
    ),
    (
        "scalar_pair",
        "local scalarPair = Pair()",
        "Pair()",
        &["number"],
    ),
    (
        "tail_pair",
        "local pairA, pairB = Pair()",
        "Pair()",
        &["number", "string"],
    ),
    (
        "nonlast_pair",
        "local prefixA, prefixB = Pair(), 9",
        "Pair()",
        &["number"],
    ),
    (
        "paren_pair",
        "local parenA, parenB = (Pair())",
        "Pair()",
        &["number"],
    ),
    (
        "forward_pair",
        "local forwardA, forwardB = Forward()",
        "Forward()",
        &["number", "string"],
    ),
    (
        "single_forward",
        "local singleA, singleB = Parenthesized()",
        "Parenthesized()",
        &["number"],
    ),
    (
        "explicit_return",
        "local contractValue = Annotated()",
        "Annotated()",
        &["string"],
    ),
    (
        "empty_padding",
        "local readEmptyB = emptyB",
        "emptyB",
        &["nil"],
    ),
    (
        "pair_second",
        "local readPairB = pairB",
        "pairB",
        &["string"],
    ),
    (
        "paren_padding",
        "local readParenB = parenB",
        "parenB",
        &["nil"],
    ),
    (
        "forward_second",
        "local readForwardB = forwardB",
        "forwardB",
        &["string"],
    ),
    (
        "single_padding",
        "local readSingleB = singleB",
        "singleB",
        &["nil"],
    ),
];
const RETURN_FILES: &[&str] = &["profile.lua", "invocation-return-slots.lua"];
fn native_return_slots(out: &mut NativeOutput) -> Result<()> {
    out.emit(json!({"event":"build_identity","engineCommit":ENGINE_COMMIT,"engineArchiveSHA256":ENGINE_ARCHIVE_SHA256,"scope":"invocation-return-slots"}))?;
    let files = RETURN_FILES;
    for file in files {
        out.emit(json!({"event":"input_identity","name":file,"file":file,"bytes":source(file).len(),"sha256":hash(source(file).as_bytes())}))?;
    }
    let input = NativeInput::from_files(files, &out.control)?;
    let graph = input.graph_for_scope(
        "return-slots",
        "after_load",
        files,
        Some("invocation-return-slots-v1"),
    )?;
    let provider = input.freeze(&graph)?;
    let session = probe_session(provider.clone(), input.operation_control())?;
    let mut check = || input.operation_control().check();
    let mut model = probe_query(&input, &session, input.files()[files[1]], &mut check);
    for (label, marker, expression, expected) in RETURN_QUERIES {
        let site = lifecycle_site(files[1], marker, expression)?;
        let ty = model.type_of_expr(input.binding_site(&site)?.syntax)?;
        let slots = match ty.structural_type() {
            LuaType::Variadic(values) => match values.as_ref() {
                VariadicType::Multi(values) => {
                    values.iter().map(object_type_label).collect::<Vec<_>>()
                }
                VariadicType::Base(ty) => vec![object_type_label(ty)],
            },
            _ => vec![object_type_label(ty.structural_type())],
        };
        out.emit(json!({"event":"return_slot_query","query":label,"sourceSite":site,"nativeLuaType":format!("{:?}",ty.structural_type()),"slots":slots,"contextIdentity":model.binding_context_identity()}))?;
        out.check(
            &format!("return_slots:{label}:native_slots"),
            json!(expected),
            json!(slots),
        )?;
    }
    out.emit(json!({"event":"scenario","name":"return-slots","snapshotKey":graph.snapshot_key,"graph":graph,"fullSourceAcceptance":false}))?;
    Ok(())
}
