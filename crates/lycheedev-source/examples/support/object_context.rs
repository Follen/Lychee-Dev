// Specification for actual native object APIs. The probe reads frozen shapes;
// it supplies no reconstructed shape, native type or compatibility result.
use emmylua_code_analysis::LuaMemberKey;
use lycheedev_emmy_adapter::session::OwnedMemberInfo as MemberInfo;
const OBJECT_FILES: &[&str] = &[
    "profile.lua",
    "generic-defs.lua",
    "generic-calls.lua",
    "generic-replace.lua",
    "object-versions.lua",
];
const OBJECT_ORDER: &[&str] = &[
    "profile.lua",
    "generic-replace.lua",
    "generic-defs.lua",
    "generic-calls.lua",
    "object-versions.lua",
];
const OBJECT_FILE: &str = "object-versions.lua";

struct ObjectSpec {
    label: &'static str,
    marker: &'static str,
    object: &'static str,
    reads: &'static str,
    value: &'static str,
    run: &'static str,
    added: bool,
    compatible: bool,
    dynamic: bool,
}
const OBJECT_SPECS: &[ObjectSpec] = &[
    ObjectSpec {
        label: "source_old",
        marker: "local good = NeedNumber(Source)",
        object: "Source",
        reads: "sourceOld",
        value: "number",
        run: "generic_function",
        added: false,
        compatible: true,
        dynamic: false,
    },
    ObjectSpec {
        label: "source_new",
        marker: "local bad = NeedNumber(Source)",
        object: "Source",
        reads: "sourceNew",
        value: "string",
        run: "string_function",
        added: true,
        compatible: false,
        dynamic: false,
    },
    ObjectSpec {
        label: "copied_old",
        marker: "local stillGood = NeedNumber(Copied)",
        object: "Copied",
        reads: "copiedOld",
        value: "number",
        run: "generic_function",
        added: false,
        compatible: true,
        dynamic: false,
    },
    ObjectSpec {
        label: "copied_new",
        marker: "local copiedBad = NeedNumber(Copied)",
        object: "Copied",
        reads: "copiedNew",
        value: "string",
        run: "string_function",
        added: true,
        compatible: false,
        dynamic: false,
    },
    ObjectSpec {
        label: "twin",
        marker: "local twinValue = Twin.value",
        object: "Twin",
        reads: "twin",
        value: "string",
        run: "string_function",
        added: true,
        compatible: false,
        dynamic: false,
    },
    ObjectSpec {
        label: "dynamic",
        marker: "local dynamic = NeedNumber(Source)",
        object: "Source",
        reads: "dynamic",
        value: "unknown",
        run: "unknown",
        added: true,
        compatible: false,
        dynamic: true,
    },
];
fn object_type_label(ty: &LuaType) -> &'static str {
    match ty {
        LuaType::Boolean | LuaType::BooleanConst(_) => "boolean",
        LuaType::DocFunction(function) if !function.get_generic_params().is_empty() => {
            "generic_function"
        }
        LuaType::DocFunction(function) if matches!(function.get_ret(), LuaType::String) => {
            "string_function"
        }
        LuaType::DocFunction(_) | LuaType::Function | LuaType::Signature(_) => "function",
        _ => native_family(ty),
    }
}
fn object_member_json(member: &MemberInfo) -> Value {
    json!({"key":member.key.name(),"nativeLuaType":format!("{:?}",member.typ.structural_type()),"label":object_type_label(member.typ.structural_type()),"declaration":member.id.as_ref().map(|id|format!("{id:?}")),"fileId":member.file_id.map(|id|format!("{id:?}")),"isMethod":member.is_method})
}
fn object_api_matrix(model: &mut ProbeModel<'_, '_>, ty: &OwnedValue) -> Result<(Value, Value)> {
    let members = model.member_infos(ty)?;
    let mut keys: Vec<_> = members
        .iter()
        .filter_map(|member| member.key.name().map(str::to_owned))
        .collect();
    keys.sort();
    keys.dedup();
    let mut summary = json!({"keys":keys,"fields":{}});
    let mut raw = json!({"memberInfos":members.iter().map(object_member_json).collect::<Vec<_>>(),"fields":{}});
    for field in ["value", "run", "added"] {
        let key = LuaMemberKey::Name(field.into());
        let one = model.member_info(ty, &key)?;
        let selected = model.member_infos_with_key(ty, &key)?;
        let all = model.member_infos_with_key_all(ty, &key)?;
        let member_type = model.member_type(ty, &key)?;
        summary["fields"][field] = json!({"memberInfo":one.as_ref().map(|m|object_type_label(m.typ.structural_type())),"withKey":selected.iter().map(|m|object_type_label(m.typ.structural_type())).collect::<Vec<_>>(),"withKeyAll":all.iter().map(|m|object_type_label(m.typ.structural_type())).collect::<Vec<_>>(),"memberType":member_type.as_ref().map(|ty|object_type_label(ty.structural_type()))});
        raw["fields"][field] = json!({"memberInfo":one.as_ref().map(object_member_json),"withKey":selected.iter().map(object_member_json).collect::<Vec<_>>(),"withKeyAll":all.iter().map(object_member_json).collect::<Vec<_>>(),"memberType":member_type.map(|ty|format!("{:?}",ty.structural_type()))});
    }
    Ok((summary, raw))
}
fn object_expected_matrix(spec: &ObjectSpec) -> Value {
    let mut expected = json!({"keys":if spec.added{vec!["added","run","value"]}else{vec!["run","value"]},"fields":{}});
    for (field, label) in [
        ("value", Some(spec.value)),
        ("run", Some(spec.run)),
        ("added", spec.added.then_some("boolean")),
    ] {
        let entries: Vec<_> = label.into_iter().collect();
        expected["fields"][field] =
            json!({"memberInfo":label,"withKey":entries,"withKeyAll":entries,"memberType":label});
    }
    expected
}
fn object_no_known_fields(matrix: &Value) -> bool {
    matrix["fields"].as_object().is_some_and(|fields| {
        fields.values().all(|field| {
            ["memberInfo", "memberType"].iter().all(|key| {
                field[*key].is_null() || field[*key] == "unknown" || field[*key] == "any"
            }) && ["withKey", "withKeyAll"].iter().all(|key| {
                field[*key]
                    .as_array()
                    .is_some_and(|types| types.iter().all(|ty| ty == "unknown" || ty == "any"))
            })
        })
    })
}
fn object_fact<'a>(graph: &'a Graph, site: &Site) -> Result<&'a UseFact> {
    let mut facts = graph.uses.iter().filter(|fact| fact.site == *site);
    let fact = facts.next().ok_or("object observation fact missing")?;
    if facts.next().is_some() {
        return Err("object observation execution context ambiguous".into());
    }
    Ok(fact)
}
fn object_frozen_view(serialized_graph: &Value, fact: &UseFact) -> Value {
    let Some(view_id) = serialized_graph["uses"][fact.id]["object_view"].as_u64() else {
        return json!({"state":"unavailable","reason":"graph_object_observation_missing","factId":fact.id});
    };
    let Some(view) = serialized_graph["object_views"]
        .as_array()
        .and_then(|views| views.iter().find(|view| view["id"] == view_id))
    else {
        return json!({"state":"unavailable","reason":"graph_object_view_missing","factId":fact.id,"objectViewId":view_id});
    };
    json!({"state":"recorded","factId":fact.id,"objectViewId":view_id,"view":view})
}
fn object_ref(value: &BindingDecision<BoundValue>) -> Option<&ObjectRef> {
    match value {
        BindingDecision::Bound(BoundValue {
            value: SelectedValue::Object(object),
            ..
        }) => Some(object),
        _ => None,
    }
}
fn native_object_red(out: &mut NativeOutput) -> Result<()> {
    let patch = option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256")
        .ok_or("missing compile-time context patch identity")?;
    if patch.len() != 64 || !patch.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid compile-time patch identity".into());
    }
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":patch,"scope":"object-red"}))?;
    for name in OBJECT_FILES {
        out.emit(json!({"event":"input_identity","name":name,"file":name,"bytes":source(name).len(),"sha256":hash(source(name).as_bytes())}))?;
    }
    out.emit(json!({"event":"capabilities","scope":"object-red","sourceExecution":false,"configExecution":false,"nativeTypingProfile":NATIVE_PROFILE,"fullSourceAcceptance":false,"limits":"16-file/64KiB/6000-AST/10000-step; 64 objects/512 views/4096 slots/128 fields/64 depth/4096 mutations","gaps":["native object identity/epoch type projection is not implemented","public structure coverage API is missing","native cycle/metatable/inheritance/dynamic-key resolution remains separate work"]}))?;
    let input = NativeInput::from_files(OBJECT_FILES, &out.control)?;
    let graph = input.graph_for_scope(
        "object-versions",
        "after_load",
        OBJECT_ORDER,
        patch,
        Some("object-version-structure-red-v1"),
    )?;
    let serialized = serde_json::to_value(&graph).map_err(|error| error.to_string())?;
    let provider = input.freeze(&graph)?;
    let session = probe_session(provider.clone(), input.operation_control())?;
    let file_id = input.files()[OBJECT_FILE];
    let mut check = || input.operation_control().check();
    let mut model = probe_query(&input, &session, file_id, &mut check);
    let mut foreign_check = || input.operation_control().check();
    let mut foreign = probe_query(
        &input,
        &session,
        input.files()["generic-defs.lua"],
        &mut foreign_check,
    );
    let need = graph
        .functions
        .iter()
        .find(|function| function.name == "NeedNumber" && function.declaration.file == OBJECT_FILE)
        .ok_or("NeedNumber declaration")?;
    let signature = model
        .type_of_signature_in_file(file_id, input.binding_site(&need.closure)?.syntax)?
        .ok_or("NeedNumber native signature")?;
    let expected_type = signature
        .function_parameters()
        .and_then(|params| params.into_iter().next().flatten())
        .ok_or("NeedNumber native parameter type")?;
    let (expected_members, expected_members_raw) = object_api_matrix(&mut model, &expected_type)?;
    out.emit(json!({"event":"object_expected_type","scope":"object-red","declaration":need.declaration,"closure":need.closure,"nativeLuaType":format!("{:?}",expected_type.structural_type()),"nativeSignature":format!("{:?}",signature.structural_type()),"memberAPIs":expected_members,"memberRaw":expected_members_raw}))?;
    let mut sites = Vec::new();
    let mut types = Vec::new();
    let mut decisions = Vec::new();
    for spec in OBJECT_SPECS {
        let site = marker_site(OBJECT_FILE, spec.marker, spec.object)?;
        let native = input.binding_site(&site)?;
        types.push(model.type_of_expr(native.syntax)?);
        decisions.push(provider.resolve(native));
        sites.push(site);
    }
    for (step, index) in [0usize, 1, 0, 2, 3, 2, 4, 5].into_iter().enumerate() {
        let spec = &OBJECT_SPECS[index];
        let site = &sites[index];
        let native = input.binding_site(site)?;
        let fact = object_fact(&graph, site)?;
        let ty = model.type_of_expr(native.syntax)?;
        let (sum, raw) = object_api_matrix(&mut model, &ty)?;
        let (foreign_sum, foreign_raw) = object_api_matrix(&mut foreign, &ty)?;
        let compatibility = model.type_check(&ty, &expected_type)?;
        let foreign_compatibility = foreign.type_check(&ty, &expected_type)?;
        let frozen_view = object_frozen_view(&serialized, fact);
        let mut direct = json!({});
        let mut direct_proofs = json!({});
        for (field, suffix) in [("value", "Value"), ("run", "Run"), ("added", "Added")] {
            let expression = format!("{}.{}", spec.object, field);
            let marker = format!("local {}{suffix} = {expression}", spec.reads);
            let index_site = marker_site(OBJECT_FILE, &marker, &expression)?;
            let index_native = input.binding_site(&index_site)?;
            let index_type = model.type_of_expr(index_native.syntax)?;
            let index_fact = object_fact(&graph, &index_site)?;
            direct[field] = json!(object_type_label(index_type.structural_type()));
            direct_proofs[field] = json!({"site":index_site,"nativeLuaType":format!("{:?}",index_type.structural_type()),"factId":index_fact.id,"proofId":index_fact.proof_id,"decision":index_fact.decision,"writeVersions":index_fact.writes});
        }
        let coverage = probe_coverage(&session, input.operation_control())?;
        let structure_coverage = coverage
            .iter()
            .find(|record| record.site == native && record.operation == "structure_check");
        out.emit(json!({"event":"object_observation","scope":"object-red","query":spec.label,"interleaveStep":step,"snapshotKey":graph.snapshot_key,"sourceSite":site,"factId":fact.id,"proofId":fact.proof_id,"binding":decision_json(&input,&decisions[index]),"frozenObjectView":frozen_view,"nativeLuaType":format!("{:?}",ty.structural_type()),"memberAPIs":sum,"memberRaw":raw,"foreignMemberAPIs":foreign_sum,"foreignMemberRaw":foreign_raw,"directIndex":direct,"directIndexProofs":direct_proofs,"publicStructureCheck":{"nativeBoolean":compatibility,"foreignNativeBoolean":foreign_compatibility,"coverage":structure_coverage.map(|record|record.state),"proofId":structure_coverage.map(|record|&record.proof)},"contextIdentity":model.binding_context_identity(),"foreignContextIdentity":foreign.binding_context_identity()}))?;
        let prefix = format!("object_red:{}:step:{step}", spec.label);
        out.check(
            &(prefix.clone() + ":frozen_object_view"),
            json!("recorded"),
            frozen_view["state"].clone(),
        )?;
        if spec.dynamic {
            out.check(
                &(prefix.clone() + ":members_not_known"),
                json!(true),
                json!(object_no_known_fields(&sum)),
            )?;
            out.check(
                &(prefix.clone() + ":foreign_members_not_known"),
                json!(true),
                json!(object_no_known_fields(&foreign_sum)),
            )?;
        } else {
            out.check(
                &(prefix.clone() + ":member_api_matrix"),
                object_expected_matrix(spec),
                sum.clone(),
            )?;
            out.check(
                &(prefix.clone() + ":foreign_member_api_matrix"),
                object_expected_matrix(spec),
                foreign_sum.clone(),
            )?;
        }
        out.check(&(prefix.clone()+":direct_index"),json!({"value":spec.value,"run":spec.run,"added":if spec.dynamic{"unknown"}else if spec.added{"boolean"}else{"nil"}}),direct)?;
        out.check(
            &(prefix.clone() + ":public_structure_boolean"),
            json!(spec.compatible),
            json!(compatibility),
        )?;
        out.check(
            &(prefix.clone() + ":foreign_structure_boolean"),
            json!(spec.compatible),
            json!(foreign_compatibility),
        )?;
        // Preserve the public completion-quality vocabulary. The executor is
        // the owned structure gate, with its actual per-session provenance.
        out.check(
            &(prefix.clone() + ":public_structure_coverage"),
            json!(if spec.dynamic {
                "unavailable"
            } else {
                "native_checked"
            }),
            json!(structure_coverage.map(|record| record.state)),
        )?;
        out.check(
            &(prefix + ":foreign_context"),
            json!(graph.snapshot_key),
            json!(foreign.binding_context_identity()),
        )?;
    }
    // These comparisons consume real provider epochs and native Eq keys. The
    // probe never synthesizes an expected epoch from source writes.
    for (label, left, right, same_object) in [
        ("source_changed", 0usize, 1usize, true),
        ("copied_changed", 2, 3, true),
        ("source_vs_copy", 1, 3, false),
        ("same_shape_twin", 1, 4, false),
        ("dynamic_changed", 1, 5, true),
    ] {
        let left_ref = object_ref(&decisions[left]);
        let right_ref = object_ref(&decisions[right]);
        out.emit(json!({"event":"object_identity_pair","scope":"object-red","query":label,"left":decision_json(&input,&decisions[left]),"right":decision_json(&input,&decisions[right]),"nativeTypesEqual":types[left]==types[right]}))?;
        out.check(
            &format!("object_red:{label}:object_identity"),
            json!(same_object),
            json!(
                left_ref
                    .zip(right_ref)
                    .is_some_and(|(a, b)| a.identity == b.identity)
            ),
        )?;
        if same_object {
            out.check(
                &format!("object_red:{label}:epoch_changed"),
                json!(true),
                json!(
                    left_ref
                        .zip(right_ref)
                        .is_some_and(|(a, b)| a.epoch != b.epoch)
                ),
            )?;
        }
        out.check(
            &format!("object_red:{label}:native_type_identity_distinct"),
            json!(true),
            json!(types[left] != types[right]),
        )?;
    }
    // Equal shapes must retain different value-reference evidence.
    let reference_sites = |decision: &BindingDecision<BoundValue>| -> Result<Vec<BindingSite>> {
        let BindingDecision::Bound(value) = decision else {
            return Err("object value identity missing".into());
        };
        match probe_references(&session, &value.identity, input.operation_control())? {
            BindingDecision::Bound(refs) => {
                Ok(refs.iter().map(|reference| reference.site).collect())
            }
            _ => Err("object reference overlay unavailable".into()),
        }
    };
    let source_refs = reference_sites(&decisions[1])?;
    let twin_refs = reference_sites(&decisions[4])?;
    out.emit(json!({"event":"object_reference_partition","scope":"object-red","source":source_refs.iter().map(|site|binding_site_json(&input,*site)).collect::<Vec<_>>(),"twin":twin_refs.iter().map(|site|binding_site_json(&input,*site)).collect::<Vec<_>>()}))?;
    out.check(
        "object_red:same_shape_distinct_reference_evidence",
        json!(true),
        json!(
            !source_refs.is_empty()
                && !twin_refs.is_empty()
                && source_refs.iter().all(|site| !twin_refs.contains(site))
        ),
    )?;
    for (label, marker, expression, spec_index) in [
        (
            "generic_old",
            "local oldGeneric = Identity(Source)",
            "Identity(Source)",
            0usize,
        ),
        (
            "generic_new",
            "local newGeneric = Identity(Source)",
            "Identity(Source)",
            1,
        ),
        (
            "generic_copy",
            "local copiedGeneric = Identity(Copied)",
            "Identity(Copied)",
            3,
        ),
        (
            "generic_dynamic",
            "local dynamicGeneric = Identity(Source)",
            "Identity(Source)",
            5,
        ),
    ] {
        let source_site = marker_site(OBJECT_FILE, marker, expression)?;
        let native = input.binding_site(&source_site)?;
        let call = graph
            .calls
            .iter()
            .find(|call| call.site == source_site)
            .ok_or("object generic call")?;
        let argument = &graph.uses[*call.arguments.first().ok_or("object generic argument")?];
        let argument_site = input.binding_site(&argument.site)?;
        let argument_type = model.type_of_expr(argument_site.syntax)?;
        let result_type = model.type_of_expr(native.syntax)?;
        let (matrix, raw) = object_api_matrix(&mut model, &result_type)?;
        let (foreign_matrix, foreign_raw) = object_api_matrix(&mut foreign, &result_type)?;
        let compatibility = model.type_check(&result_type, &expected_type)?;
        let spec = &OBJECT_SPECS[spec_index];
        out.emit(json!({"event":"object_generic_query","scope":"object-red","query":label,"snapshotKey":graph.snapshot_key,"sourceSite":source_site,"callFactId":call.id,"resultFactId":call.result_fact,"argumentFactId":argument.id,"argumentProofId":argument.proof_id,"argumentDecision":decision_json(&input,&provider.resolve(argument_site)),"argumentNativeType":format!("{:?}",argument_type.structural_type()),"resultNativeType":format!("{:?}",result_type.structural_type()),"nativeTypesEqual":argument_type==result_type,"memberAPIs":matrix,"memberRaw":raw,"foreignMemberAPIs":foreign_matrix,"foreignMemberRaw":foreign_raw,"publicStructureBoolean":compatibility}))?;
        out.check(
            &format!("object_red:{label}:version_preserved"),
            json!(true),
            json!(argument_type == result_type),
        )?;
        if spec.dynamic {
            out.check(
                &format!("object_red:{label}:members_not_known"),
                json!(true),
                json!(object_no_known_fields(&matrix)),
            )?;
            out.check(
                &format!("object_red:{label}:foreign_members_not_known"),
                json!(true),
                json!(object_no_known_fields(&foreign_matrix)),
            )?;
        } else {
            out.check(
                &format!("object_red:{label}:member_api_matrix"),
                object_expected_matrix(spec),
                matrix,
            )?;
            out.check(
                &format!("object_red:{label}:foreign_member_api_matrix"),
                object_expected_matrix(spec),
                foreign_matrix,
            )?;
        }
        out.check(
            &format!("object_red:{label}:public_structure_boolean"),
            json!(spec.compatible),
            json!(compatibility),
        )?;
    }
    let diagnostics = probe_diagnostics(&input, &session, file_id)?;
    let rows:Vec<_>=diagnostics.iter().map(|diagnostic|json!({"code":diagnostic.code.get_name(),"start":u32::from(diagnostic.range.start()),"end":u32::from(diagnostic.range.end())})).collect();
    let expected_diagnostics: Vec<_> = [
        (OBJECT_SPECS[1].marker, "Source"),
        (OBJECT_SPECS[3].marker, "Copied"),
    ]
    .iter()
    .map(|(marker, object)| {
        let site = marker_site(OBJECT_FILE, marker, object).expect("fixed diagnostic marker");
        json!({"code":"param-type-mismatch","start":site.start,"end":site.end})
    })
    .collect();
    out.emit(json!({"event":"native_diagnostics","scope":"object-red","scenario":graph.scenario,"snapshotKey":graph.snapshot_key,"coordinate":"original_utf8_bytes","diagnostics":diagnostics.iter().map(|diagnostic|json!({"code":diagnostic.code.get_name(),"sourceSite":{"file":OBJECT_FILE,"start":u32::from(diagnostic.range.start()),"end":u32::from(diagnostic.range.end())},"message":diagnostic.message})).collect::<Vec<_>>(),"coverage":coverage_json(&input,&session)?}))?;
    out.check(
        "object_red:exact_two_structural_argument_diagnostics",
        json!(expected_diagnostics),
        json!(rows),
    )?;
    let coverage = probe_coverage(&session, input.operation_control())?;
    for index in [0usize, 1, 2, 3, 5] {
        let spec = &OBJECT_SPECS[index];
        let expression = format!("NeedNumber({})", spec.object);
        let site = marker_site(OBJECT_FILE, spec.marker, &expression)?;
        let native = input.binding_site(&site)?;
        // Completion quality retains the original public state spelling.
        out.check(
            &format!("object_red:{}:native_argument_coverage", spec.label),
            json!(if spec.dynamic {
                "unavailable"
            } else {
                "native_checked"
            }),
            json!(
                coverage
                    .iter()
                    .find(|record| record.site == native && record.operation == "argument_check")
                    .map(|record| record.state)
            ),
        )?;
    }
    out.check(
        "object_red:explicit_five_inputs",
        json!(5),
        json!(input.files().len()),
    )?;
    out.emit(json!({"event":"scenario","name":graph.scenario,"scope":"object-red","snapshotKey":graph.snapshot_key,"graphSHA256":digest(&graph),"graph":graph,"fullSourceAcceptance":false}))?;
    Ok(())
}
