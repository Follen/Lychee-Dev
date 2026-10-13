use lycheedev_core::{Invocation, control::OperationControl, selection, storage};
use lycheedev_source::{SourceTools, closure::Closure, luac, native::*, xml_inline, xml_load};
use serde_json::{Value,json};
use std::{collections::BTreeMap, fs, path::{Path,PathBuf}, sync::Arc};

const METHODS:&str="---@param flag boolean\nfunction Check(flag) end\nM={flag=true}\nfunction M:Run() Check(self.flag) end\nfunction Callback(self) Check(self.flag) end\n";

// Each case is a separate cold native analysis. The owned runner gives each
// original operation its own unchanged deadline instead of applying one
// operation's watchdog to the entire sixteen-operation acceptance matrix.
macro_rules! xml_case {
    ($test:ident, $case:literal) => {
        #[test]
        #[ignore="explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration"]
        fn $test() { run_xml_case($case); }
    };
}
xml_case!(xml_ordered_copy_versions,"ordered_copy_versions");
xml_case!(xml_repeated_include_occurrences,"repeated_include_occurrences");
xml_case!(xml_inline_script_load_order,"inline_script_load_order");
xml_case!(xml_invalidated_mixin_source,"invalidated_mixin_source");
xml_case!(xml_copied_method_survives_source_overwrite,"copied_method_survives_source_overwrite");
xml_case!(xml_self_survives_global_rebind,"self_survives_global_rebind");
xml_case!(xml_instance_field_mutation,"instance_field_mutation");
xml_case!(xml_virtual_distinct_instances,"virtual_distinct_instances");
xml_case!(xml_intrinsic_definition_instances,"intrinsic_definition_instances");
xml_case!(xml_anonymous_instances,"anonymous_instances");
xml_case!(xml_function_attribute_entity,"function_attribute_entity");
xml_case!(xml_inline_self,"inline_self");
xml_case!(xml_parent_key_identity,"parent_key_identity");
xml_case!(xml_empty_registration,"empty_registration");
xml_case!(xml_method_overwritten_noncallable,"method_overwritten_noncallable");
xml_case!(xml_native_mixin_not_lua_alias,"native_mixin_not_lua_alias");

fn run_xml_case(selected:&str){
    let home=PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let pin_id=std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let pin=selection::load_pinned(&home,&pin_id).unwrap();
    let manifest:Value=serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let compiler=luac::Runtime::verified(&PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed Lua51 compiler")),serde_json::from_value(manifest["runtime"].clone()).unwrap()).unwrap();
    let stage=std::env::var_os("LYCHEEDEV_XML_OUTPUT").map(PathBuf::from).unwrap_or_else(||std::env::temp_dir().join(format!("lycheedev-native-xml-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    fs::create_dir(&stage).expect("fresh XML output directory");
    let cases=[
        ("ordered_copy_versions","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Script file=\"rewrite.lua\"/><Frame name=\"B\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame></Ui>","M.flag={}\n",1,2,2),
        ("repeated_include_occurrences","<Ui><Script file=\"defs.lua\"/><Include file=\"part.xml\"/><Script file=\"rewrite.lua\"/><Include file=\"part.xml\"/></Ui>","M.flag={}\n",1,2,2),
        ("inline_script_load_order","<Ui><Script file=\"defs.lua\"/><Script>M.flag={}</Script><Frame mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame></Ui>","",1,1,1),
        ("invalidated_mixin_source","<Ui><Script file=\"defs.lua\"/><Script file=\"rewrite.lua\"/><Frame mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame></Ui>","M[UnknownKey]={}\n",0,1,0),
        ("copied_method_survives_source_overwrite","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Script file=\"rewrite.lua\"/></Ui>","M.Run=1\n",0,1,1),
        ("self_survives_global_rebind","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Script file=\"rewrite.lua\"/></Ui>","A={flag={}}\n",0,1,1),
        ("instance_field_mutation","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Script file=\"rewrite.lua\"/></Ui>","A.flag={}\n",1,1,1),
        ("virtual_distinct_instances","<Ui><Script file=\"defs.lua\"/><Frame name=\"Template\" virtual=\"true\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Frame name=\"A\" inherits=\"Template\"/><Frame name=\"B\" inherits=\"Template\"/><Script file=\"rewrite.lua\"/></Ui>","A.flag={}\n",1,2,2),
        ("intrinsic_definition_instances","<Ui><Script file=\"defs.lua\"/><Frame name=\"Custom\" intrinsic=\"true\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Custom name=\"A\"/><Custom name=\"B\"/></Ui>","",0,2,2),
        ("anonymous_instances","<Ui><Script file=\"defs.lua\"/><Frame mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Frame mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame></Ui>","",0,2,2),
        ("function_attribute_entity","\u{feff}<Ui>\r\n<Script file=\"defs.lua\"/><Frame name=\"界\" mixin=\"M\"><Scripts><OnLoad function=\"Call&#98;ack\"/></Scripts></Frame></Ui>","",0,1,1),
        ("inline_self","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad>Check(self.flag)</OnLoad></Scripts></Frame></Ui>","",0,1,1),
        ("parent_key_identity","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\"><Frames><Frame parentKey=\"Child\" mixin=\"M\"/></Frames><Scripts><OnLoad>Check(self.Child.flag)</OnLoad></Scripts></Frame></Ui>","",0,2,1),
        ("empty_registration","<Ui><Script file=\"defs.lua\"/><Frame><Scripts><OnLoad/></Scripts></Frame></Ui>","",0,1,0),
        ("method_overwritten_noncallable","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Script file=\"rewrite.lua\"/></Ui>","A.Run=1\n",0,1,0),
        ("native_mixin_not_lua_alias","<Ui><Script file=\"defs.lua\"/><Frame name=\"A\" nativeMixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame></Ui>","",0,1,0),
    ];
    assert!(cases.iter().any(|case|case.0==selected),"unknown XML matrix case");
    let mut reports=Vec::new();
    for (name,xml,rewrite,mismatches,objects,entries) in cases.into_iter().filter(|case|case.0==selected){
        let addon=stage.join(name);fs::create_dir(&addon).unwrap();
        fs::write(addon.join("Main.toc"),format!("## Interface: {}\nui.xml\n",selection::source_interface(pin.source.as_ref().unwrap()).unwrap())).unwrap();
        fs::write(addon.join("defs.lua"),METHODS).unwrap();fs::write(addon.join("rewrite.lua"),rewrite).unwrap();fs::write(addon.join("ui.xml"),xml).unwrap();
        fs::write(addon.join("part.xml"),"<Ui><Frame name=\"Repeated\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame></Ui>").unwrap();
        let inv=Invocation{command:"source validate".into(),positional:None,home:home.clone(),cwd:addon.clone(),flags:BTreeMap::from([("--path".into(),addon.to_string_lossy().into()),("--toc".into(),"Main.toc".into()),("--snapshot".into(),pin_id.clone()),("--semantic".into(),"true".into())]),control:Default::default()};
        let (report,error)=match lycheedev_source::dispatch_with_tools(&inv,SourceTools{luac:Some(&compiler),luals:None}){
            Ok(outcome)=>(outcome.data,None),
            Err(error)=>(error.details["result"].clone(),Some((error.code,error.exit_code))),
        };
        fs::write(addon.join("report.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        if mismatches>0{assert_eq!(error,Some(("codebase.addon_static_check_failed".into(),4)),"{name}: negative semantic result must use the public validation error");}
        else{assert!(error.is_none(),"{name}: expected successful public validation, got {error:?}");}
        assert_eq!(report["valid"],mismatches==0,"{name}: public valid result");
        assert_eq!(report["syntaxCompilation"]["passed"],true,"{name}: syntax remains valid");
        let semantic=&report["semantic"];
        assert_eq!(semantic["passed"],mismatches==0,"{name}: semantic pass state");
        let actual=semantic["diagnostics"].as_array().unwrap().iter().filter(|row|row["code"]=="param-type-mismatch").count();
        assert_eq!(actual,mismatches,"{name}: {}",summary(semantic));
        assert_eq!(semantic["xmlFacts"]["objects"].as_array().unwrap().len(),objects,"{name}: {}",summary(semantic));
        assert_eq!(semantic["xmlEntries"].as_array().unwrap().len(),entries,"{name}: {}",summary(semantic));
        for entry in semantic["xmlEntries"].as_array().unwrap(){let origin=&entry["selfOrigin"]["xml"];let path=origin["path"].as_str().unwrap();assert_eq!(origin["sha256"],storage::sha256(&fs::read(addon.join(path)).unwrap()));}
        if name=="virtual_distinct_instances"||name=="intrinsic_definition_instances"{
            let objects=semantic["xmlFacts"]["objects"].as_array().unwrap();assert_ne!(objects[0]["object"],objects[1]["object"]);
            assert!(objects.iter().all(|object|object["name"]!="Template"&&object["name"]!="Custom"));
        }
        if name=="function_attribute_entity"{
            let origin=&semantic["xmlFacts"]["handlers"][0]["target"]["reference"]["origin"];
            let start=origin["bytes"]["start"].as_u64().unwrap()as usize;let end=origin["bytes"]["end"].as_u64().unwrap()as usize;
            assert_eq!(&xml[start..end],"Call&#98;ack");assert_eq!(semantic["xmlFacts"]["handlers"][0]["target"]["reference"]["name"],"Callback");
        }
        if name=="method_overwritten_noncallable"||name=="native_mixin_not_lua_alias"{
            assert!(semantic["xmlFacts"]["handlers"][0]["closure"].is_null());
            assert!(semantic["gaps"].as_array().unwrap().iter().any(|gap|gap["code"]=="xml_handler_callable_unavailable"));
        }
        if name=="native_mixin_not_lua_alias"{assert!(semantic["xmlFacts"]["copies"].as_array().unwrap().is_empty());}
        if name=="ordered_copy_versions"||name=="invalidated_mixin_source"{assert_public_queries(&home,&addon,&compiler,name=="invalidated_mixin_source");}
        reports.push(json!({"case":name,"report":addon.join("report.json"),"expectedValid":mismatches==0,"expectedMismatches":mismatches,"objects":objects,"entries":entries}));
    }
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"source":pin.source,"cases":reports})).unwrap()).unwrap();
    println!("{}",stage.join("report.json").display());
}
fn summary(value:&Value)->Value{json!({"diagnostics":value["diagnostics"],"gaps":value["gaps"],"xmlFacts":value["xmlFacts"],"xmlEntries":value["xmlEntries"]})}

fn assert_public_queries(home:&Path,addon:&Path,compiler:&luac::Runtime,invalidated:bool){
    let control=OperationControl::default();let closure=Closure::inspect_with_control(home,addon,"Main.toc",&control).unwrap();
    let (files,_)=xml_inline::inputs(&closure.documents).unwrap();
    let program=Arc::new(xml_load::Program::from_closure(&closure,&control).unwrap());
    let runtime=compiler.manifest();let host=TrustedHostEnvironment::from_host_material("xml-query-fixture".into(),false,vec![],vec![],&control).unwrap();
    let mut config=emmylua_code_analysis::Emmyrc::default();config.runtime.version=emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    let inputs=SourceInputs::admit(SourceInputSpec{files:files.into_iter().map(|(path,bytes)|{let text=String::from_utf8(bytes).unwrap();SourceFileInput{path,content_sha256:storage::sha256(text.as_bytes()),analysis_utf8:text.clone(),original_utf8:text,map:SourceMap::Identity}}).collect(),workspace_root:PathBuf::from("C:/lycheedev/xml-query-fixture"),source_identity:json!({"fixture":"ordered-xml-original-bytes"}),phase:"after_load".into(),ordered_occurrences:vec![],config,host,luac:LuacIdentity{version:runtime.version.clone(),platform:runtime.platform.clone(),executable_sha256:runtime.sha256.clone(),executable_bytes:runtime.bytes},limits:SourceLimits::research()},&control).unwrap();
    let prepared=prepare(inputs,&control).unwrap();let graph=prepared.build_xml_plan("xml-query",program,None,&control).unwrap();
    let facade=prepared.source_queries(&control).unwrap();let query=facade.with_execution(&graph,QuerySelector::AllOccurrences,&control).unwrap();
    let refs=query.references(QueryTarget::All,ReferenceFilter::default(),PageRequest{limit:200,bytes:2<<20,cursor:None}).unwrap();
    assert!(refs.rows.iter().any(|row|row.site.file=="ui.xml"&&matches!(row.selected,QueryFact::Known{..})),"XML references must use the shared query surface");
    let xml=graph.xml.as_ref().unwrap();let method=xml.references.iter().find(|reference|reference.role=="handler_method").unwrap();
    let site=Site{file:method.origin.path.clone(),start:method.origin.bytes.start as u32,end:method.origin.bytes.end as u32};
    if invalidated{
        assert!(matches!(query.definitions(&site).unwrap().selected,QueryFact::Unavailable{..}));
        assert!(xml.gaps.iter().any(|gap|gap["code"]=="xml_mixin_source_field_unavailable"&&gap["field"]=="Run"));
        assert!(xml.copies.is_empty(),"invalidated source fields must not claim stale copied writes");
        let mixin=xml.references.iter().find(|reference|reference.role=="mixin").unwrap();
        let site=Site{file:mixin.origin.path.clone(),start:mixin.origin.bytes.start as u32,end:mixin.origin.bytes.end as u32};
        let context=query.context(QueryTarget::Site(site),ContextOptions{depth:4,snippet_bytes:4096},PageRequest{limit:200,bytes:2<<20,cursor:None}).unwrap();
        assert!(context.rows.iter().any(|row|row.kind=="selected_use"&&row.detail["selected"]["state"]=="unavailable"));
        assert!(!context.rows.iter().any(|row|row.kind=="xml_mixin_copy_edge"));
        return;
    }
    let definition=query.definitions(&site).unwrap();assert!(matches!(definition.selected,QueryFact::Known{value:ValueDefinition{function:Some(_),..}}));
    let copy=&xml.copies[0];let site=Site{file:copy.origin.path.clone(),start:copy.origin.bytes.start as u32,end:copy.origin.bytes.end as u32};
    let context=query.context(QueryTarget::Site(site),ContextOptions{depth:4,snippet_bytes:4096},PageRequest{limit:200,bytes:2<<20,cursor:None}).unwrap();
    assert!(context.rows.iter().any(|row|row.kind=="xml_mixin_copy_edge"&&matches!(&row.origin,ValueOrigin::Xml{xml}if xml.path=="ui.xml")&&row.snippet.as_deref()==Some("M")));
    let flag_copies=xml.copies.iter().filter(|copy|copy.field=="flag").collect::<Vec<_>>();assert_eq!(flag_copies.len(),2);assert_ne!(flag_copies[0].source_field_write,flag_copies[1].source_field_write);assert_ne!(flag_copies[0].destination_object,flag_copies[1].destination_object);
    let selector=refs.rows.iter().find(|row|row.site.file==copy.origin.path&&row.site.start==copy.origin.bytes.start as u32).unwrap().occurrence.clone();
    assert!(matches!(selector,QuerySelector::XmlOccurrence{..}));
    let other=refs.rows.iter().find(|row|matches!(row.occurrence,QuerySelector::XmlOccurrence{..})&&row.occurrence!=selector).unwrap().occurrence.clone();
    let scoped=facade.with_execution(&graph,selector,&control).unwrap();
    let page=scoped.context(QueryTarget::All,ContextOptions{depth:4,snippet_bytes:4096},PageRequest{limit:1,bytes:2<<20,cursor:None}).unwrap();
    let cursor=page.next_cursor.expect("XML occurrence context has multiple provenance records");
    let continuation=scoped.context(QueryTarget::All,ContextOptions{depth:4,snippet_bytes:4096},PageRequest{limit:1,bytes:2<<20,cursor:Some(cursor.clone())}).unwrap();
    assert!(!continuation.rows.is_empty());assert_ne!(page.rows[0].id,continuation.rows[0].id);
    let other=facade.with_execution(&graph,other,&control).unwrap();
    assert!(other.context(QueryTarget::All,ContextOptions{depth:4,snippet_bytes:4096},PageRequest{limit:1,bytes:2<<20,cursor:Some(cursor.clone())}).is_err(),"XML action cursor must not cross to another XML action");
    let load=facade.with_execution(&graph,QuerySelector::LoadOccurrence{occurrence:0},&control).unwrap();
    assert!(load.context(QueryTarget::All,ContextOptions{depth:4,snippet_bytes:4096},PageRequest{limit:1,bytes:2<<20,cursor:Some(cursor)}).is_err(),"XML cursor must not cross to a Lua load occurrence");
}
