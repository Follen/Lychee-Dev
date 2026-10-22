use super::*;
use crate::xml_load::{Action, GameMode, HandlerTarget, LoadFamily, Program, XmlOrigin, XmlReference};
use std::{sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn program(control: &OperationControl) -> Program {
    Program::from_sources("Main.toc", [
        ("Main.toc", b"ui.xml\n".as_slice()),
        ("ui.xml", br#"<Ui><Script file="a.lua"/><Frame name="Template" virtual="true"/><Frame name="Intrinsic" intrinsic="true"/><Frame name="Parent" mixin="M"><Frames><Frame parentKey="Child"/><Frame parentKey="Other" parent="Parent"/></Frames><Scripts><OnLoad method="Run"/><OnShow function="Callback"/><OnHide/><OnEvent method="Run" function="Callback"/><OnUpdate>return self</OnUpdate></Scripts></Frame><Frame/></Ui>"#.as_slice()),
        ("a.lua", b"M={}\n".as_slice()),
    ], control).unwrap()
}
fn identity(program: Option<Arc<Program>>) -> SourceIdentityEvidence {
    SourceIdentityEvidence {
        common: Arc::new(evidence_fields(json!({"a":{"z":0,"a":1},"shared":"common","z":[]}))),
        execution: evidence_fields(json!({"executionPhase":"after_load","shared":"execution"})),
        environment: None,
        environment_value: std::sync::OnceLock::new(),
        xml: program,
        xml_value: std::sync::OnceLock::new(),
    }
}
fn eager_identity(identity: &SourceIdentityEvidence) -> Value {
    let mut fields = identity.common.as_ref().clone();
    fields.extend(identity.execution.clone());
    if let Some(environment) = &identity.environment {
        let host=TrustedHostEnvironment{data:environment.clone(),admission_control:control()};
        fields.insert("environment".into(),host.identity_value(&host.admission_control).unwrap());
    }
    if let Some(program) = &identity.xml { fields.insert("xmlProgram".into(), json!(program.as_ref())); }
    Value::Object(fields)
}
fn snapshot(identity: SourceIdentityEvidence) -> Arc<GraphSnapshotEvidence> {
    Arc::new(GraphSnapshotEvidence {
        admission: Arc::new(identity),
        common: Arc::new(evidence_fields(json!({"z":[],"effectiveEmmyrcJson":"{\"quoted\":\"你好\\n\"}"}))),
        execution: evidence_fields(json!({"admissionSHA256":"fixture","validationScope":"scope"})),
    })
}
fn error_json<T>(result: lycheedev_core::Result<T>) -> Value {
    match result { Ok(_) => panic!("expected failure"), Err(error) => json!(error) }
}

const HOST_SOURCE:&str="Mixin=function() end\nCreateFromMixins=function() end\nsetmetatable=function() end\n";
fn populated_host(c:&OperationControl)->TrustedHostEnvironment {
    let rules=json!({"calls":{"HostRules":{"name":"HostRules",
        "arguments":{"1":{"secret":"conditional","condition":"界\nquoted\"","metadata":{"z":null,"a":[true,1.25]}}},
        "returns":{"1":{"state":"possible"},"2":{"state":"never"}}}}});
    let rules_bytes=serde_json::to_vec(&rules).unwrap();
    let material=json!({"sourceExecution":false,"configExecution":false,
        "rules":{"sha256":hash(&rules_bytes)},
        "callables":[
            {"name":"HostUnknown","identity":"unknown","effect":"unknown","sourceExecution":false},
            {"name":"HostRules","identity":"rules","effect":"no_lua_heap_write","retainsArguments":false,"returnArity":2,"sourceExecution":false,
                "descriptor":{"kind":"function","name":"HostRules","raw":{"z":{"z":null,"a":[u64::MAX,i64::MIN,-1.25,0.0]},"a":"\"\\\n\t界"}}}],
        "methods":[{"name":"FrameOwner.Show","identity":"method","effect":"unknown","sourceExecution":false,"returnArity":0,
            "descriptor":{"kind":"function","name":"Show","owner":{"kind":"script-object","name":"FrameOwner","source":null},"raw":{}}}],
        "nominalObjects":{"Frame":{"owner":"FrameOwner"}},
        "predicates":[{"name":"issecretvalue","identity":"predicate","effect":"no_lua_heap_write","retainsArguments":false,"returnArity":1,"argument":1}],
        "initialObjects":[{"name":"HostObject","identity":"object","fields":[]}]});
    let api_bytes=serde_json::to_vec(&material).unwrap();
    let intrinsics=["Mixin","CreateFromMixins","setmetatable"].into_iter().map(|name|{
        let start=HOST_SOURCE.find(name).unwrap()as u32;
        IntrinsicDeclaration{file:"a.lua".into(),source_sha256:hash(HOST_SOURCE.as_bytes()),start,end:start+name.len()as u32,
            name:name.into(),rule:PROFILE.into(),host_material:"api".into()}
    }).collect();
    let reference=|material:&str,pointer:&str|HostMaterialRef{material:material.into(),pointer:pointer.into()};
    TrustedHostEnvironment::from_host_material("canonical-host-界".into(),true,vec![
        HostMaterialInput{name:"api".into(),expected_sha256:hash(&api_bytes),bytes:api_bytes},
        HostMaterialInput{name:"rules".into(),expected_sha256:hash(&rules_bytes),bytes:rules_bytes},
    ],intrinsics,c).unwrap().with_bindings(vec![
        HostBindingInput::Callable{declaration:reference("api","/callables/0"),rules:None},
        HostBindingInput::Callable{declaration:reference("api","/callables/1"),rules:Some(reference("rules","/calls/HostRules"))},
        HostBindingInput::Method{declaration:reference("api","/methods/0")},
        HostBindingInput::Predicate{declaration:reference("api","/predicates/0")},
        HostBindingInput::EmptyObject{declaration:reference("api","/initialObjects/0")},
    ],c).unwrap()
}

#[test]
fn canonical_host_environment_matches_eager_bytes_hash_and_all_binding_shapes() {
    let c=control();
    let populated=populated_host(&c);
    let eager=populated.identity_value(&c).unwrap();
    assert_eq!(eager["bindings"].as_array().unwrap().len(),5);
    assert_eq!(eager["intrinsics"].as_array().unwrap().len(),3);
    assert_eq!(eager["bindings"][0]["return_arity"],Value::Null);
    assert_eq!(eager["bindings"][0]["rules"],Value::Null);
    assert!(eager["bindings"][0].get("descriptor").is_none());
    assert!(eager["bindings"][0].get("receiver_type").is_none());
    assert_eq!(eager["bindings"][1]["descriptor"]["raw"]["z"]["a"][0],u64::MAX);
    assert_eq!(eager["bindings"][2]["receiver_type"],"Frame");
    assert_eq!(eager["bindings"][3]["kind"],"predicate");
    assert_eq!(eager["bindings"][4]["kind"],"empty_object");
    let material_only=TrustedHostEnvironment::from_host_material("material-only".into(),false,
        vec![HostMaterialInput{name:"raw".into(),bytes:vec![0,255],expected_sha256:hash(&[0,255])}],vec![],&c).unwrap();
    for host in [populated,material_only,
        TrustedHostEnvironment::from_host_material("empty".into(),false,vec![],vec![],&c).unwrap(),
        TrustedHostEnvironment::from_host_material("closed".into(),true,vec![],vec![],&c).unwrap()] {
        let old=host.identity_value(&c).unwrap();
        let expected=serialize_json(&old,&c,32<<20).unwrap();
        let canonical=CanonicalHostEnvironment(&host.data);
        assert_eq!(serialize_json(&canonical,&c,32<<20).unwrap(),expected);
        assert_eq!(serialize_json_hash(&canonical,&c,32<<20).unwrap(),(hash(&expected),expected.len()));
        if host.bindings().is_empty(){assert!(old.get("bindings").is_none());}
    }
}

#[test]
fn host_identity_is_lazy_shared_and_uses_current_request_control_after_renewal() {
    let c=control();
    let host=populated_host(&c);
    let host_data=host.data.clone();
    let old_environment=host.identity_value(&c).unwrap();
    let input=SourceInputs::admit(SourceInputSpec{
        files:vec![SourceFileInput{path:"a.lua".into(),original_utf8:HOST_SOURCE.into(),analysis_utf8:HOST_SOURCE.into(),
            content_sha256:hash(HOST_SOURCE.as_bytes()),map:SourceMap::Identity}],
        workspace_root:std::env::current_dir().unwrap().join("host-identity-fixture"),source_identity:json!({"pin":"host-identity"}),
        phase:"after_load".into(),ordered_occurrences:vec!["a.lua".into()],config:crate::engine_config::research(),host,
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        limits:SourceLimits::research()},&c).unwrap();
    assert!(Arc::ptr_eq(input.identity.environment.as_ref().unwrap(),&host_data));
    assert!(input.identity.common.get("environment").is_none());
    assert!(input.identity.environment_value.get().is_none());
    let expected=eager_identity(input.identity());
    let expected_bytes=serde_json::to_vec(&expected).unwrap();
    assert_eq!(serde_json::to_vec(input.identity()).unwrap(),expected_bytes);
    assert_eq!(input.identity_sha256(),hash(&expected_bytes));
    assert!(input.identity.environment_value.get().is_none());
    assert_eq!(input.identity()["environment"],old_environment);
    assert!(std::ptr::eq(&input.identity()["environment"],&input.identity()["environment"]));
    let cloned=input.identity().clone();
    assert!(Arc::ptr_eq(cloned.environment.as_ref().unwrap(),&host_data));
    assert!(cloned.environment_value.get().is_none());
    assert_eq!(serde_json::to_vec(&cloned).unwrap(),expected_bytes);
    let refreshed=input.execution("after_load",&["a.lua","a.lua"],&c).unwrap();
    assert!(Arc::ptr_eq(refreshed.identity.environment.as_ref().unwrap(),&host_data));
    assert!(refreshed.identity.environment_value.get().is_none());
    assert_eq!(refreshed.identity_sha256(),hash(&serde_json::to_vec(&eager_identity(refreshed.identity())).unwrap()));

    let mut prepared=prepare(input.clone(),&c).unwrap();
    c.tracked_cancel_handle().unwrap().cancel();
    let current=control();
    prepared.begin_research_request(&current).unwrap();
    let renewed=prepared.inputs.execution("after_load",&["a.lua"],&current).unwrap();
    assert!(Arc::ptr_eq(renewed.identity.environment.as_ref().unwrap(),&host_data));
    assert!(renewed.identity.environment_value.get().is_none());
    assert_eq!(serialize_json_hash(renewed.identity(),&current,32<<20).unwrap(),(hash(&expected_bytes),expected_bytes.len()));
    assert_eq!(renewed.identity_sha256(),input.identity_sha256());
    assert_eq!(renewed.identity()["environment"],old_environment);
    assert_eq!(error_json(serialize_json_hash(renewed.identity(),&c,32<<20))["code"],"command.cancelled");
    current.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(error_json(serialize_json_hash(renewed.identity(),&current,32<<20))["code"],"command.cancelled");
}

#[test]
fn canonical_xml_program_matches_old_value_bytes_for_every_produced_action() {
    let c = control();
    let mut program = program(&c);
    let kinds = program.actions().iter().map(|action| match action {
        Action::Lua { .. } => "lua", Action::Object { .. } => "object", Action::Mixin { .. } => "mixin",
        Action::ParentField { .. } => "parent_field", Action::Handler { .. } => "handler", Action::Definition { .. } => "definition",
    }).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds, ["definition", "handler", "lua", "mixin", "object", "parent_field"].into_iter().collect());
    let targets = program.actions().iter().filter_map(|action| match action {
        Action::Handler { target, .. } => Some(match target {
            HandlerTarget::Inline { .. } => "inline", HandlerTarget::Method { .. } => "method",
            HandlerTarget::Function { .. } => "function", HandlerTarget::Empty => "empty", HandlerTarget::Unavailable { .. } => "unavailable",
        }), _ => None,
    }).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(targets, ["empty", "function", "inline", "method", "unavailable"].into_iter().collect());
    program.gaps.push(json!({"z":{"嵌套":[{"z":u64::MAX,"a":i64::MIN},-1.25,0.0,null]},"a":"\"\\\n\t界"}));
    for (family, game, mode) in [
        (None, None, GameMode::Standard),
        (Some(LoadFamily::Mainline), Some("".into()), GameMode::Plunderstorm),
        (Some(LoadFamily::Classic), Some("Mists".into()), GameMode::Wowhack),
    ] {
        program.context.family = family; program.context.game = game; program.context.mode = mode;
        program.context.basis = vec![]; program.context.uncertainties = vec!["\n未知".into()];
        let old = json!(&program);
        let expected = serialize_json(&old, &c, 32 << 20).unwrap();
        let actual = serialize_json(&CanonicalXmlProgram(&program), &c, 32 << 20).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(serialize_json_hash(&CanonicalXmlProgram(&program), &c, 32 << 20).unwrap(), (controlled_hash(&expected, &c).unwrap(), expected.len()));
    }
}

#[test]
fn canonical_xml_actions_preserve_null_empty_and_nested_origin_fields() {
    let c = control();
    let origin = XmlOrigin { path:"界/\".xml".into(), sha256:String::new(), bytes:crate::xml_inline::source_map::Span::new(0, usize::MAX) };
    let reference = XmlReference { name:String::new(), origin:origin.clone() };
    let mut actions = vec![
        Action::Lua { file:String::new() },
        Action::Object { instance:String::new(), origin:origin.clone(), tag:String::new(), name:None, parent:None },
        Action::Object { instance:"child".into(), origin:origin.clone(), tag:"Frame".into(), name:Some(String::new()), parent:Some("parent".into()) },
        Action::Mixin { instance:String::new(), reference:reference.clone() },
        Action::ParentField { parent:None, parent_reference:None, child:String::new(), reference:reference.clone() },
        Action::ParentField { parent:Some(String::new()), parent_reference:Some(reference.clone()), child:"child".into(), reference:reference.clone() },
        Action::Definition { name:String::new(), intrinsic:false, origin:origin.clone() },
        Action::Definition { name:"Intrinsic".into(), intrinsic:true, origin:origin.clone() },
    ];
    for target in [HandlerTarget::Inline { file:String::new() }, HandlerTarget::Method { reference:reference.clone() },
        HandlerTarget::Function { reference:reference.clone() }, HandlerTarget::Empty, HandlerTarget::Unavailable { reason:String::new() }] {
        actions.push(Action::Handler { instance:String::new(), handler:String::new(), target, origin:origin.clone() });
    }
    let expected = serde_json::to_vec(&json!(&actions)).unwrap();
    assert_eq!(serde_json::to_vec(&CanonicalXmlActions(&actions)).unwrap(), expected);
    assert_eq!(serialize_json_hash(&CanonicalXmlActions(&actions), &c, 2 << 20).unwrap(), (hash(&expected), expected.len()));
    let empty = Program::from_sources("Empty.toc", [("Empty.toc", b"".as_slice())], &c).unwrap();
    assert_eq!(serde_json::to_vec(&CanonicalXmlProgram(&empty)).unwrap(), serde_json::to_vec(&json!(&empty)).unwrap());
}

#[test]
fn xml_identity_indexes_materialize_only_requested_value_and_refresh_shares_program() {
    let c = control();
    let program = Arc::new(program(&c));
    let evidence = identity(Some(Arc::clone(&program)));
    let old = eager_identity(&evidence);
    assert!(Arc::ptr_eq(evidence.xml.as_ref().unwrap(), &program));
    assert!(evidence.xml_value.get().is_none());
    assert_eq!(evidence["shared"], "execution"); assert_eq!(evidence["absent"], Value::Null);
    assert_eq!(serialize_json(&evidence, &c, 32 << 20).unwrap(), serde_json::to_vec(&old).unwrap());
    serialize_json_hash(&evidence, &c, 32 << 20).unwrap();
    assert!(evidence.xml_value.get().is_none());
    assert_eq!(evidence["xmlProgram"], old["xmlProgram"]);
    assert!(std::ptr::eq(&evidence["xmlProgram"], &evidence["xmlProgram"]));
    let cloned = evidence.clone();
    assert!(cloned.xml_value.get().is_none());
    assert!(Arc::ptr_eq(cloned.xml.as_ref().unwrap(), &program));
    assert_eq!(serde_json::to_vec(&cloned).unwrap(), serde_json::to_vec(&old).unwrap());
    assert_eq!(identity(None)["xmlProgram"], Value::Null);

    let source = "M={}\n";
    let host = TrustedHostEnvironment::from_host_material("identity-fixture".into(), false, vec![], vec![], &c).unwrap();
    let base = SourceInputs::admit(SourceInputSpec {
        files:vec![SourceFileInput { path:"a.lua".into(), original_utf8:source.into(), content_sha256:hash(source.as_bytes()), analysis_utf8:source.into(), map:SourceMap::Identity }],
        workspace_root:std::env::current_dir().unwrap().join("identity-fixture"), source_identity:json!({"pin":"unchanged"}), phase:"after_load".into(),
        ordered_occurrences:vec!["a.lua".into()], config:crate::engine_config::research(), host,
        luac:LuacIdentity { version:"5.1.5".into(), platform:"windows-amd64".into(), executable_sha256:"a".repeat(64), executable_bytes:1 }, limits:SourceLimits::research(),
    }, &c).unwrap();
    let mut with_xml = base.as_ref().clone(); with_xml.xml = Some(Arc::clone(&program)); with_xml.refresh_identity(&c).unwrap();
    let expected = eager_identity(with_xml.identity());
    assert_eq!(with_xml.identity_sha256(), hash(&serde_json::to_vec(&expected).unwrap()));
    assert!(with_xml.identity.xml_value.get().is_none());
    let next = with_xml.execution("after_load", &["a.lua", "a.lua"], &c).unwrap();
    assert!(Arc::ptr_eq(next.identity.xml.as_ref().unwrap(), &program));
    assert!(next.identity.xml_value.get().is_none());
    assert_eq!(next.identity_sha256(), hash(&serde_json::to_vec(&eager_identity(next.identity())).unwrap()));
    assert_ne!(next.identity_sha256(), with_xml.identity_sha256());
}

#[test]
fn snapshot_json_is_lazy_shared_exact_and_clone_does_not_copy_materialization() {
    let c = control();
    let evidence = snapshot(identity(Some(Arc::new(program(&c)))));
    let old = json!({"admission":eager_identity(&evidence.admission),"z":[],"effectiveEmmyrcJson":"{\"quoted\":\"你好\\n\"}","admissionSHA256":"fixture","validationScope":"scope"});
    let expected = serde_json::to_string(&old).unwrap();
    let (lazy, digest) = SnapshotInputsJson::new(Arc::clone(&evidence), &c, 32 << 20).unwrap();
    assert_eq!(digest, hash(expected.as_bytes()));
    assert_eq!(lazy.len(), expected.len()); assert!(!lazy.is_empty()); assert!(!lazy.is_materialized());
    assert!(Arc::ptr_eq(&lazy.evidence, &evidence));
    assert_eq!(serde_json::to_vec(evidence.as_ref()).unwrap(), expected.as_bytes());
    assert!(!lazy.is_materialized()); assert!(evidence.admission.xml_value.get().is_none());
    assert_eq!(lazy.as_bytes(), expected.as_bytes());
    assert!(lazy.is_materialized());
    assert_eq!(lazy.as_str(), expected); assert_eq!(&*lazy, expected.as_str());
    assert_eq!(serde_json::to_vec(&lazy).unwrap(), serde_json::to_vec(&expected).unwrap());
    assert!(evidence.admission.xml_value.get().is_none());
    let cloned = lazy.clone();
    assert!(!cloned.is_materialized()); assert!(Arc::ptr_eq(&cloned.evidence, &evidence));
    assert_eq!(serde_json::to_vec(&cloned).unwrap(), serde_json::to_vec(&expected).unwrap());
    assert!(cloned.is_materialized());
}

#[test]
fn streamed_json_hash_matches_original_all_byte_limits_and_small_writes() {
    let c = control();
    let value = json!({"z":[1,2,3,null],"a":"界\n\"\\\t","nested":{"z":false,"a":u64::MAX}});
    let bytes = serde_json::to_vec(&value).unwrap();
    for limit in [0, 1, bytes.len()-1, bytes.len(), bytes.len()+1, 2<<20, 32<<20, (32<<20)+1] {
        let old = serialize_json(&value, &c, limit).and_then(|bytes| Ok((controlled_hash(&bytes, &c)?, bytes.len())));
        let new = serialize_json_hash(&value, &c, limit);
        match old {
            Ok(expected) => assert_eq!(new.unwrap(), expected),
            Err(error) => assert_eq!(error_json(new), json!(error)),
        }
    }
    let values = (0..10_000).collect::<Vec<_>>();
    let bytes = serialize_json(&values, &c, 2<<20).unwrap();
    assert_eq!(serialize_json_hash(&values, &c, 2<<20).unwrap(), (hash(&bytes), bytes.len()));
    assert!(std::mem::size_of::<ControlledHashWriter<'_>>() < 512, "hash-only writer has constant-size state");
}

#[test]
fn streamed_json_hash_preserves_two_and_thirty_two_mib_ceilings() {
    let c = control();
    for limit in [2<<20, 32<<20] {
        let exact = "a".repeat(limit-2);
        let bytes = serialize_json(&exact, &c, limit).unwrap();
        assert_eq!(bytes.len(), limit);
        assert_eq!(serialize_json_hash(&exact, &c, limit).unwrap(), (hash(&bytes), limit));
        let too_large = format!("{exact}a");
        assert_eq!(error_json(serialize_json_hash(&too_large, &c, limit)), error_json(serialize_json(&too_large, &c, limit)));
    }
}

struct FailingSerialization;
impl Serialize for FailingSerialization {
    fn serialize<S:serde::Serializer>(&self, serializer:S) -> std::result::Result<S::Ok,S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(&"prefix")?;
        Err(serde::ser::Error::custom("original serializer failure"))
    }
}
#[test]
fn streamed_json_hash_preserves_serializer_failure_and_budget_first_error() {
    let c = control();
    for limit in [1, 2<<20] {
        assert_eq!(error_json(serialize_json_hash(&FailingSerialization, &c, limit)), error_json(serialize_json(&FailingSerialization, &c, limit)));
    }
    assert_eq!(error_json(serialize_json_hash(&FailingSerialization, &c, 2<<20))["message"], "original serializer failure");
}

struct CancelDuringSerialization<'a>(&'a OperationControl, bool);
impl Serialize for CancelDuringSerialization<'_> {
    fn serialize<S:serde::Serializer>(&self, serializer:S) -> std::result::Result<S::Ok,S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(&"prefix")?;
        self.0.tracked_cancel_handle().unwrap().cancel();
        if self.1 { return Err(serde::ser::Error::custom("failure after cancellation")); }
        seq.serialize_element(&"next")?;
        seq.end()
    }
}
#[test]
fn streamed_json_hash_preserves_cancellation_deadline_and_post_error_precedence() {
    for deadline in [false, true] {
        let stopped = stopped_control(deadline, Instant::now()+Duration::from_secs(30));
        for limit in [0, 1, 32<<20, (32<<20)+1] {
            assert_eq!(error_json(serialize_json_hash(&json!({}), &stopped, limit)), error_json(serialize_json(&json!({}), &stopped, limit)));
        }
    }
    for error_after_cancel in [false, true] {
        let old_control = control(); let new_control = control();
        let old = error_json(serialize_json(&CancelDuringSerialization(&old_control, error_after_cancel), &old_control, 2<<20));
        let new = error_json(serialize_json_hash(&CancelDuringSerialization(&new_control, error_after_cancel), &new_control, 2<<20));
        assert_eq!(new, old); assert_eq!(new["code"], "command.cancelled");
    }
}

#[test]
fn snapshot_hash_validates_before_lazy_construction_and_preserves_original_failures() {
    let c = control();
    let evidence = snapshot(identity(None));
    let expected = serialize_json(evidence.as_ref(), &c, 2<<20).unwrap();
    for limit in [0, 1, expected.len()-1, (32<<20)+1] {
        assert_eq!(error_json(SnapshotInputsJson::new(Arc::clone(&evidence), &c, limit)), error_json(serialize_json(evidence.as_ref(), &c, limit)));
    }
    let (lazy, _) = SnapshotInputsJson::new(Arc::clone(&evidence), &c, expected.len()).unwrap();
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(error_json(SnapshotInputsJson::new(Arc::clone(&evidence), &c, expected.len())), error_json(serialize_json(evidence.as_ref(), &c, expected.len())));
    // As before, the already admitted immutable public text remains readable
    // after the request ends; laziness cannot turn that access into a new task.
    assert_eq!(lazy.as_bytes(), expected);
}
