// Real admitted parser/semantic projections; none of these Lua strings execute.
#[test]
fn incoming_callers_distinguish_actual_owners_recursion_and_noncall_rows() {
    let source="function GetActionButtonForID(id) return id end\n\
function ActionButtonDown(id)\n local button = GetActionButtonForID(id)\nend\n\
function ActionButtonUp(id)\n local button = GetActionButtonForID(id)\nend\n\
function KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame(actionButton)\n return GetActionButtonForID(actionButton)\nend\n\
function Recursive() return Recursive() end\n";
    let session=planner_fixture(BTreeMap::from([("logic.lua".into(),source.into())]),BTreeMap::new(),None);
    let symbol=planned_query("GetActionButtonForID","logic.lua",1,1,"function","incoming").symbol;
    let actual=session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    let calls=actual["relations"].as_array().unwrap().iter().filter(|row|row["kind"]=="call").collect::<Vec<_>>();
    assert_eq!(calls.len(),3);
    for(row,name,line)in [(calls[0],"ActionButtonDown",3),(calls[1],"ActionButtonUp",6),(calls[2],"KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame",9)] {
        assert_eq!(row["source"],name);assert_eq!(row["line"],line);
        assert_eq!(row["referenceExpression"],"GetActionButtonForID");assert_eq!(row["caller"]["kind"],"named");assert_eq!(row["caller"]["name"],name);
        let bytes=&row["caller"]["position"]["bytes"];
        assert_eq!(&source[bytes["start"].as_u64().unwrap()as usize..bytes["end"].as_u64().unwrap()as usize],name);
        assert_ne!(row["source"],row["target"],"these original calls are not recursion");
    }
    let before=session.prepared.research_steps();
    let old=session.prepared.full_scan_research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    let old_cost=session.prepared.research_steps()-before;
    let before=session.prepared.research_steps();
    let new=session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    assert_eq!(session.prepared.research_steps()-before,old_cost);
    assert_eq!(original_call_labels(new),old,"proofs, observations, noncall rows, order and complete analysis unchanged");
    let symbol=planned_query("Recursive","logic.lua",11,11,"function","incoming").symbol;
    let actual=session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    let recursive=actual["relations"].as_array().unwrap().iter().find(|row|row["kind"]=="call").unwrap();
    assert_eq!(recursive["source"],"Recursive");assert_eq!(recursive["target"],"Recursive");assert_eq!(recursive["caller"]["kind"],"named");
}

#[test]
fn incoming_callers_stop_at_anonymous_and_recognize_exact_assignment_slots() {
    let source="\u{feff}-- 😀 界\r\nfunction Target() end\r\nfunction Outer()\r\n Target()\r\n Register(function() Target() end)\r\n local function Inner() Target() end\r\n local Local = (function() Target() end)\r\n Assigned, Other = function() Target() end, function() Target() end\r\n Obj.Run = function() Target() end\r\n Obj[key] = function() Target() end\r\n local t = {Run=function() Target() end}\r\n return function() Target() end\r\nend\r\nTarget()\r\n";
    let session=planner_fixture(BTreeMap::from([("logic.lua".into(),source.into())]),BTreeMap::new(),None);
    let symbol=planned_query("Target","logic.lua",2,2,"function","incoming").symbol;
    let result=session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    let calls=result["relations"].as_array().unwrap().iter().filter(|row|row["kind"]=="call").collect::<Vec<_>>();
    let expected=[("Outer","named"),("<anonymous>","anonymous"),("Inner","named"),("Local","named"),("Assigned","named"),("Other","named"),("Obj.Run","named"),("<anonymous>","anonymous"),("<anonymous>","anonymous"),("<anonymous>","anonymous"),("<file>","file")];
    assert_eq!(calls.len(),expected.len());
    for(row,(name,kind))in calls.into_iter().zip(expected) {
        assert_eq!(row["source"],name);assert_eq!(row["caller"]["kind"],kind);assert_eq!(row["referenceExpression"],"Target");
        assert_eq!(row["caller"]["position"]["path"],"logic.lua");
        if kind!="named" {assert!(row["caller"].get("name").is_none(),"placeholder is not a symbol name");}
    }
    let outgoing=session.prepared.research_relations(&planned_query("Outer","logic.lua",3,13,"function","outgoing").symbol,"outgoing",None,&session.xml).unwrap();
    assert!(outgoing["relations"].as_array().unwrap().iter().all(|row|row.get("caller").is_none()&&row.get("referenceExpression").is_none()),"outgoing contract is outside this change");
}

#[test]
fn incoming_alias_calls_keep_reference_spelling_and_nearest_definition() {
    let source="function Target(x) return x end\nAlias=Target\nfunction ActualCaller() return Alias(1) end\n";
    let session=planner_fixture(BTreeMap::from([("logic.lua".into(),source.into())]),BTreeMap::new(),None);
    let control=session.prepared.operation_control();
    let graph=session.prepared.build_execution("caller-alias","after_load",&["logic.lua"],EvaluatorProfile::Native,None,control).unwrap();
    let symbol=planned_query("Target","logic.lua",1,1,"function","incoming").symbol;
    let result=session.prepared.research_relations(&symbol,"incoming",Some(&graph),&session.xml).unwrap();
    let row=result["relations"].as_array().unwrap().iter().find(|row|row["kind"]=="call"&&row["referenceExpression"]=="Alias").expect("selected alias is retained");
    assert_eq!(row["source"],"ActualCaller");assert_eq!(row["target"],"Target");assert_eq!(row["caller"]["kind"],"named");
    assert!(!row["observations"].as_array().unwrap().is_empty());
    let expected=session.prepared.full_scan_research_relations(&symbol,"incoming",Some(&graph),&session.xml).unwrap();
    assert_eq!(original_call_labels(result),expected);
}

#[test]
fn incoming_xml_callers_distinguish_wrappers_nested_functions_and_original_maps() {
    let control=OperationControl::default();
    let documents=BTreeMap::from([
        ("Frames.xml".into(),"\u{feff}<Ui>\r\n<Frame name=\"Frame\"><Scripts><OnLoad>local s=&quot;😀&quot;&#10;Tar&#103;et(self); local function Inner() Target() end; Register(function() Target() end)</OnLoad></Scripts></Frame>\r\n<Script>Target()</Script></Ui>".into()),
        ("Bindings.xml".into(),"<Bindings><Binding name=\"CLICK_界\" header=\"H\">Target(keystate)</Binding></Bindings>".into()),
    ]);
    let session=ResearchSession::prepare(SnapshotRequest{files:BTreeMap::from([("logic.lua".into(),"function Target() end\n".into())]),load_documents:documents.clone(),environment_documents:BTreeMap::new(),source:json!({"repository":"caller-test","product":"retail"}),
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},api:vec![],environment_manifest:None,load_plans:vec![],input_gaps:vec![]},&control).unwrap();
    let maps=session.xml.mapping_manifests();
    let symbol=planned_query("Target","logic.lua",1,1,"function","incoming").symbol;
    let actual=session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    let calls=actual["relations"].as_array().unwrap().iter().filter(|row|row["kind"]=="call").collect::<Vec<_>>();
    assert_eq!(calls.len(),5);
    for(kind,source)in [("xml_handler","<xml-handler:OnLoad>"),("xml_binding","<xml-binding:Binding>"),("named","Inner"),("anonymous","<anonymous>"),("file","<file>")] {
        let row=calls.iter().find(|row|row["caller"]["kind"]==kind).expect(kind);assert_eq!(row["source"],source);
        assert_eq!(row["referenceExpression"],"Target");assert!(row["position"]["sourceMap"].is_object());
        assert!(documents.contains_key(row["caller"]["position"]["path"].as_str().unwrap()));
    }
    let handler=calls.iter().find(|row|row["caller"]["kind"]=="xml_handler").unwrap();
    assert_eq!(handler["position"]["sourceMap"]["accuracy"],"covering");
    assert!(handler["position"]["sourceMap"]["segments"].as_array().unwrap().iter().any(|segment|segment["origin"]=="Entity"));
    assert_eq!(maps,session.xml.mapping_manifests(),"full mapping inventory remains unchanged");
    let old=session.prepared.full_scan_research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    assert_eq!(original_call_labels(actual),old,"including all original XML site/position/proof fields");
    control.cancel_handle().store(true,std::sync::atomic::Ordering::Release);
    assert_eq!(session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap_err().code,"command.cancelled");
}

#[test]
fn incoming_callers_match_authenticated_complete_retail_documents() {
    let action=include_str!("../../../../tests/rust-source-callers/ActionButton.lua");
    let bindings=include_str!("../../../../tests/rust-source-callers/Blizzard_Keybindings.lua");
    let manifest:Value=serde_json::from_str(include_str!("../../../../tests/rust-source-callers/fixtures.json")).unwrap();
    let action_path="Interface/AddOns/Blizzard_ActionBar/Shared/ActionButton.lua";
    let bindings_path="Interface/AddOns/Blizzard_Settings_Shared/Blizzard_Keybindings.lua";
    let files=BTreeMap::from([(action_path.into(),action.into()),(bindings_path.into(),bindings.into())]);
    for file in manifest["files"].as_array().unwrap() {
        let text:&String=&files[file["originalPath"].as_str().unwrap()];
        assert_eq!(storage::sha256(text.as_bytes()),file["sha256"]);assert_eq!(text.len()as u64,file["bytes"].as_u64().unwrap());
    }
    let session=planner_fixture(files,BTreeMap::new(),None);
    let symbol=planned_query("GetActionButtonForID",action_path,97,107,"function","incoming").symbol;
    let result=session.prepared.research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    let calls=result["relations"].as_array().unwrap().iter().filter(|row|row["kind"]=="call").collect::<Vec<_>>();
    assert_eq!(calls.len(),3);
    for(path,line,start,end,owner,owner_line)in [(action_path,141,3940,3960,"ActionButtonDown",136),(action_path,156,4223,4243,"ActionButtonUp",151),(bindings_path,318,8621,8641,"KeyBindingFrameBindingTemplateMixin:DetermineHighlightFrame",312)] {
        let row=calls.iter().find(|row|row["path"]==path&&row["line"]==line).unwrap();
        assert_eq!(row["site"],json!({"file":path,"start":start,"end":end}));assert_eq!(row["source"],owner);
        assert_eq!(row["referenceExpression"],"GetActionButtonForID");assert_eq!(row["caller"]["kind"],"named");
        assert_eq!(row["caller"]["position"]["coveringRange"]["start"]["line"],owner_line-1);
    }
    let old=session.prepared.full_scan_research_relations(&symbol,"incoming",None,&session.xml).unwrap();
    assert_eq!(original_call_labels(result),old);
}

#[test]
fn incoming_malformed_function_documents_keep_real_session_admission_rejection() {
    // The production boundary rejects parser errors before caller indexing.
    // Do not bypass that boundary to manufacture successful malformed rows.
    for malformed in ["function Obj.() Target() end","function Obj:() Target() end","function Obj[key]() Target() end","function Obj().Run() Target() end","function Obj()() Target() end","function Obj a) Target() end","function Obj(a Target() end","local function Obj.() Target() end","local function Obj[key]() Target() end","local function Obj()() Target() end","function BodyBroken() local x = ; Target() end"] {
        let snapshot=SnapshotRequest{files:BTreeMap::from([("logic.lua".into(),format!("function Target() end\n{malformed}\n"))]),load_documents:BTreeMap::new(),environment_documents:BTreeMap::new(),source:json!({"repository":"caller-parser-recovery-test","product":"retail"}),
            luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},api:vec![],environment_manifest:None,load_plans:vec![],input_gaps:vec![]};
        let result=ResearchSession::prepare(snapshot,&OperationControl::default());
        let error=match result{Ok(_)=>panic!("malformed document passed real session admission: {malformed}"),Err(error)=>error};
        assert!(error.to_string().contains("native source parse error"),"unexpected failure for {malformed}: {error}");
    }
}
