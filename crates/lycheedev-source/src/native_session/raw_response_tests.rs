use super::*;
use std::io::Cursor;

fn encoded(value:&impl Serialize)->Vec<u8>{serde_json::to_vec(value).unwrap()}
#[test]
fn typed_mapping_reply_keeps_bounded_frame_raw_forwarding_and_cached_second_query(){
    let control=OperationControl::default();
    let documents=BTreeMap::from([
        ("z.xml".to_owned(),"<Ui><Script>local z=1</Script></Ui>".to_owned()),
        ("a.xml".to_owned(),"\u{feff}<Ui>\r\n<Frame><Scripts><OnLoad>local text=&quot;界&quot;\r\n</OnLoad></Scripts></Frame></Ui>".to_owned()),
    ]);
    let mut scripts=BTreeMap::new();
    for (path,text) in &documents {
        for script in crate::xml_inline::extract(path,text.as_bytes()).unwrap(){scripts.insert(script.key.clone(),script);}
    }
    let old_maps=Value::Array(scripts.values().map(crate::xml_inline::Script::mapping_manifest).collect());
    let snapshot=crate::native_research::SnapshotRequest{encoding_warnings:vec![],
        files:BTreeMap::from([("target.lua".into(),"function Target() end\n".into())]),load_documents:documents,environment_documents:BTreeMap::new(),
        source:json!({"repository":"typed-reply-fixture","product":"retail"}),
        luac:crate::native::LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        api:vec![],environment_manifest:None,load_plans:vec![],input_gaps:vec![]};
    let query=QueryRequest{symbol:crate::index::Symbol{id:String::new(),kind:"declaration".into(),name:"Target".into(),target:String::new(),category:"function".into(),confidence:String::new(),path:"target.lua".into(),line:1,end_line:1,signature:String::new(),scope:String::new(),ordinal:0},direction:"incoming".into()};
    let key=snapshot.cache_key(&control).unwrap();let mut cached=None;
    for (hit,work,reuse) in [
        (false,Some(WorkerRequest{snapshot,query:query.clone()}),None),
        (true,None,Some(CachedQuery{key,query})),
    ] {
        let reply=execute_cached_query(&mut cached,work,reuse,&control,41,|_,_|{}).unwrap();
        let write=ResponseWrite{value:Some(&reply),error:None,terminal:None};
        let bytes=serialize::serialize(&write,RESPONSE_BYTES).unwrap();
        let mut short=Vec::new();
        assert_eq!(write_frame(&mut short,&write,bytes.len()-1).unwrap_err().code,"source.native_frame_limit");
        assert!(short.is_empty(),"no partial frame may be published after bounded serialization fails");
        let mut wire=Vec::new();write_frame(&mut wire,&write,bytes.len()).unwrap();
        assert_eq!(read_raw_frame(&mut Cursor::new(wire),RESPONSE_BYTES).unwrap(),bytes);
        let value=reply.into_value();
        assert_eq!(value["sourceMaps"],old_maps);
        assert_eq!(value["environmentManifest"],Value::Null);
        assert_eq!(value["analysis"]["cacheHit"],hit);
        assert_eq!(value["nativeSession"]["cacheHit"],hit);
        assert_eq!(value["nativeSession"]["snapshotTransfer"],if hit{"identity_only"}else{"full"});
        let expected=encoded(&Response{value:Some(value),error:None,terminal:None});
        assert_eq!(bytes,expected,"typed writer retains the complete canonical Response<Value> bytes");
        let raw=receive(bytes).unwrap();assert!(!raw.failed());
        let mut forwarded=Vec::new();OwnerReply::Forward(raw).write(&mut forwarded).unwrap();
        let decoded:Response=read_frame(&mut Cursor::new(forwarded),RESPONSE_BYTES).unwrap();
        assert_eq!(encoded(&decoded),expected);
    }
}
fn receive(raw:Vec<u8>)->Result<RawResponse>{
    let mut framed=Vec::new();write_raw_frame(&mut framed,&raw)?;
    RawResponse::decode(read_raw_frame(&mut Cursor::new(framed),RESPONSE_BYTES)?)
}
#[test]
fn owner_raw_success_retains_one_allocation_and_exact_public_consumer_value(){
    let bytes=br#" {"value":{"z":[null,true,18446744073709551616,1e999],"a":"\u754c\ud83d\ude00","duplicate":1,"duplicate":2},"error":null,"extension":{"ignored":true}} "#.to_vec();
    let old:Response=serde_json::from_slice(&bytes).unwrap();
    let pointer=bytes.as_ptr();let capacity=bytes.capacity();
    let response=RawResponse::decode(bytes).unwrap();
    assert_eq!(response.bytes.as_ptr(),pointer);assert_eq!(response.bytes.capacity(),capacity);
    assert!(!response.failed());assert!(response.error.is_none());
    let mut frame=Vec::new();OwnerReply::Forward(response).write(&mut frame).unwrap();
    let raw=read_raw_frame(&mut Cursor::new(frame),RESPONSE_BYTES).unwrap();
    let actual:Response=serde_json::from_slice(&raw).unwrap();
    assert_eq!(encoded(&actual),encoded(&old));
    assert!(raw.starts_with(b" {"));assert!(raw.ends_with(b"} "));
}
#[test]
fn default_io_stack_relays_large_max_depth_and_drops_abandoned_raw_reply(){
    std::thread::Builder::new().name("raw-response-default-stack".into()).spawn(||{
        // Response consumes one of serde's 128 levels. The deepest admitted
        // nested Value contains 126 containers; width never changes that cap.
        let mut bytes=b"{\"value\":".to_vec();
        bytes.extend(std::iter::repeat_n(b'[',126));bytes.push(b'"');
        bytes.extend(std::iter::repeat_n(b'x',8<<20));bytes.push(b'"');
        bytes.extend(std::iter::repeat_n(b']',126));bytes.extend_from_slice(b",\"error\":null}");
        let expected=crate::native::hash(&bytes);
        let response=receive(bytes).unwrap();
        let mut frame=Vec::new();OwnerReply::Forward(response).write(&mut frame).unwrap();
        let raw=read_raw_frame(&mut Cursor::new(frame),RESPONSE_BYTES).unwrap();
        assert_eq!(crate::native::hash(&raw),expected);
        // Models retire() dropping a completed pipe exchange. It owns no nested
        // Value, and its destructor cannot recurse with the payload depth.
        drop(Some(Ok::<_,ToolkitError>(RawResponse::decode(raw).unwrap())));
    }).unwrap().join().unwrap();
}
#[test]
fn raw_response_keeps_full_frame_and_typed_envelope_rejection(){
    for raw in [
        br#"{"value":[],"error":null} false"#.as_slice(),
        br#"{"value":[1,],"error":null}"#,
        br#"{"value":null,"value":1,"error":null}"#,
        br#"{"value":null,"error":{"code":"x","message":"m","exit_code":5}}"#,
        br#"{"value":null,"error":{"code":1,"message":"m","exit_code":5,"details":null}}"#,
        br#"{"value":null,"error":null,"terminal":{"wrong":true}}"#,
        br#"{"value":"\ud800","error":null}"#,
    ] {
        assert!(serde_json::from_slice::<Response>(raw).is_err());
        assert_eq!(RawResponse::decode(raw.to_vec()).err().unwrap().code,"input.invalid");
    }
    let oversized=(RESPONSE_BYTES as u32+1).to_be_bytes();
    assert_eq!(read_raw_frame(&mut Cursor::new(oversized),RESPONSE_BYTES).unwrap_err().code,"source.native_frame_limit");
    for bytes in [vec![],vec![0,0,0],vec![0,0,0,3,b'{']] {
        assert_eq!(read_raw_frame(&mut Cursor::new(bytes),RESPONSE_BYTES).unwrap_err().code,"external.io");
    }
}
#[test]
fn raw_child_errors_keep_primary_and_exact_retirement_merge_semantics(){
    for details in [json!(null),json!([1,2]),json!({"z":null,"analysisRetirementError":"stale","a":"界"}),
        json!({"$serde_json::private::Number":"18446744073709551616"}),
        json!({"$serde_json::private::RawValue":"[true,{\"nested\":1}]"})] {
        for failed in [false,true] {
            let original=json!({"value":{"ignoredOnError":true},"error":{"code":"command.cancelled","message":"original","exit_code":7,"details":details}});
            let mut old:Response=serde_json::from_slice(&encoded(&original)).unwrap();
            let mut expected_error=old.error.take().unwrap();
            let retirement=failed.then(||fault("unconfirmed").with_details(json!({"rootReaped":true,"jobEmpty":false})));
            if let Some(cleanup)=&retirement {
                let prior=std::mem::take(&mut expected_error.details);
                expected_error.details=if prior.is_object(){prior}else{json!({"originalDetails":prior})};
                expected_error.details["analysisRetirementError"]=json!(cleanup);
            }
            let expected=Response{value:None,error:Some(expected_error),terminal:None};
            let response=receive(encoded(&original)).unwrap();assert!(response.failed());
            let mut frame=Vec::new();OwnerReply::ChildError{response,retirement,terminal:None}.write(&mut frame).unwrap();
            let actual:Response=read_frame(&mut Cursor::new(frame),RESPONSE_BYTES).unwrap();
            assert_eq!(encoded(&actual),encoded(&expected));
        }
    }
}
#[test]
fn raw_forward_write_failure_preserves_io_error_and_bounded_drop(){
    struct Fail;
    impl Write for Fail {
        fn write(&mut self,_:&[u8])->std::io::Result<usize>{Err(std::io::Error::other("original disconnected peer"))}
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    let response=receive(encoded(&json!({"value":{"rows":[1,2,3]},"error":null}))).unwrap();
    let failure=OwnerReply::Forward(response).write(&mut Fail).unwrap_err();
    assert_eq!(failure.code,"external.io");assert!(failure.message.contains("original disconnected peer"));
}

fn old_owner_and_cli(bytes:&[u8])->Result<Response>{
    let owner:Response=serde_json::from_slice(bytes)?;
    let mut frame=Vec::new();write_frame(&mut frame,&owner,RESPONSE_BYTES)?;
    read_frame(&mut Cursor::new(frame),RESPONSE_BYTES)
}
#[test]
fn canonical_writer_root_nested_markers_and_final_cli_preserve_full_values(){
    let number=json!({"$serde_json::private::Number":"18446744073709551616"});
    let raw=json!({"$serde_json::private::RawValue":"[null,{\"a\":true,\"z\":1e999}]"});
    for value in [number.clone(),raw.clone(),json!({"analysis":{"scopes":[{"coverage":{"complete":false,"reason":null},"number":number},{"raw":raw}]},"rows":[null,true,false],"unicode":"界😀"})] {
        // Exact same canonical Value writer as the authenticated analysis child.
        let bytes=encoded(&Response{value:Some(value),error:None,terminal:None});
        let expected=old_owner_and_cli(&bytes).unwrap();
        let response=receive(bytes).unwrap();let mut frame=Vec::new();
        OwnerReply::Forward(response).write(&mut frame).unwrap();
        let actual:Response=read_frame(&mut Cursor::new(frame),RESPONSE_BYTES).unwrap();
        assert_eq!(encoded(&actual),encoded(&expected));
    }
    for value in [json!({"$serde_json::private::Number":"1","z":0}),json!({"nested":{"$serde_json::private::RawValue":"[1,]"}})] {
        let bytes=encoded(&Response{value:Some(value),error:None,terminal:None});
        assert!(old_owner_and_cli(&bytes).is_err());assert!(receive(bytes).is_err());
    }
}
#[test]
fn final_cli_consumption_and_output_of_large_production_shaped_reply_on_default_stack(){
    std::thread::spawn(||{
        let value=json!({"analysis":{"scopes":[{"coverage":{"complete":true,"unknown":null},"digest":"one"},{"coverage":{"complete":false,"unknown":["reason"]},"digest":"two"}]},"relations":[{"direction":"incoming","source":"x.lua","target":"f"}],"text":"x".repeat(8<<20)});
        let bytes=encoded(&Response{value:Some(value),error:None,terminal:None});
        let expected=encoded(&old_owner_and_cli(&bytes).unwrap());
        let mut frame=Vec::new();OwnerReply::Forward(receive(bytes).unwrap()).write(&mut frame).unwrap();
        // This is the actual remaining CLI read_frame<Response> consumer, not a
        // raw-only test. Its ordinary public serialization and Drop both run.
        let actual:Response=read_frame(&mut Cursor::new(frame),RESPONSE_BYTES).unwrap();
        assert_eq!(encoded(&actual),expected);drop(actual);
    }).join().unwrap();
}

#[test]
fn actual_api_event_slot_preserves_old_two_consumer_private_raw_outcome(){
    use crate::{environment,native_research::SnapshotRequest};
    let path="Interface/AddOns/Blizzard_APIDocumentationGenerated/ExampleDocumentation.lua";
    for embedded in [r#"{"a":[null,true],"z":"界"}"#,r#"{"z":0,"$serde_json::private::Number":"1"}"#] {
        let text=format!(r#"local API = {{ Name = "Example", Type = "System", Events = {{ {{ Name = "Target", Type = "Event", Payload = {{ {{ ["$serde_json::private::RawValue"] = [=[{embedded}]=] }} }} }} }} }}
APIDocumentation:AddDocumentationTable(API)
"#);
        let control=OperationControl::default();
        let api=environment::build(environment::Identity{repository:"wow-ui-source".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:Default::default(),generator_version:environment::GENERATOR.into()},
            &BTreeMap::from([(path.into(),text.as_bytes().to_vec())]),environment::Limits::default(),&control).unwrap();
        assert_eq!(api.facts.iter().find(|record|record.name=="Target").unwrap().payload[0].raw["$serde_json::private::RawValue"],embedded);
        let snapshot=SnapshotRequest{encoding_warnings:vec![],files:BTreeMap::from([("target.lua".into(),"function Target() end\n".into())]),load_documents:BTreeMap::new(),environment_documents:BTreeMap::from([(path.into(),text)]),
            source:json!({"repository":"raw-event-test","product":"retail"}),
            luac:crate::native::LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
            api:api.facts,environment_manifest:Some(api.manifest),load_plans:vec![],input_gaps:vec![]};
        // The actual typed request preserves Slot.raw's BTreeMap, then verified
        // API rebuilding and the real research projection produce this field.
        let snapshot:SnapshotRequest=serde_json::from_slice(&encoded(&snapshot)).unwrap();
        let mut session=ResearchSession::prepare(snapshot,&control).unwrap();
        let value=session.query(QueryRequest{symbol:crate::index::Symbol{id:String::new(),kind:"declaration".into(),name:"Target".into(),target:String::new(),category:"function".into(),confidence:String::new(),path:"target.lua".into(),line:1,end_line:1,signature:String::new(),scope:String::new(),ordinal:0},direction:"incoming".into()},&control).unwrap();
        assert_eq!(value["apiDeclarations"][0]["payload"][0]["raw"]["$serde_json::private::RawValue"],embedded);
        let bytes=encoded(&Response{value:Some(value),error:None,terminal:None});
        let expected=old_owner_and_cli(&bytes);
        let mut frame=Vec::new();OwnerReply::Forward(receive(bytes).unwrap()).write(&mut frame).unwrap();
        let actual:Result<Response>=read_frame(&mut Cursor::new(frame),RESPONSE_BYTES);
        match (expected,actual) {
            (Ok(expected),Ok(actual))=>assert_eq!(encoded(&actual),encoded(&expected)),
            (Err(expected),Err(actual))=>{assert_eq!(actual.code,expected.code);assert!(actual.message.contains("trailing comma"));},
            _=>panic!("private API slot changed final consumer outcome"),
        }
    }
}

#[test]
fn private_raw_error_details_keep_old_cleanup_and_second_decode_failures(){
    for failed in [false,true] {
        let original=Response{value:Some(json!({"$serde_json::private::RawValue":"[1,2]"})),error:Some(ToolkitError::new("command.cancelled","original",7).with_details(json!({"$serde_json::private::RawValue":"{\"z\":0,\"$serde_json::private::Number\":\"1\"}"}))),terminal:None};
        let bytes=encoded(&original);
        let mut old:Response=serde_json::from_slice(&bytes).unwrap();
        old.value=None;
        let retirement=failed.then(||fault("unconfirmed").with_details(json!({"jobEmpty":false})));
        if let Some(cleanup)=&retirement{old.error.as_mut().unwrap().details["analysisRetirementError"]=json!(cleanup);}
        let expected=serde_json::from_slice::<Response>(&encoded(&old)).err().unwrap();
        let response=receive(bytes).unwrap();let mut frame=Vec::new();
        OwnerReply::ChildError{response,retirement,terminal:None}.write(&mut frame).unwrap();
        let actual=read_frame::<Response>(&mut Cursor::new(frame),RESPONSE_BYTES).err().unwrap();
        assert_eq!(actual.code,"input.invalid");
        assert!(expected.to_string().contains("trailing comma"));
        assert!(actual.message.contains("trailing comma"));
    }
}

#[test]
fn noncanonical_private_key_order_is_outside_the_authenticated_child_writer(){
    // The private child writes Response<Value>, whose map keys are canonical.
    // Arbitrary noncanonical peer JSON is not a promised second-pass emulator.
    // RawValue strings ARE reachable inside that writer and are normalized by
    // the separate API-event regression above, rather than exempted here.
    let bytes=br#"{"value":{"z":0,"$serde_json::private::Number":"1"},"error":null}"#;
    let owner:Response=serde_json::from_slice(bytes).unwrap();
    assert!(old_owner_and_cli(bytes).is_err());
    let mut frame=Vec::new();OwnerReply::Forward(receive(bytes.to_vec()).unwrap()).write(&mut frame).unwrap();
    assert!(read_frame::<Response>(&mut Cursor::new(frame),RESPONSE_BYTES).is_ok());
    // A real child encoding of precisely that Value is rejected on both paths.
    let canonical=encoded(&owner);
    assert_ne!(canonical.as_slice(),bytes.as_slice());
    assert!(old_owner_and_cli(&canonical).is_err());assert!(receive(canonical).is_err());
}

#[test]
fn replacement_frame_caps_capacity_before_incremental_growth(){
    let chunk=vec![b'x';7<<20];
    let mut output=Vec::new();
    for _ in 0..4 {
        append_bounded(&mut output,&chunk).unwrap();
        assert!(output.capacity()<=RESPONSE_BYTES);
    }
    append_bounded(&mut output,&chunk[..4<<20]).unwrap();
    assert_eq!(output.len(),RESPONSE_BYTES);assert!(output.capacity()<=RESPONSE_BYTES);
    let capacity=output.capacity();
    assert_eq!(append_bounded(&mut output,b"x").unwrap_err().code,"source.native_frame_limit");
    assert_eq!(output.len(),RESPONSE_BYTES);assert_eq!(output.capacity(),capacity);
    assert!(output.iter().all(|byte|*byte==b'x'));
}
