//! Isolated XML program/source-map prototype. Never used by default Source.
//! Reads explicit bytes; does not evaluate Lua, discover configuration or follow XML loads.
use std::{collections::{BTreeMap, BTreeSet}, fs::{self, File}, io::{Read, Write},
    path::{Component, Path, PathBuf}, time::{Duration, Instant}};



use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const COMMIT: &str = "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b";
const ENGINE: &str = "08b0873962e83bb8307e9a81052e3f41578fbc78";
const XML_RULE_REVISION: &str = "xml-map-v8";
const XSD: &[u8] = include_bytes!("../../../tests/fixtures/source-native/xml-retail-UI.xsd");
const COMPANION: &str = include_str!("../../../tests/fixtures/source-native/xml-companion-manifest.json");
const GOLDEN: &[u8] = include_bytes!("../../../tests/fixtures/source-native/xml-golden-204.xml");
const ORDERED: &[u8] = include_bytes!("../../../tests/fixtures/source-native/xml-ordered.xml");
const SPLIT: &[u8] = include_bytes!("../../../tests/fixtures/source-native/xml-split.xml");
const MAX_FILE: usize = 8 << 20;
const MAX_OUTPUT: usize = 8 << 20;
const SOURCE_DIGEST: &str = "88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928";
const MANIFEST_DIGEST: &str = "7c2ba06f96d302621f9f9f0f505e0f66fd978e1509325119ba769ba511b1934a";
const DOCUMENT_LIMIT: u64 = 128 << 20;
const RETAIL_SECONDS: u64 = 80;
const RETAIL_SCRIPTS: usize = 128_000;
const RETAIL_MAPS: usize = 2_000_000;
const RETAIL_FACTS: usize = 2_000_000;
const RETAIL_OCCURRENCES: usize = 1_000_000;
const RETAIL_DECODED: usize = 64 << 20;
const HASH_FILE_LIMIT: usize = 64 << 20;
const HASH_TOTAL_LIMIT: usize = 512 << 20;
const FAILURE_SAMPLES: usize = 24;

type Result<T> = std::result::Result<T, String>;

fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn err<T>(message: &str) -> Result<T> { Err(message.to_owned()) }

use lycheedev_source::xml_inline::source_map::{Span, Origin};

use lycheedev_source::xml_program::{extract,extract_with_deadline,catalog,ExtractionLimits,XmlProgram,xml_name,SCRIPT_CATALOG_TYPES};

fn require(name: &str, expected: Value, actual: Value, checks: &mut Vec<Value>) {
    checks.push(json!({"name":name,"passed":expected==actual,"expected":expected,"actual":actual}));
}

fn golden() -> Result<Value> {
    let golden = extract("xml-golden-204.xml",GOLDEN)?;
    let ordered = extract("xml-ordered.xml",ORDERED)?;
    let split = extract("xml-split.xml",SPLIT)?;
    let mut checks = Vec::new();
    require("original_204_bytes",json!(204),json!(GOLDEN.len()),&mut checks);
    require("golden_handler_set",json!(["OnEvent","PreClick"]),json!(golden.scripts.iter().map(|s|&s.tag).collect::<Vec<_>>()),&mut checks);
    let run_spans: Vec<_> = golden.scripts.iter().flat_map(|s| s.syntax["names"].as_array().into_iter().flatten())
        .filter(|name|name["name"]=="Run").map(|name|name["sourceMap"]["covering"].clone()).collect();
    require("original_three_run_ranges",json!([Span::new(92,95),Span::new(123,126),Span::new(159,162)]),json!(run_spans),&mut checks);
    for (name,span) in [("event",Span::new(96,101)),("button",Span::new(163,169))] {
        let spans:Vec<_> = golden.scripts.iter().flat_map(|s|s.syntax["names"].as_array().into_iter().flatten())
            .filter(|n|n["name"]==name).map(|n|n["sourceMap"]["covering"].clone()).collect();
        require(&format!("original_{name}_range"),json!([span]),json!(spans),&mut checks);
    }
    let entities:Vec<_> = golden.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::Entity).map(|s|s.raw).collect();
    require("entities_exact_raw_intervals",json!([Span::new(65,71),Span::new(75,81),Span::new(82,87)]),json!(entities),&mut checks);
    require("entity_newline_reverse",json!(1),json!(golden.scripts[0].decoded.reverse(Span::new(83,84))?.len()),&mut checks);
    require("crlf_normalized",json!(true),json!(golden.scripts[0].decoded.segments.iter().any(|s|s.origin==Origin::NewlineNormalization && s.raw.is_some_and(|r|r.len()==2))),&mut checks);
    require("no_parameter_invention",json!(true),json!(golden.scripts.iter().all(|s|s.compiler.text.starts_with("return function(...)\n") && !s.compiler.text.starts_with("return function(self"))),&mut checks);
    require("synthetic_wrapper_origin",json!("Synthetic"),serde_json::to_value(golden.scripts[0].compiler.segments[0].origin).map_err(|e|e.to_string())?,&mut checks);
    let kinds:Vec<_> = ordered.occurrences.iter().map(|o|o.kind.as_str()).collect();
    require("source_occurrence_order",json!(["xml_element","script_file","include","inline_script","include","object_declaration","xml_element","handler_registration","handler_method_reference","handler_registration","handler_function_reference","handler_registration"]),json!(kinds),&mut checks);
    require("duplicate_include_edges_retained",json!(2),json!(ordered.occurrences.iter().filter(|o|o.kind=="include"&&o.attributes["file"].value=="Child.xml").count()),&mut checks);
    require("root_script_is_unwrapped",json!("Root = 1"),json!(ordered.scripts[0].compiler.text),&mut checks);
    require("postclick_discovered",json!("PostClick"),json!(ordered.scripts[1].tag),&mut checks);
    let split_run:Vec<_> = split.scripts[0].syntax["names"].as_array().ok_or("missing syntax names")?.iter()
        .filter(|n|n["name"]=="Run").collect();
    require("split_identifiers_not_comment_or_string",json!(2),json!(split_run.len()),&mut checks);
    require("entity_and_cdata_identifiers_covering",json!(["covering","covering"]),json!(split_run.iter().map(|n|n["sourceMap"]["accuracy"].clone()).collect::<Vec<_>>()),&mut checks);
    require("split_identifiers_keep_multiple_segments",json!(true),json!(split_run.iter().all(|n|n["sourceMap"]["segments"].as_array().is_some_and(|s|s.len()>1))),&mut checks);
    for (name,raw) in [
        ("invalid_entity",include_bytes!("../../../tests/fixtures/source-native/xml-invalid-entity.xml").as_slice()),
        ("invalid_structure",include_bytes!("../../../tests/fixtures/source-native/xml-invalid-structure.xml").as_slice()),
        ("invalid_dtd",include_bytes!("../../../tests/fixtures/source-native/xml-invalid-dtd.xml").as_slice()),
    ] { require(name,json!(true),json!(extract(name,raw).is_err()),&mut checks); }
    let bad = extract("xml-invalid-lua.xml",include_bytes!("../../../tests/fixtures/source-native/xml-invalid-lua.xml"))?;
    require("invalid_lua_no_syntax_facts",json!({"state":"invalid","names":0,"calls":0}),json!({
        "state":bad.scripts[0].syntax["state"],"names":bad.scripts[0].syntax["names"].as_array().map(Vec::len),
        "calls":bad.scripts[0].syntax["calls"].as_array().map(Vec::len)}),&mut checks);
    require("luac_input_count",json!(2),json!(golden.compiler_inputs().len()),&mut checks);
    require("luac_column_remains_unknown",json!(false),golden.scripts[0].compiler_line(2)?["columnKnown"].clone(),&mut checks);
    let newlines=extract("xml-newline-contract.xml",include_bytes!("../../../tests/fixtures/source-native/xml-newline-contract.xml"))?;
    for (index,(label,newline,line,span)) in [
        ("bare_cr","\r",2,Span::new(28,37)),
        ("bare_lf","\n",2,Span::new(70,79)),
        ("paired_crlf","\r\n",2,Span::new(117,126)),
        ("paired_lfcr","\n\r",2,Span::new(164,173)),
        ("repeated_cr","\r\r",3,Span::new(211,220)),
        ("repeated_lf","\n\n",3,Span::new(258,267)),
    ].into_iter().enumerate() {
        let script=&newlines.scripts[index];
        let mapping=script.compiler_line(line).unwrap_or_else(|error|json!({"error":error}));
        require(&format!("lua51_line_mapping_{label}"),json!({"input":format!("local x = 1{newline}local = 2"),
            "rawCovering":span,"columnKnown":false,"nextLineOutside":true}),
            json!({"input":script.compiler.text,"rawCovering":mapping["sourceMap"]["covering"],
                "columnKnown":mapping["columnKnown"],"nextLineOutside":script.compiler_line(line+1).is_err()}),&mut checks);
    }
    for (name,raw,line,span,input) in [
        ("top",include_bytes!("../../../tests/fixtures/source-native/xml-entity-cr-top.xml").as_slice(),2,Span::new(28,37),
            "local x = 1\rlocal = 2"),
        ("handler",include_bytes!("../../../tests/fixtures/source-native/xml-entity-cr-handler.xml").as_slice(),3,Span::new(44,53),
            "return function(...)\nlocal x = 1\rlocal = 2\nend\n"),
    ] {
        let source=extract(name,raw)?;
        let script=&source.scripts[0];
        let mapping=script.compiler_line(line).unwrap_or_else(|error|json!({"error":error}));
        require(&format!("actual_cr_red_regression_{name}"),json!({"input":input,"rawCovering":span,"columnKnown":false}),
            json!({"input":script.compiler.text,"rawCovering":mapping["sourceMap"]["covering"],
                "columnKnown":mapping["columnKnown"]}),&mut checks);
    }
    let empty_control=include_bytes!("../../../tests/fixtures/source-native/xml-empty-control.xml").as_slice();
    let empty_bom=include_bytes!("../../../tests/fixtures/source-native/xml-empty-bom.xml").as_slice();
    require("bom_empty_fixture_exact_prefix",json!(true),json!(empty_bom.starts_with(b"\xef\xbb\xbf")
        && empty_bom.get(3..)==Some(empty_control)),&mut checks);
    for (label,raw,prefix) in [("control",empty_control,0usize),("bom",empty_bom,3usize)] {
        let program=extract(label,raw)?;
        let shifted=|start,end|Span::new(start+prefix,end+prefix);
        let expected_ranges:Vec<_>=[(0,4),(4,13),(13,35),(35,58),(58,78),(78,97),
            (97,128),(128,137),(137,145),(182,203),(203,211)].into_iter()
            .map(|(start,end)|shifted(start,end)).collect();
        require(&format!("empty_elements_{label}_original_open_ranges"),json!(expected_ranges),
            json!(program.occurrences.iter().map(|o|o.raw).collect::<Vec<_>>()),&mut checks);
        require(&format!("empty_elements_{label}_ancestry"),json!([null,0,0,0,0,4,4,4,7,0,0]),
            json!(program.occurrences.iter().map(|o|o.parent).collect::<Vec<_>>()),&mut checks);
        require(&format!("empty_elements_{label}_whole_script_ranges_and_text"),json!([
            {"tag":"OnLoad","element":shifted(137,164),"decoded":"self:Run()","compiler":"return function(...)\nself:Run()\nend\n"},
            {"tag":"Script","element":shifted(203,228),"decoded":"Root = 1","compiler":"Root = 1"}]),
            json!(program.scripts.iter().map(|s|json!({"tag":s.tag,"element":s.element,
                "decoded":s.decoded.text,"compiler":s.compiler.text})).collect::<Vec<_>>()),&mut checks);
        let script=&program.scripts[0];
        let run=script.syntax["names"].as_array().into_iter().flatten().find(|n|n["name"]=="Run");
        require(&format!("empty_elements_{label}_body_and_synthetic_anchors"),json!({
            "prefix":prefix,"decodedRaw":shifted(145,155),"run":shifted(150,153),
            "wrapperStart":shifted(137,137),"wrapperEnd":shifted(155,164)}),json!({
            "prefix":program.coverage["rawCursorPrefixBytes"],"decodedRaw":script.decoded.segments[0].raw,
            "run":run.map(|n|n["sourceMap"]["covering"].clone()),
            "wrapperStart":script.compiler.segments.first().and_then(|s|s.anchor),
            "wrapperEnd":script.compiler.segments.last().and_then(|s|s.anchor)}),&mut checks);
    }
    let catalog_fixture=extract("xml-handler-catalog.xml",include_bytes!("../../../tests/fixtures/source-native/xml-handler-catalog.xml"))?;
    let class_counts:BTreeMap<_,_>=catalog_fixture.coverage["handlerCatalogDeclarations"].as_object().ok_or("missing handler catalog")?
        .iter().map(|(name,handlers)|(name,handlers.as_object().map_or(0,serde_json::Map::len))).collect();
    require("catalog_four_schema_classes",json!({"ScriptsType":75,"ActorScriptsType":6,"AnimScriptsType":6,"AnimGroupScriptsType":7}),
        json!(class_counts),&mut checks);
    require("catalog_six_missing_handler_exact_sources",json!([
        {"tag":"OnModelLoading","element":Span::new(20,65),"text":"self:Ready()"},
        {"tag":"OnLoop","element":Span::new(108,136),"text":"self:Loop()"},
        {"tag":"OnPlay","element":Span::new(172,200),"text":"self:Play()"},
        {"tag":"OnPause","element":Span::new(200,231),"text":"self:Pause()"},
        {"tag":"OnStop","element":Span::new(231,259),"text":"self:Stop()"},
        {"tag":"OnFinished","element":Span::new(259,297),"text":"self:Finish()"}]),
        json!(catalog_fixture.scripts.iter().map(|s|json!({"tag":s.tag,"element":s.element,"text":s.decoded.text})).collect::<Vec<_>>()),&mut checks);
    for (tag,owner,owner_type,class,provider,path,declaration) in [
        ("OnModelLoading","Actor","ModelSceneActorType","ActorScriptsType","ModelSceneActorType",
            vec!["ModelSceneActorType"],Span::new(49236,49289)),
        ("OnLoop","AnimationGroup","AnimationGroupType","AnimGroupScriptsType","AnimationGroupType",
            vec!["AnimationGroupType"],Span::new(61227,61272)),
        ("OnPlay","LineTranslation","LineTranslationType","AnimScriptsType","AnimationType",
            vec!["LineTranslationType","TranslationType","AnimationType"],Span::new(60511,60556)),
        ("OnPause","LineTranslation","LineTranslationType","AnimScriptsType","AnimationType",
            vec!["LineTranslationType","TranslationType","AnimationType"],Span::new(60561,60607)),
        ("OnStop","LineTranslation","LineTranslationType","AnimScriptsType","AnimationType",
            vec!["LineTranslationType","TranslationType","AnimationType"],Span::new(60612,60657)),
        ("OnFinished","LineTranslation","LineTranslationType","AnimScriptsType","AnimationType",
            vec!["LineTranslationType","TranslationType","AnimationType"],Span::new(60714,60763)),
    ] {
        let proof=catalog_fixture.occurrences.iter().find(|o|o.tag==tag&&o.kind=="handler_registration")
            .and_then(|o|o.handler_catalog.as_ref()).ok_or("missing handler provenance")?;
        require(&format!("catalog_owner_provenance_{tag}"),json!({"owner":owner,"ownerType":owner_type,
            "catalogType":class,"provider":provider,"inheritance":path,"declaration":declaration,"resolution":"matched"}),
            json!({"owner":proof["ownerElement"],"ownerType":proof["ownerType"],
                "catalogType":proof["scriptContainer"]["catalogType"],"provider":proof["scriptContainer"]["declaredByType"],
                "inheritance":proof["inheritance"].as_array().into_iter().flatten().map(|step|step["type"].clone()).collect::<Vec<_>>(),
                "declaration":proof["matchedDeclaration"],"resolution":proof["ownerResolution"]}),&mut checks);
    }
    let frame_proof=golden.occurrences.iter().find(|o|o.tag=="OnEvent"&&o.kind=="handler_registration")
        .and_then(|o|o.handler_catalog.as_ref()).ok_or("missing frame handler provenance")?;
    require("catalog_frame_scripts_substitution_provenance",json!({"ownerType":"FrameType","catalogType":"ScriptsType",
        "provider":"FrameType","via":"element_reference_substitution","path":["Scripts","LayoutField"],"resolution":"matched"}),
        json!({"ownerType":frame_proof["ownerType"],"catalogType":frame_proof["scriptContainer"]["catalogType"],
            "provider":frame_proof["scriptContainer"]["declaredByType"],"via":frame_proof["scriptContainer"]["via"],
            "path":frame_proof["scriptContainer"]["substitutionPath"],"resolution":frame_proof["ownerResolution"]}),&mut checks);
    require("catalog_implicit_parameters_still_not_injected",json!(true),json!(catalog_fixture.scripts.iter()
        .all(|s|s.compiler.text.starts_with("return function(...)\n")&&s.implicit_parameters.starts_with("not_injected"))),&mut checks);
    require("catalog_unknown_handler_still_rejected",json!(true),json!(extract("unknown-handler",
        b"<Ui><AnimationGroup><Scripts><OnInvented/></Scripts></AnimationGroup></Ui>").is_err()),&mut checks);
    let mismatch=extract("owner-mismatch",b"<Ui><Frame><Scripts><OnLoop/></Scripts></Frame></Ui>")?;
    let mismatch_proof=mismatch.occurrences.iter().find_map(|o|o.handler_catalog.as_ref()).ok_or("missing mismatch provenance")?;
    require("catalog_owner_mismatch_is_explicit",json!("known_owner_mismatch"),mismatch_proof["ownerResolution"].clone(),&mut checks);
    // Keep the exact six independently observed malformed inputs unchanged.
    for (name,input) in [
        ("attribute_raw_lt","<Ui a=\"<\"/>"),
        ("text_cdata_close","<Ui>]]></Ui>"),
        ("tail_bom","<Ui/>\u{feff}"),
        ("tail_nbsp","<Ui/>\u{a0}"),
        ("invalid_element_name","<Ui><1bad/></Ui>"),
        ("invalid_attribute_name","<Ui 1bad=\"x\"/>"),
    ] {
        require(&format!("xml10_actual_negative_{name}"),json!(true),json!(extract(name,input.as_bytes()).is_err()),&mut checks);
    }
    let lexical=extract("xml-lexical-valid.xml",include_bytes!("../../../tests/fixtures/source-native/xml-lexical-valid.xml"))?;
    require("xml10_legal_cdata_comments_escaped_close_and_spaces",json!({"scripts":1,
        "text":r#"local a = 1 < 2; local s = "]]>";local t = "]]>";"#,"parsed":true}),
        json!({"scripts":lexical.scripts.len(),"text":lexical.scripts[0].compiler.text,
            "parsed":lexical.scripts[0].syntax["state"]=="parsed"}),&mut checks);
    require("xml10_legal_unicode_names_and_namespace_attributes",json!({
        "tags":["Ui","Élément_名-0.\u{300}\u{203f}","\u{10000}","Script"],
        "namespace":"http://www.blizzard.com/wow/ui/","namespaceAttribute":"http://www.w3.org/2001/XMLSchema-instance",
        "schemaLocation":"UI.xsd","escapedLt":"<","middleDotName":"ok","supplementaryName":"v"}),
        json!({"tags":lexical.occurrences.iter().map(|o|&o.tag).collect::<Vec<_>>(),
            "namespace":lexical.occurrences[0].attributes["xmlns"].value,
            "namespaceAttribute":lexical.occurrences[0].attributes["xmlns:xsi"].value,
            "schemaLocation":lexical.occurrences[0].attributes["xsi:noNamespaceSchemaLocation"].value,
            "escapedLt":lexical.occurrences[0].attributes["窗口"].value,
            "middleDotName":lexical.occurrences[0].attributes["a·b"].value,
            "supplementaryName":lexical.occurrences[1].attributes["\u{10000}"].value}),&mut checks);
    require("xml10_legal_segmented_cdata_and_entity_map",json!({"segments":5,"entitySegments":1,"accuracy":"covering"}),
        json!({"segments":lexical.scripts[0].decoded.segments.len(),
            "entitySegments":lexical.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::Entity).count(),
            "accuracy":lexical.scripts[0].decoded.project(Span::new(0,lexical.scripts[0].decoded.text.len()))?["accuracy"]}),&mut checks);
    let valid_names=["_a","a-0.","É","名","a\u{b7}","a\u{300}","a\u{203f}","\u{200c}","\u{10000}","\u{effff}"];
    require("xml10_unicode_name_start_and_continuation_acceptance",json!(vec![true;valid_names.len()]),
        json!(valid_names.iter().map(|name|extract("valid-name",format!("<Ui><{name} {name}=\"v\"/></Ui>").as_bytes()).is_ok()).collect::<Vec<_>>()),&mut checks);
    let invalid_names=["","1bad","-bad",".bad","\u{300}bad","\u{b7}bad","\u{37e}bad",
        "bad\u{d7}","bad\u{200b}","bad\u{fdd0}","bad\u{f0000}","bad?","bad<"];
    require("xml10_name_range_gaps_and_invalid_starts",json!(vec![false;invalid_names.len()]),
        json!(invalid_names.iter().map(|name|xml_name(name)).collect::<Vec<_>>()),&mut checks);
    require("xml10_leading_bom_and_all_four_outside_spaces",json!(true),
        json!(extract("leading-bom-whitespace",b"\xef\xbb\xbf \t\r\n<Ui/> \t\r\n").is_ok()),&mut checks);
    require("xml10_misplaced_declaration_rejected",json!(true),
        json!(extract("misplaced-declaration",b" <?xml version=\"1.0\"?><Ui/>").is_err()),&mut checks);
    let binding_raw=include_bytes!("../../../tests/fixtures/source-native/xml-bindings-394.xml");
    let bindings=extract("xml-bindings-394.xml",binding_raw)?;
    require("bindings_exact_fixture",json!({"bytes":394,"sha256":"aca963840dc4e0201609be4443c35f4985199d72556500c95e6957db18c16e96"}),
        json!({"bytes":binding_raw.len(),"sha256":hash(binding_raw)}),&mut checks);
    require("bindings_profile_counts_and_order",json!({"profile":"Bindings","scripts":2,
        "kinds":["bindings_root","binding_declaration","modified_click","binding_declaration","binding_declaration"],
        "ranges":[Span::new(0,394),Span::new(12,149),Span::new(198,246),Span::new(248,312),Span::new(314,381)]}),
        json!({"profile":bindings.source_profile,"scripts":bindings.scripts.len(),
            "kinds":bindings.occurrences.iter().map(|o|&o.kind).collect::<Vec<_>>(),
            "ranges":bindings.occurrences.iter().map(|o|o.raw).collect::<Vec<_>>()}),&mut checks);
    require("bindings_duplicate_names_and_empty_metadata",json!({"names":["A&B","Empty","A&B"],"emptyScript":null,
        "context":"HousingEditorLayoutMode","runOnUp":"true","header":"H","category":"C"}),
        json!({"names":bindings.occurrences.iter().filter_map(|o|o.attributes.get("name").map(|a|&a.value)).collect::<Vec<_>>(),
            "emptyScript":bindings.occurrences[3].script,"context":bindings.occurrences[3].attributes["bindingContext"].value,
            "runOnUp":bindings.occurrences[1].attributes["runOnUp"].value,"header":bindings.occurrences[1].attributes["header"].value,
            "category":bindings.occurrences[1].attributes["category"].value}),&mut checks);
    require("bindings_modified_click_is_metadata",json!({"action":"SELFCAST","default":"ALT","script":null}),
        json!({"action":bindings.occurrences[2].attributes["action"].value,"default":bindings.occurrences[2].attributes["default"].value,
            "script":bindings.occurrences[2].script}),&mut checks);
    require("bindings_decoded_bodies_and_raw_spans",json!({"text":["Run(keystate)\nRun(keystate)\rreturn","local keystate=\"up\";Run(keystate)"],
        "bodies":[Span::new(75,139),Span::new(338,371)],"elements":[Span::new(12,149),Span::new(314,381)]}),
        json!({"text":bindings.scripts.iter().map(|s|&s.decoded.text).collect::<Vec<_>>(),
            "bodies":bindings.scripts.iter().map(|s|s.binding_body_raw).collect::<Vec<_>>(),
            "elements":bindings.scripts.iter().map(|s|s.element).collect::<Vec<_>>()}),&mut checks);
    let binding_runs:Vec<_>=bindings.scripts.iter().flat_map(|s|s.syntax["names"].as_array().into_iter().flatten())
        .filter(|n|n["name"]=="Run").map(|n|n["sourceMap"].clone()).collect();
    require("bindings_run_token_covering_ranges",json!([Span::new(75,92),Span::new(107,118),Span::new(358,361)]),
        json!(binding_runs.iter().map(|m|&m["covering"]).collect::<Vec<_>>()),&mut checks);
    require("bindings_run_token_exact_segment_ranges",json!([
        [Span::new(75,76),Span::new(76,82),Span::new(91,92)],
        [Span::new(107,108),Span::new(116,118)],[Span::new(358,361)]]),
        json!(binding_runs.iter().map(|m|m["segments"].as_array().into_iter().flatten().map(|s|s["raw"].clone()).collect::<Vec<_>>()).collect::<Vec<_>>()),&mut checks);
    require("bindings_run_token_origins_and_accuracy",json!({"origins":[["Exact","Entity","Exact"],["Exact","Exact"],["Exact"]],
        "accuracy":["covering","covering","exact"]}),json!({
        "origins":binding_runs.iter().map(|m|m["segments"].as_array().into_iter().flatten().map(|s|s["origin"].clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "accuracy":binding_runs.iter().map(|m|&m["accuracy"]).collect::<Vec<_>>()}),&mut checks);
    require("bindings_newline_origins",json!({"physical":[Span::new(105,107)],"entityCR":[Span::new(128,133)]}),json!({
        "physical":bindings.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::NewlineNormalization).map(|s|s.raw).collect::<Vec<_>>(),
        "entityCR":bindings.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::Entity && &bindings.scripts[0].decoded.text[s.output.start..s.output.end]=="\r").map(|s|s.raw).collect::<Vec<_>>()}),&mut checks);
    require("bindings_comment_gap_maps_to_no_output",json!([]),json!(bindings.scripts[0].decoded.reverse(Span::new(108,116))?),&mut checks);
    require("bindings_keystate_tokens_include_unresolved_local_shadow",json!([Span::new(96,104),Span::new(119,127),Span::new(344,352),Span::new(362,370)]),
        json!(bindings.scripts.iter().flat_map(|s|s.syntax["names"].as_array().into_iter().flatten())
            .filter(|n|n["name"]=="keystate").map(|n|n["sourceMap"]["covering"].clone()).collect::<Vec<_>>()),&mut checks);
    require("bindings_comment_fake_and_binding_identifiers_are_not_lua_names",json!(true),
        json!(bindings.scripts.iter().flat_map(|s|s.syntax["names"].as_array().into_iter().flatten())
            .all(|n|!matches!(n["name"].as_str(),Some("FAKE"|"Ghost"|"A&B"|"Empty")))),&mut checks);
    require("bindings_wrapper_without_signature_invention",json!(true),json!(bindings.scripts.iter().all(|s|
        s.kind=="binding" && s.compiler.text==format!("return function(...)\n{}\nend\n",s.decoded.text)
        && s.syntax["state"]=="parsed" && s.implicit_parameters.starts_with("not_injected"))),&mut checks);
    require("bindings_no_semantic_or_schema_claim",json!({"semanticComplete":false,
        "schema":"not_performed; UI.xsd is not a Bindings schema","bindingFacts":[false,false]}),json!({
        "semanticComplete":bindings.coverage["semanticComplete"],"schema":bindings.coverage["schemaValidation"],
        "bindingFacts":bindings.scripts.iter().map(|s|&s.syntax["bindingFactsExported"]).collect::<Vec<_>>()}),&mut checks);
    let metadata=extract("binding-metadata",b"<Bindings xmlns=\"urn:test\" custom=\"root\"><Binding name=\"Open Issue Report\" default=\"F6\" hidden=\"true\"></Binding></Bindings>")?;
    require("bindings_root_unknown_attributes_space_name_paired_empty",json!({"root":"root","namespace":"urn:test","name":"Open Issue Report",
        "default":"F6","hidden":"true","scripts":0}),json!({"root":metadata.occurrences[0].attributes["custom"].value,
        "namespace":metadata.occurrences[0].attributes["xmlns"].value,"name":metadata.occurrences[1].attributes["name"].value,
        "default":metadata.occurrences[1].attributes["default"].value,"hidden":metadata.occurrences[1].attributes["hidden"].value,
        "scripts":metadata.scripts.len()}),&mut checks);
    for (name,input) in [
        ("unknown_direct_child","<Bindings><Unknown/></Bindings>"),
        ("nested_binding_markup","<Bindings><Binding name=\"A\"><Frame/></Binding></Bindings>"),
        ("nested_modified_click","<Bindings><ModifiedClick><Binding/></ModifiedClick></Bindings>"),
        ("modified_click_body","<Bindings><ModifiedClick>Run()</ModifiedClick></Bindings>"),
    ] {
        require(&format!("bindings_reject_{name}"),json!(true),json!(extract(name,input.as_bytes()).is_err()),&mut checks);
    }
    // Same v6 input, corrected v7 contract: XML character data remains evidence,
    // never Lua source or approval of native profile/runtime behavior.
    let root_text=extract("root_executable_text",b"<Bindings>Run()</Bindings>")?;
    require("bindings_root_executable_text_retained_with_invalid_profile",json!({"text":"Run()","span":Span::new(10,15),
        "scripts":0,"extractionComplete":true,"profileValidationPassed":false,"diagnostics":[{
            "code":"binding_root_text","severity":"warning","occurrence":1,"raw":Span::new(10,15),
            "validationState":"unresolved","nativeRuntimeBehavior":"unknown"}]}),json!({
        "text":root_text.occurrences[1].source_text,"span":root_text.occurrences[1].raw,"scripts":root_text.scripts.len(),
        "extractionComplete":root_text.coverage["extractionComplete"],"profileValidationPassed":root_text.coverage["profileValidationPassed"],
        "diagnostics":root_text.diagnostics}),&mut checks);
    let root_text_raw=include_bytes!("../../../tests/fixtures/source-native/xml-bindings-root-text.xml");
    let root_text=extract("xml-bindings-root-text.xml",root_text_raw)?;
    require("bindings_root_text_fixture_exact_bytes",json!({"bytes":90,"sha256":"a565f2446b7a976fd8f42b6acdc6e56808c7b2dc4d80a6d8a74f10a68e0cee5d"}),
        json!({"bytes":root_text_raw.len(),"sha256":hash(root_text_raw)}),&mut checks);
    require("bindings_root_text_retains_both_declarations_in_order",json!({
        "kinds":["bindings_root","binding_declaration","unexpected_profile_text","binding_declaration"],
        "ranges":[Span::new(0,90),Span::new(10,43),Span::new(43,45),Span::new(45,79)],"names":["A","B"]}),json!({
        "kinds":root_text.occurrences.iter().map(|o|&o.kind).collect::<Vec<_>>(),
        "ranges":root_text.occurrences.iter().map(|o|o.raw).collect::<Vec<_>>(),
        "names":root_text.occurrences.iter().filter_map(|o|o.attributes.get("name").map(|a|&a.value)).collect::<Vec<_>>()}),&mut checks);
    require("bindings_root_text_preserves_raw_event_and_parent",json!({"text":">\n","raw":Span::new(43,45),"tag":"#text","parent":0,"script":null}),
        json!({"text":root_text.occurrences[2].source_text,"raw":root_text.occurrences[2].raw,"tag":root_text.occurrences[2].tag,
            "parent":root_text.occurrences[2].parent,"script":root_text.occurrences[2].script}),&mut checks);
    require("bindings_root_text_diagnostic_nonspace_subrange",json!([{"code":"binding_root_text","severity":"warning",
        "occurrence":2,"raw":Span::new(43,44),"validationState":"unresolved","nativeRuntimeBehavior":"unknown"}]),
        json!(root_text.diagnostics),&mut checks);
    require("bindings_root_text_keeps_only_binding_bodies",json!({"bodies":["Run()","Next()"],
        "ranges":[Span::new(28,33),Span::new(63,69)],"names":["Run","Next"],"calls":2}),json!({
        "bodies":root_text.scripts.iter().map(|s|&s.decoded.text).collect::<Vec<_>>(),
        "ranges":root_text.scripts.iter().map(|s|s.binding_body_raw).collect::<Vec<_>>(),
        "names":root_text.scripts.iter().flat_map(|s|s.syntax["names"].as_array().into_iter().flatten()).map(|n|&n["name"]).collect::<Vec<_>>(),
        "calls":root_text.scripts.iter().map(|s|s.syntax["calls"].as_array().map_or(0,Vec::len)).sum::<usize>()}),&mut checks);
    let (root_text_counts,root_text_summary)=summarize_program(&root_text,HASH_FILE_LIMIT)?;
    require("bindings_root_text_summary_keeps_diagnostics_visible",json!({"diagnostics":1,"filesWithDiagnostics":1,
        "extractionComplete":true,"profileValidationPassed":false,"profileValidationState":"unresolved",
        "nativeRuntimeBehavior":"unknown","retainedText":">\n","diagnosticRaw":Span::new(43,44)}),json!({
        "diagnostics":root_text_counts.profile_diagnostics,"filesWithDiagnostics":root_text_counts.files_with_profile_diagnostics,
        "extractionComplete":root_text_summary["extractionComplete"],"profileValidationPassed":root_text_summary["profileValidationPassed"],
        "profileValidationState":root_text_summary["profileValidationState"],"nativeRuntimeBehavior":root_text_summary["nativeRuntimeBehavior"],
        "retainedText":root_text_summary["unexpectedProfileText"][0]["sourceText"],"diagnosticRaw":root_text_summary["profileDiagnostics"][0]["raw"]}),&mut checks);
    require("bindings_clean_fixture_has_no_profile_diagnostics",json!({"passed":true,"diagnostics":[]}),
        json!({"passed":bindings.coverage["profileValidationPassed"],"diagnostics":bindings.diagnostics}),&mut checks);
    // Preserve the six actual failing source strings from the independent runner.
    for (name,input) in [
        ("attribute_missing_separator",r#"<Ui a="1"b="2"/>"#),
        ("declaration_standalone_invalid",r#"<?xml version="1.0" standalone="maybe"?><Ui/>"#),
        ("declaration_unknown_field",r#"<?xml version="1.0" other="x"?><Ui/>"#),
        ("declaration_duplicate_version",r#"<?xml version="1.0" version="1.0"?><Ui/>"#),
        ("declaration_field_order",r#"<?xml version="1.0" standalone="yes" encoding="UTF-8"?><Ui/>"#),
        ("declaration_invalid_tail",r#"<?xml version="1.0" encoding="UTF-8" broken?><Ui/>"#),
    ] {
        require(&format!("xml10_actual_declaration_negative_{name}"),json!(true),json!(extract(name,input.as_bytes()).is_err()),&mut checks);
    }
    let legal_declarations=[
        "<?xml version=\"1.0\"?><Ui/>",
        "<?xml version='1.0' ?><Ui/>",
        "<?xml version='1.0' encoding='UTF-8'?><Ui/>",
        "<?xml version='1.0' standalone='yes'?><Ui/>",
        "<?xml version='1.0' encoding='utf-8' standalone='no'?><Ui/>",
        "\u{feff}<?xml\tversion\n=\r'1.0' \tencoding \r= \n'UtF-8' \rstandalone \t= \n'yes'\t?><Ui/>",
    ];
    require("xml10_legal_complete_declaration_variants",json!(vec![true;legal_declarations.len()]),
        json!(legal_declarations.iter().map(|input|extract("legal-declaration",input.as_bytes()).is_ok()).collect::<Vec<_>>()),&mut checks);
    let attribute_separators=[' ','\t','\r','\n'];
    require("xml10_element_attributes_accept_each_xml_s_separator",json!([true,true,true,true]),
        json!(attribute_separators.iter().map(|separator|extract("legal-attribute-separator",
            format!("<Ui a='甲'{separator}b = \"乙\"/>").as_bytes())
            .is_ok_and(|program|program.occurrences[0].attributes["b"].value=="乙")).collect::<Vec<_>>()),&mut checks);
    let declaration_raw=include_bytes!("../../../tests/fixtures/source-native/xml-declaration-valid.xml");
    let declaration=extract("xml-declaration-valid.xml",declaration_raw)?;
    require("xml10_legal_declaration_unicode_attribute_fixture",json!({"bytes":156,
        "sha256":"59b1714957341a5da89ffb260556b08cb758036bfe6f1247d8dd2d6a66a29144",
        "prefix":3,"whitespace":"甲 乙 丙 丁 戊","name":"A&B","empty":"","body":"Run()"}),json!({
        "bytes":declaration_raw.len(),"sha256":hash(declaration_raw),"prefix":declaration.coverage["rawCursorPrefixBytes"],
        "whitespace":declaration.occurrences[0].attributes["空白"].value,"name":declaration.occurrences[0].attributes["name"].value,
        "empty":declaration.occurrences[0].attributes["empty"].value,"body":declaration.scripts[0].decoded.text}),&mut checks);
    require("xml10_legal_declaration_preserves_original_body_range",json!([Span::new(137,140)]),
        json!(declaration.scripts[0].syntax["names"].as_array().into_iter().flatten().filter(|n|n["name"]=="Run")
            .map(|n|n["sourceMap"]["covering"].clone()).collect::<Vec<_>>()),&mut checks);
    let invalid_declarations=[
        "<?xml?><Ui/>",
        "<?xml encoding='UTF-8'?><Ui/>",
        "<?xml version='1.1'?><Ui/>",
        "<?xml version='1.0'encoding='UTF-8'?><Ui/>",
        "<?xml version='1.0' encoding='UTF-8'standalone='yes'?><Ui/>",
        "<?xml version='1.0' encoding='UTF-8' encoding='UTF-8'?><Ui/>",
        "<?xml version='1.0' standalone='yes' standalone='no'?><Ui/>",
        "<?xml version='1.0' standalone='YES'?><Ui/>",
        "<?xml version='1.0' encoding='UTF-16'?><Ui/>",
        "<?xml version='1.&#48;'?><Ui/>",
        "<?xml version='1.0' standalone='&#121;es'?><Ui/>",
        "<?xml version=1.0?><Ui/>",
        "<?xml version '1.0'?><Ui/>",
        "<?xml version='1.0'",
        "<?xml version='1.0' encoding=\"UTF-8?><Ui/>",
        "<Ui a='1'b='2'/>",
        "<Ui a='1'\u{a0}b='2'/>",
        "<Ui a='1' a='2'/>",
        "<Ui a='1' broken/>",
    ];
    require("xml10_declaration_complete_tail_and_attribute_framing_negatives",json!(vec![true;invalid_declarations.len()]),
        json!(invalid_declarations.iter().map(|input|extract("invalid-declaration-or-attribute",input.as_bytes()).is_err()).collect::<Vec<_>>()),&mut checks);
    require("xml10_late_attribute_failure_is_transactional",json!(true),
        json!(extract("late-invalid-attribute",b"<Ui><Script>Run()</Script><Frame a=\"1\"b=\"2\"/></Ui>").is_err()),&mut checks);
    let passed=checks.iter().all(|check|check["passed"]==true);
    Ok(json!({"schema":"lycheedev.emmy-xml-prototype.v1","engine":ENGINE,"sourceCommit":COMMIT,"xmlRuleRevision":XML_RULE_REVISION,
        "state":if passed{"passed"}else{"failed"},"checks":checks,"companion":serde_json::from_str::<Value>(COMPANION).map_err(|e|e.to_string())?,
        "fixtures":[golden,ordered,split,bad],"execution":"never","luacActualRun":"not_run",
        "semanticComplete":false,"scope":"XML extraction, byte mapping and Emmy syntax facts only"}))
}

// Manifest and confined-file reading follow the already reviewed Retail loader in
// emmy_library_probe.rs. Kept local so this isolated example does not load its
// semantic engine or modify the default Source command surface.
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct InputManifest {
    schema: String, commit: String, documents: usize, lua_files: usize, lua_bytes: u64,
    #[serde(rename="documentPathSHA256SizeDigest")]
    document_digest: String,
    files: Vec<InputFile>,
}
#[derive(Deserialize)]
struct InputFile { path: String, bytes: u64, sha256: String, mode: String }

fn regular_file(path: &Path) -> Result<fs::Metadata> {
    let meta=fs::symlink_metadata(path).map_err(|e|e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() { return err("xml.input_not_regular_nonlink_file"); }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return err("xml.input_reparse_point"); }
    }
    Ok(meta)
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    if !path.is_absolute() { return err("xml.input_path_not_absolute"); }
    let meta=regular_file(path)?;
    if meta.len()>maximum { return err("xml.input_byte_budget"); }
    let file=File::open(path).map_err(|e|e.to_string())?;
    let mut bytes=Vec::with_capacity(meta.len() as usize);
    file.take(maximum+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
    if bytes.len() as u64>maximum || bytes.len() as u64!=meta.len() { return err("xml.input_changed_length"); }
    Ok(bytes)
}

fn checked_relative(path: &str) -> Result<PathBuf> {
    if path.is_empty() || path.len()>4096 || path.chars().any(|c|matches!(c,'\\'|':'|'\0'))
        || path.split('/').any(|p|p.is_empty()||p=="."||p=="..") { return err("xml.invalid_manifest_path"); }
    let path=PathBuf::from(path);
    if path.components().any(|part|!matches!(part,Component::Normal(_))) { return err("xml.manifest_path_not_relative"); }
    Ok(path)
}

fn contained_file(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path=root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        let meta=fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
        if meta.file_type().is_symlink() { return err("xml.source_symlink"); }
        #[cfg(windows)] {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 { return err("xml.source_reparse_point"); }
        }
    }
    Ok(path)
}

fn verified_bytes(root: &Path, input: &InputFile) -> Result<Vec<u8>> {
    let path=contained_file(root,&checked_relative(&input.path)?)?;
    let bytes=read_bounded(&path,MAX_FILE as u64)?;
    if bytes.len() as u64!=input.bytes || hash(&bytes)!=input.sha256 { return err("xml.fixed_file_hash_or_length_mismatch"); }
    Ok(bytes)
}

struct HashWriter { hash: Sha256, bytes: usize, limit:usize }
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.saturating_add(bytes.len())>self.limit { return Err(std::io::Error::other("xml.hash_serialization_budget")); }
        self.hash.update(bytes); self.bytes+=bytes.len(); Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn hash_value(value: &impl Serialize) -> Result<(String,usize)> {
    hash_value_limited(value,HASH_FILE_LIMIT)
}
fn hash_value_limited(value: &impl Serialize,limit:usize) -> Result<(String,usize)> {
    let mut writer=HashWriter { hash:Sha256::new(),bytes:0,limit:limit.min(HASH_FILE_LIMIT) };
    serde_json::to_writer(&mut writer,value).map_err(|e|e.to_string())?;
    Ok((format!("{:x}",writer.hash.finalize()),writer.bytes))
}

#[derive(Default, Serialize)]
#[serde(rename_all="camelCase")]
struct RetailCounts {
    profile_diagnostics:usize, files_with_profile_diagnostics:usize,
    binding_files:usize, binding_raw_bytes:usize, binding_declarations:usize, modified_clicks:usize, binding_scripts:usize,
    occurrences:usize, scripts:usize, top_level:usize, handlers:usize, load_edges:usize,
    handler_registrations:usize, includes:usize, script_files:usize,
    handler_owners_matched:usize, handler_owners_unknown:usize, handler_owners_mismatched:usize,
    decoded_bytes:usize, compiler_bytes:usize, decoded_segments:usize, compiler_segments:usize,
    names:usize, calls:usize, syntax_error_scripts:usize, syntax_errors:usize,
    hashed_serialization_bytes:usize,
}
impl RetailCounts {
    fn add(&mut self, other:&Self) {
        self.profile_diagnostics+=other.profile_diagnostics;
        self.files_with_profile_diagnostics+=other.files_with_profile_diagnostics;
        self.binding_files+=other.binding_files; self.binding_raw_bytes+=other.binding_raw_bytes;
        self.binding_declarations+=other.binding_declarations; self.modified_clicks+=other.modified_clicks;
        self.binding_scripts+=other.binding_scripts;
        self.occurrences+=other.occurrences; self.scripts+=other.scripts;
        self.top_level+=other.top_level; self.handlers+=other.handlers; self.load_edges+=other.load_edges;
        self.handler_registrations+=other.handler_registrations; self.includes+=other.includes; self.script_files+=other.script_files;
        self.handler_owners_matched+=other.handler_owners_matched;
        self.handler_owners_unknown+=other.handler_owners_unknown;
        self.handler_owners_mismatched+=other.handler_owners_mismatched;
        self.decoded_bytes+=other.decoded_bytes; self.compiler_bytes+=other.compiler_bytes;
        self.decoded_segments+=other.decoded_segments; self.compiler_segments+=other.compiler_segments;
        self.names+=other.names; self.calls+=other.calls; self.syntax_error_scripts+=other.syntax_error_scripts;
        self.syntax_errors+=other.syntax_errors; self.hashed_serialization_bytes+=other.hashed_serialization_bytes;
    }
    fn within_budget(&self)->bool {
        self.scripts<=RETAIL_SCRIPTS && self.decoded_segments+self.compiler_segments<=RETAIL_MAPS
            && self.names+self.calls<=RETAIL_FACTS && self.occurrences<=RETAIL_OCCURRENCES
            && self.decoded_bytes<=RETAIL_DECODED && self.hashed_serialization_bytes<=HASH_TOTAL_LIMIT
    }
}

fn summarize_program(program:&XmlProgram,remaining_hash_bytes:usize)->Result<(RetailCounts,Value)> {
    let mut count=RetailCounts::default();
    count.profile_diagnostics=program.diagnostics.len();
    count.files_with_profile_diagnostics=usize::from(!program.diagnostics.is_empty());
    count.binding_files=usize::from(program.source_profile=="Bindings");
    count.binding_raw_bytes=if program.source_profile=="Bindings" {program.raw_bytes}else{0};
    count.binding_declarations=program.occurrences.iter().filter(|o|o.kind=="binding_declaration").count();
    count.modified_clicks=program.occurrences.iter().filter(|o|o.kind=="modified_click").count();
    count.binding_scripts=program.scripts.iter().filter(|s|s.kind=="binding").count();
    count.occurrences=program.occurrences.len(); count.scripts=program.scripts.len();
    count.load_edges=program.occurrences.iter().filter(|o|matches!(o.kind.as_str(),"include"|"script_file")).count();
    count.handler_registrations=program.occurrences.iter().filter(|o|o.kind=="handler_registration").count();
    count.includes=program.occurrences.iter().filter(|o|o.kind=="include").count();
    count.script_files=program.occurrences.iter().filter(|o|o.kind=="script_file").count();
    for proof in program.occurrences.iter().filter_map(|o|o.handler_catalog.as_ref()) {
        match proof["ownerResolution"].as_str() {
            Some("matched")=>count.handler_owners_matched+=1,
            Some("known_owner_mismatch")=>count.handler_owners_mismatched+=1,
            _=>count.handler_owners_unknown+=1,
        }
    }
    let mut kinds=BTreeMap::<String,usize>::new();
    for script in &program.scripts {
        *kinds.entry(script.tag.clone()).or_default()+=1;
        count.top_level+=usize::from(script.kind=="top_level"); count.handlers+=usize::from(script.kind=="handler");
        count.decoded_bytes+=script.decoded.text.len(); count.compiler_bytes+=script.compiler.text.len();
        count.decoded_segments+=script.decoded.segments.len(); count.compiler_segments+=script.compiler.segments.len();
        count.names+=script.syntax["names"].as_array().map_or(0,Vec::len);
        count.calls+=script.syntax["calls"].as_array().map_or(0,Vec::len);
        count.syntax_error_scripts+=usize::from(script.syntax["state"]!="parsed");
        count.syntax_errors+=script.syntax["errors"].as_array().map_or(0,Vec::len);
    }
    // Hash the complete deterministic serialization before dropping the program:
    // source text, both maps, every occurrence and every syntax fact are included.
    let (program_hash,serialized)=hash_value_limited(program,remaining_hash_bytes)
        .map_err(|error|if remaining_hash_bytes<HASH_FILE_LIMIT {"xml.remaining_program_budget".to_owned()}else{error})?;
    count.hashed_serialization_bytes=serialized;
    let (maps_hash,_)=hash_value(&program.scripts.iter().map(|s|(&s.id,&s.decoded,&s.compiler)).collect::<Vec<_>>())?;
    let (facts_hash,_)=hash_value(&program.scripts.iter().map(|s|(&s.id,&s.syntax)).collect::<Vec<_>>())?;
    let (occurrence_hash,_)=hash_value(&program.occurrences)?;
    let syntax_samples:Vec<_>=program.scripts.iter().filter(|s|s.syntax["state"]!="parsed").take(3).map(|s| {
        let errors:Vec<_>=s.syntax["errors"].as_array().into_iter().flatten().take(2).map(|e|json!({
            "message":e["message"].as_str().unwrap_or("").chars().take(512).collect::<String>(),
            "compilerBytes":e["bytes"],"rawCovering":e["sourceMap"]["covering"],"accuracy":e["sourceMap"]["accuracy"]})).collect();
        json!({"scriptId":s.id,"tag":s.tag,"element":s.element,"errors":errors})
    }).collect();
    let sampled_errors:usize=syntax_samples.iter().map(|s|s["errors"].as_array().map_or(0,Vec::len)).sum();
    let summary=json!({"sourceProfile":program.source_profile,"snapshotIdentity":program.snapshot_identity,"programSHA256":program_hash,
        "extractionComplete":program.coverage["extractionComplete"],
        "profileValidationPassed":program.coverage["profileValidationPassed"],
        "profileValidationState":program.coverage["profileValidationState"],
        "nativeRuntimeBehavior":"unknown","profileDiagnostics":program.diagnostics,
        "unexpectedProfileText":program.occurrences.iter().filter(|o|o.kind=="unexpected_profile_text").collect::<Vec<_>>(),
        "mapSHA256":maps_hash,"syntaxFactsSHA256":facts_hash,"occurrencesSHA256":occurrence_hash,
        "hashEncoding":"serde_json compact UTF-8; complete ordered values; no trailing newline",
        "scriptKinds":kinds,"counts":count,"syntaxErrorSamples":syntax_samples,
        "syntaxErrorSamplesTruncated":count.syntax_error_scripts>syntax_samples.len() || count.syntax_errors>sampled_errors});
    Ok((count,summary))
}

fn retail(manifest_path:&Path, source_root:&Path)->Result<Value> {
    let started=Instant::now();
    let deadline=started+Duration::from_secs(RETAIL_SECONDS);
    let manifest_bytes=read_bounded(manifest_path,4<<20)?;
    if hash(&manifest_bytes)!=MANIFEST_DIGEST { return err("xml.original_manifest_sha256_mismatch"); }
    let manifest:InputManifest=serde_json::from_slice(&manifest_bytes).map_err(|e|e.to_string())?;
    if manifest.schema!="lycheedev.source-memory-input.v1" || manifest.commit!=COMMIT
        || manifest.documents!=4036 || manifest.files.len()!=4036 || manifest.lua_files!=2551
        || manifest.lua_bytes!=23354176 || manifest.document_digest!=SOURCE_DIGEST {
        return err("xml.not_complete_fixed_retail_manifest");
    }
    if !source_root.is_absolute() { return err("xml.source_root_not_absolute"); }
    let root=source_root.canonicalize().map_err(|e|e.to_string())?;
    if !root.is_dir() { return err("xml.source_root_not_directory"); }
    let mut digest=Sha256::new();
    let mut seen=BTreeSet::new();
    let (mut document_bytes,mut lua_bytes,mut lua_files,mut xml_bytes,mut xml_files)=(0u64,0u64,0usize,0u64,0usize);
    // Verify ALL 4036 contents before extraction. XML is re-read and reverified
    // just before analysis; no file body survives this pass.
    for input in &manifest.files {
        if Instant::now()>=deadline { return err("xml.retail_deadline_during_input_verification"); }
        checked_relative(&input.path)?;
        if input.mode!="100644" || !seen.insert(input.path.to_lowercase()) || input.bytes>MAX_FILE as u64
            || input.sha256.len()!=64 || !input.sha256.bytes().all(|c|c.is_ascii_hexdigit()&&!c.is_ascii_uppercase()) {
            return err("xml.invalid_duplicate_or_oversized_manifest_record");
        }
        document_bytes=document_bytes.checked_add(input.bytes).ok_or("xml.document_byte_sum_overflow")?;
        if document_bytes>DOCUMENT_LIMIT { return err("xml.document_byte_budget"); }
        let _=verified_bytes(&root,input)?;
        digest.update(input.path.as_bytes());digest.update([0]);digest.update(input.sha256.as_bytes());digest.update([0]);digest.update(input.bytes.to_le_bytes());
        if input.path.to_ascii_lowercase().ends_with(".lua") { lua_files+=1;lua_bytes+=input.bytes; }
        if input.path.to_ascii_lowercase().ends_with(".xml") { xml_files+=1;xml_bytes+=input.bytes; }
    }
    if format!("{:x}",digest.finalize())!=SOURCE_DIGEST || lua_files!=2551 || lua_bytes!=23354176 || xml_files!=1091 {
        return err("xml.verified_document_set_mismatch");
    }
    let catalog=catalog()?;
    let mut total=RetailCounts::default();
    let mut files=Vec::with_capacity(xml_files);
    let mut failure_samples=Vec::new();
    let mut failures_by_reason=BTreeMap::<String,usize>::new();
    let (mut attempted,mut extracted,mut parsed,mut unsupported,mut failed,mut not_run)=(0usize,0usize,0usize,0usize,0usize,0usize);
    let mut stop_reason:Option<&str>=None;
    for input in manifest.files.iter().filter(|f|f.path.to_ascii_lowercase().ends_with(".xml")) {
        if Instant::now()>=deadline { stop_reason=Some("xml.retail_deadline"); }
        let outcome=if let Some(reason)=stop_reason { not_run+=1;Err(reason.to_owned()) }
            else {
                attempted+=1;
                let limits=ExtractionLimits { scripts:RETAIL_SCRIPTS.saturating_sub(total.scripts),
                    decoded:RETAIL_DECODED.saturating_sub(total.decoded_bytes),
                    maps:RETAIL_MAPS.saturating_sub(total.decoded_segments+total.compiler_segments),
                    occurrences:RETAIL_OCCURRENCES.saturating_sub(total.occurrences),
                    facts:RETAIL_FACTS.saturating_sub(total.names+total.calls) };
                verified_bytes(&root,input).and_then(|bytes|extract_with_deadline(&input.path,&bytes,&catalog,Some(deadline),&limits))
                    .and_then(|program|summarize_program(&program,HASH_TOTAL_LIMIT.saturating_sub(total.hashed_serialization_bytes)))
            };
        match outcome {
            Ok((count,summary))=>{
                extracted+=1;
                let syntax_ok=count.syntax_error_scripts==0;
                if syntax_ok { parsed+=1; }
                total.add(&count);
                let mut record=json!({"path":input.path,"rawBytes":input.bytes,"rawSHA256":input.sha256,
                    "state":if syntax_ok{"extracted"}else{"syntax_failed"},"summary":summary});
                if !syntax_ok {
                    failed+=1;
                    *failures_by_reason.entry("xml.emmy_syntax_errors".into()).or_default()+=1;
                    record["reason"]=json!("xml.emmy_syntax_errors");
                    if failure_samples.len()<FAILURE_SAMPLES { failure_samples.push(record.clone()); }
                }
                files.push(record);
                if !total.within_budget() { stop_reason=Some("xml.retail_aggregate_budget"); }
            }
            Err(error)=>{
                let reason=error.split(':').next().unwrap_or("xml.failure").to_owned();
                let is_unsupported=reason.starts_with("xml.unsupported_") || matches!(reason.as_str(),
                    "xml.prefixed_element_not_supported"|"xml.dtd_not_allowed"|"xml.processing_instruction_not_allowed");
                let state=if stop_reason.is_some() {"not_run"}else if is_unsupported {unsupported+=1;"unsupported"}else{failed+=1;"failed"};
                if reason=="xml.remaining_program_budget" { stop_reason=Some("xml.retail_aggregate_budget"); }
                *failures_by_reason.entry(reason.clone()).or_default()+=1;
                let record=json!({"path":input.path,"rawBytes":input.bytes,"rawSHA256":input.sha256,
                    "state":state,"reason":reason,"message":error.chars().take(512).collect::<String>(),"errorSHA256":hash(error.as_bytes()),
                    "messageTruncated":error.chars().count()>512,
                    "factsExported":false});
                if failure_samples.len()<FAILURE_SAMPLES { failure_samples.push(record.clone()); }
                files.push(record);
            }
        }
    }
    let (files_hash,_)=hash_value(&files)?;
    if Instant::now()>=deadline { stop_reason=Some("xml.retail_deadline"); }
    let complete=attempted==1091 && extracted==1091 && parsed==1091 && stop_reason.is_none();
    let mut report=json!({"schema":"lycheedev.emmy-xml-retail.v1",
        "state":if !complete{"failed"}else if total.profile_diagnostics>0{"extracted_with_profile_diagnostics"}else{"passed"},
        "extractionState":if complete{"passed"}else{"failed"},
        "profileValidationPassed":complete && total.profile_diagnostics==0,
        "profileValidationState":if total.profile_diagnostics>0{"unresolved"}else if complete{"passed_observed_extraction_profile"}else{"incomplete"},
        "profileValidationScope":"observed extraction profile only; native validation not performed",
        "nativeRuntimeBehavior":"unknown","xmlRuleRevision":XML_RULE_REVISION,
        "engine":ENGINE,"sourceCommit":COMMIT,"manifestSHA256":MANIFEST_DIGEST,"documentPathSHA256SizeDigest":SOURCE_DIGEST,
        "documentsVerified":4036,"documentBytesVerified":document_bytes,"luaFilesVerified":lua_files,"luaBytesVerified":lua_bytes});
    let coverage=json!({
        "xmlFilesExpected":1091,"xmlInputBytes":xml_bytes,"xmlFilesAttempted":attempted,"xmlFilesExtracted":extracted,
        "xmlFilesEmmyParsed":parsed,"xmlFilesUnsupported":unsupported,"xmlFilesFailed":failed,"xmlFilesNotRun":not_run,
        "xmlFilesExcluded":0,"extractionComplete":extracted==1091,"counts":total,"files":files,"filesSHA256":files_hash});
    let failures=json!({
        "failuresByReason":failures_by_reason,"firstFailure":failure_samples.first(),"failureSamples":failure_samples,
        "failureSampleLimit":FAILURE_SAMPLES,"failureSamplesTruncated":failed+unsupported+not_run>FAILURE_SAMPLES,
        "stopReason":stop_reason,"elapsedMillis":started.elapsed().as_millis()});
    let scope=json!({
        "supportedProfile":"Ui and observed Bindings; root text retained with unresolved profile diagnostics",
        "semanticComplete":false,"implicitParameters":"not_injected","lua51Compilation":"not_run",
        "loads":"ordered declarations only; targets never opened","execution":"never",
        "schemaCompanionSHA256":hash(XSD),"handlerCatalogTypes":SCRIPT_CATALOG_TYPES,
        "handlerOwnerResolution":"provenance only; no complete XSD validity claim"});
    let limits=json!({"documentInputBytes":DOCUMENT_LIMIT,
            "sourceReadBytesTwoPassMaximum":DOCUMENT_LIMIT*2,"fileInputBytes":MAX_FILE,"scriptsTotal":RETAIL_SCRIPTS,
            "decodedBytesTotal":RETAIL_DECODED,"mapSegmentsTotal":RETAIL_MAPS,"syntaxFactsTotal":RETAIL_FACTS,
            "occurrencesTotal":RETAIL_OCCURRENCES,"completeProgramHashBytesPerFile":HASH_FILE_LIMIT,
            "completeProgramHashBytesTotal":HASH_TOTAL_LIMIT,"outputBytes":MAX_OUTPUT,"cooperativeWallSeconds":RETAIL_SECONDS,
            "outerOwnerWallSecondsRequired":90,"oneProgramResidentAtATime":true});
    let fields=report.as_object_mut().expect("literal report object");
    for part in [coverage,failures,scope] {
        let Value::Object(part)=part else { unreachable!("literal report section object") };
        fields.extend(part);
    }
    fields.insert("limits".into(),limits);
    Ok(report)
}

fn run() -> Result<Value> {
    let args:Vec<_>=std::env::args_os().skip(1).collect();
    if args.len()==1 && args[0]=="golden" { return golden(); }
    if args.len()==3 && args[0]=="retail" {
        return retail(Path::new(&args[1]),Path::new(&args[2]));
    }
    if args.len()==4 && args[0]=="line" {
        let path=Path::new(&args[1]);
        let index=args[2].to_str().ok_or("xml.invalid_script_index")?.parse::<usize>().map_err(|_|"xml.invalid_script_index")?;
        let line=args[3].to_str().ok_or("xml.invalid_line_number")?.parse::<usize>().map_err(|_|"xml.invalid_line_number")?;
        let program=extract(&path.to_string_lossy(),&read_bounded(path,MAX_FILE as u64)?)?;
        let script=program.scripts.get(index).ok_or("xml.script_index_outside_program")?;
        return Ok(json!({"schema":"lycheedev.emmy-xml-line.v1","path":program.path,"rawSHA256":program.raw_sha256,"xmlRuleRevision":XML_RULE_REVISION,
            "scriptIndex":index,"scriptId":script.id,"compilerLine":line,"compilerInputSHA256":hash(script.compiler_input()),
            "mapping":script.compiler_line(line)?,"columnKnown":false,"semanticComplete":false}));
    }
    if args.len()==2 && args[0]=="extract" {
        let path=std::path::Path::new(&args[1]);
        let mut file=std::fs::File::open(path).map_err(|e|e.to_string())?;
        if !file.metadata().map_err(|e|e.to_string())?.is_file(){return err("xml.input_not_regular_file");}
        let mut bytes=Vec::new();
        (&mut file).take((MAX_FILE+1) as u64).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        return serde_json::to_value(extract(&path.to_string_lossy(),&bytes)?).map_err(|e|e.to_string());
    }
    err("usage: emmy_xml_program golden | extract <explicit-xml-file> | retail <absolute-manifest> <absolute-source-root> | line <absolute-file> <script-index> <compiler-line>")
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_OUTPUT {
            return Err(std::io::Error::other("xml.output_budget"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn main() {
    let (value,exit)=match run(){Ok(value)=>{let exit=i32::from(value["state"]=="failed");(value,exit)},
        Err(error)=>(json!({"schema":"lycheedev.emmy-xml-prototype.v1","state":"failed","error":error,"factsExported":false,"xmlRuleRevision":XML_RULE_REVISION}),1)};
    let mut bytes = BoundedOutput(Vec::new());
    match serde_json::to_writer(&mut bytes,&value) {
        Ok(())=>{
            let mut out=std::io::stdout().lock();
            if out.write_all(&bytes.0).and_then(|()|out.write_all(b"\n")).is_err(){std::process::exit(1);}
        }
        _=>{eprintln!("xml.output_budget_or_serialization; no complete report");std::process::exit(1);}
    }
    std::process::exit(exit);
}
