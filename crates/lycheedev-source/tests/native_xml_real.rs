use lycheedev_source::request::{AddonValidationRequest, SelectedSource, SourceContext, SourceRequest};
use lycheedev_core::{control::OperationControl, selection, storage};
use lycheedev_source::{SourceTools, closure::Closure, luac, native::*, xml_inline, xml_load};
use serde_json::{Value,json};
use std::{fs, path::{Path,PathBuf}, sync::Arc};

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
    let compiler=luac::Runtime::verified_with_ordered_control(&PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed Lua51 compiler")),serde_json::from_value(manifest["runtime"].clone()).unwrap(), &Default::default()).unwrap();
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
        let ctx = SourceContext {
            home: home.clone(),
            cwd: addon.clone(),
            control: Default::default(),
        };
        let request = SourceRequest::Validate(AddonValidationRequest {
            selection: SelectedSource::new(pin.clone()).unwrap(),
            root: addon.clone(),
            toc: "Main.toc".into(),
            semantic: true,
            environment: None,
            locator: format!("{}/Main.toc", addon.display()),
        });
        let (report,error)=match lycheedev_source::dispatch_with_tools(&ctx, &request,SourceTools{luac:Some(&compiler)}){
            Ok(outcome)=>(outcome.data,None),
            Err(error)=>(error.details["result"].clone(),Some((error.code,error.exit_code))),
        };
        fs::write(addon.join("report.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        if mismatches>0{assert_eq!(error,Some(("codebase.addon_static_check_failed".into(),4)),"{name}: negative semantic result must use the public validation error");}
        else{assert!(error.is_none(),"{name}: expected successful public validation, got {error:?}");}
        assert_eq!(report["staticValid"],mismatches==0,"{name}: public staticValid result");
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
        if name=="ordered_copy_versions"||name=="invalidated_mixin_source"{assert_xml_graph_and_definitions(&home,&addon,&compiler,name=="invalidated_mixin_source");}
        reports.push(json!({"case":name,"report":addon.join("report.json"),"expectedValid":mismatches==0,"expectedMismatches":mismatches,"objects":objects,"entries":entries}));
    }
    fs::write(stage.join("report.json"),serde_json::to_vec_pretty(&json!({"source":pin.source,"cases":reports})).unwrap()).unwrap();
    println!("{}",stage.join("report.json").display());
}
fn summary(value:&Value)->Value{json!({"diagnostics":value["diagnostics"],"gaps":value["gaps"],"xmlFacts":value["xmlFacts"],"xmlEntries":value["xmlEntries"]})}

fn assert_xml_graph_and_definitions(
    home: &Path,
    addon: &Path,
    compiler: &luac::Runtime,
    invalidated: bool,
) {
    let control = OperationControl::default();
    let closure = Closure::inspect_with_control(home, addon, "Main.toc", &control).unwrap();
    let (files, _) = xml_inline::inputs(&closure.documents).unwrap();
    let program = Arc::new(xml_load::Program::from_closure(&closure, &control).unwrap());
    let runtime = compiler.manifest();
    let host = TrustedHostEnvironment::from_host_material(
        "xml-query-fixture".into(),
        false,
        vec![],
        vec![],
        &control,
    )
    .unwrap();
    let mut config = emmylua_code_analysis::Emmyrc::default();
    config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    let inputs = SourceInputs::admit(
        SourceInputSpec {
            files: files
                .into_iter()
                .map(|(path, bytes)| {
                    let text = String::from_utf8(bytes).unwrap();
                    SourceFileInput {
                        path,
                        content_sha256: storage::sha256(text.as_bytes()),
                        analysis_utf8: text.clone(),
                        original_utf8: text,
                        map: SourceMap::Identity,
                    }
                })
                .collect(),
            workspace_root: PathBuf::from("C:/lycheedev/xml-query-fixture"),
            source_identity: json!({"fixture":"ordered-xml-original-bytes"}),
            phase: "after_load".into(),
            ordered_occurrences: vec![],
            config,
            host,
            luac: LuacIdentity {
                version: runtime.version.clone(),
                platform: runtime.platform.clone(),
                executable_sha256: runtime.sha256.clone(),
                executable_bytes: runtime.bytes,
            },
            limits: SourceLimits::research(),
        },
        &control,
    )
    .unwrap();
    let prepared = prepare(inputs, &control).unwrap();
    let graph = prepared
        .build_xml_plan("xml-query", program.clone(), None, &control)
        .unwrap();
    let facade = prepared.source_queries(&control).unwrap();
    let query = facade
        .with_execution(&graph, QuerySelector::AllOccurrences, &control)
        .unwrap();
    let xml = graph.xml.as_ref().unwrap();
    let native = graph.native_invocations.as_ref().unwrap();
    assert!(
        xml.references
            .iter()
            .any(|reference| reference.origin.path == "ui.xml"
                && matches!(graph.uses[reference.use_fact].decision, Decision::Bound(_))),
        "XML references retain actual selected graph values"
    );
    let method = xml
        .references
        .iter()
        .find(|reference| reference.role == "handler_method")
        .unwrap();
    let site = Site {
        file: method.origin.path.clone(),
        start: method.origin.bytes.start as u32,
        end: method.origin.bytes.end as u32,
    };
    if invalidated {
        assert!(matches!(
            query.definitions(&site).unwrap().selected,
            QueryFact::Unavailable { .. }
        ));
        let gap = xml
            .gaps
            .iter()
            .find(|gap| {
                gap["code"] == "xml_mixin_source_field_unavailable" && gap["field"] == "Run"
            })
            .unwrap();
        let field_use = &graph.uses[gap["fieldUse"].as_u64().unwrap() as usize];
        assert!(
            matches!(field_use.decision, Decision::Unavailable(_)),
            "invalidated source field remains unavailable in the actual XML load"
        );
        let mixin = xml
            .references
            .iter()
            .find(|reference| reference.role == "mixin")
            .unwrap();
        assert_eq!(field_use.site, graph.uses[mixin.use_fact].site);
        assert_eq!(field_use.context, graph.uses[mixin.use_fact].context);
        assert!(matches!(
            native.query_views.get(&field_use.context),
            Some(NativeQueryView::Xml { .. })
        ));
        assert!(
            xml.copies.is_empty(),
            "invalidated source fields must not claim stale copied writes"
        );
        control.check().unwrap();
        return;
    }
    let definition = query.definitions(&site).unwrap();
    assert!(matches!(
        definition.selected,
        QueryFact::Known {
            value: ValueDefinition {
                function: Some(_),
                ..
            }
        }
    ));
    let mut actions = std::collections::BTreeSet::new();
    for copy in &xml.copies {
        let original = program.original(&copy.origin.path).unwrap();
        assert_eq!(copy.origin.path, "ui.xml");
        assert_eq!(copy.origin.sha256, storage::sha256(original.as_bytes()));
        assert_eq!(
            &original[copy.origin.bytes.start..copy.origin.bytes.end],
            "M",
            "copy provenance uses the original XML attribute bytes"
        );
        let reference = &graph.uses[copy.reference_use];
        let field = &graph.uses[copy.field_use];
        let destination = &graph.writes[copy.destination_field_write];
        let source = &graph.writes[copy.source_field_write];
        assert_eq!(
            reference.site,
            Site {
                file: copy.origin.path.clone(),
                start: copy.origin.bytes.start as u32,
                end: copy.origin.bytes.end as u32
            }
        );
        assert_eq!(field.site, reference.site);
        assert_eq!(field.decision, copy.selected_value);
        assert_eq!(source.decision, copy.selected_value);
        assert_eq!(destination.decision, copy.selected_value);
        assert_eq!(
            field.writes,
            vec![source.id],
            "copy follows the source field's observed write epoch"
        );
        assert_eq!(
            destination.cell,
            format!("object:{}:field:{}", copy.destination_object, copy.field)
        );
        assert_eq!(
            source.cell,
            format!("object:{}:field:{}", copy.source_object, copy.field)
        );
        assert_eq!(destination.inputs, vec![copy.reference_use, copy.field_use]);
        assert_eq!(reference.context, destination.context);
        let Some(NativeQueryView::Xml { action }) = native.query_views.get(&reference.context)
        else {
            panic!("copy must belong to a real XML action")
        };
        actions.insert(*action);
        let xml_load::Action::Mixin {
            instance,
            reference: action_reference,
        } = &program.actions()[*action]
        else {
            panic!("copy action must be a mixin application")
        };
        assert_eq!(action_reference.name, "M");
        assert_eq!(action_reference.origin, copy.origin);
        let object = xml
            .objects
            .iter()
            .find(|object| object.object == copy.destination_object)
            .unwrap();
        assert_eq!(&object.instance, instance);
        assert!(
            matches!(&graph.values[object.value].origin, ValueOrigin::Xml { xml: origin } if origin.path == "ui.xml")
        );
    }
    assert_eq!(
        actions.len(),
        2,
        "the two copied objects retain separate XML actions"
    );
    let flag_copies = xml
        .copies
        .iter()
        .filter(|copy| copy.field == "flag")
        .collect::<Vec<_>>();
    assert_eq!(flag_copies.len(), 2);
    assert_ne!(
        flag_copies[0].source_field_write,
        flag_copies[1].source_field_write
    );
    assert_ne!(
        flag_copies[0].destination_object,
        flag_copies[1].destination_object
    );
    control.check().unwrap();
}
