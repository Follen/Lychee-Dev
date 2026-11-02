//! Authored XML vectors exercise the production extractor directly. No probe
//! executor, network corpus loader or summary implementation is retained here.
use super::*;
const GOLDEN: &[u8] = include_bytes!("../../../../tests/fixtures/source-native/xml-golden-204.xml");
const ORDERED: &[u8] = include_bytes!("../../../../tests/fixtures/source-native/xml-ordered.xml");
const SPLIT: &[u8] = include_bytes!("../../../../tests/fixtures/source-native/xml-split.xml");

#[test]
fn original_bytes_occurrences_compiler_spans_and_line_mapping() -> Result<()> {
    let golden = extract_production("xml-golden-204.xml", GOLDEN)?;
    let ordered = extract_production("xml-ordered.xml", ORDERED)?;
    let split = extract_production("xml-split.xml", SPLIT)?;
    assert_eq!(json!(GOLDEN.len()), json!(204), "{}", "original_204_bytes");
    assert_eq!(
        json!(golden.scripts.iter().map(|s| &s.tag).collect::<Vec<_>>()),
        json!(["OnEvent", "PreClick"]),
        "{}",
        "golden_handler_set"
    );
    let run_spans: Vec<_> = golden
        .scripts
        .iter()
        .flat_map(|s| s.syntax["names"].as_array().into_iter().flatten())
        .filter(|name| name["name"] == "Run")
        .map(|name| name["sourceMap"]["covering"].clone())
        .collect();
    assert_eq!(
        json!(run_spans),
        json!([Span::new(92, 95), Span::new(123, 126), Span::new(159, 162)]),
        "{}",
        "original_three_run_ranges"
    );
    for (name, span) in [
        ("event", Span::new(96, 101)),
        ("button", Span::new(163, 169)),
    ] {
        let spans: Vec<_> = golden
            .scripts
            .iter()
            .flat_map(|s| s.syntax["names"].as_array().into_iter().flatten())
            .filter(|n| n["name"] == name)
            .map(|n| n["sourceMap"]["covering"].clone())
            .collect();
        assert_eq!(
            json!(spans),
            json!([span]),
            "{}",
            &format!("original_{name}_range")
        );
    }
    let entities: Vec<_> = golden.scripts[0]
        .decoded
        .segments
        .iter()
        .filter(|s| s.origin == Origin::Entity)
        .map(|s| s.raw)
        .collect();
    assert_eq!(
        json!(entities),
        json!([Span::new(65, 71), Span::new(75, 81), Span::new(82, 87)]),
        "{}",
        "entities_exact_raw_intervals"
    );
    assert_eq!(
        json!(golden.scripts[0].decoded.reverse(Span::new(83, 84))?.len()),
        json!(1),
        "{}",
        "entity_newline_reverse"
    );
    assert_eq!(
        json!(golden.scripts[0].decoded.segments.iter().any(|s| s.origin
            == Origin::NewlineNormalization
            && s.raw.is_some_and(|r| r.len() == 2))),
        json!(true),
        "{}",
        "crlf_normalized"
    );
    assert_eq!(
        json!(
            golden
                .scripts
                .iter()
                .all(|s| s.compiler.text.starts_with("return function(...)\n")
                    && !s.compiler.text.starts_with("return function(self"))
        ),
        json!(true),
        "{}",
        "no_parameter_invention"
    );
    assert_eq!(
        serde_json::to_value(golden.scripts[0].compiler.segments[0].origin)
            .map_err(|e| e.to_string())?,
        json!("Synthetic"),
        "{}",
        "synthetic_wrapper_origin"
    );
    let kinds: Vec<_> = ordered
        .occurrences
        .iter()
        .map(|o| o.kind.as_str())
        .collect();
    assert_eq!(
        json!(kinds),
        json!([
            "xml_element",
            "script_file",
            "include",
            "inline_script",
            "include",
            "object_declaration",
            "xml_element",
            "handler_registration",
            "handler_method_reference",
            "handler_registration",
            "handler_function_reference",
            "handler_registration"
        ]),
        "{}",
        "source_occurrence_order"
    );
    assert_eq!(
        json!(
            ordered
                .occurrences
                .iter()
                .filter(|o| o.kind == "include" && o.attributes["file"].value == "Child.xml")
                .count()
        ),
        json!(2),
        "{}",
        "duplicate_include_edges_retained"
    );
    assert_eq!(
        json!(ordered.scripts[0].compiler.text),
        json!("Root = 1"),
        "{}",
        "root_script_is_unwrapped"
    );
    assert_eq!(
        json!(ordered.scripts[1].tag),
        json!("PostClick"),
        "{}",
        "postclick_discovered"
    );
    let split_run: Vec<_> = split.scripts[0].syntax["names"]
        .as_array()
        .ok_or("missing syntax names")?
        .iter()
        .filter(|n| n["name"] == "Run")
        .collect();
    assert_eq!(
        json!(split_run.len()),
        json!(2),
        "{}",
        "split_identifiers_not_comment_or_string"
    );
    assert_eq!(
        json!(
            split_run
                .iter()
                .map(|n| n["sourceMap"]["accuracy"].clone())
                .collect::<Vec<_>>()
        ),
        json!(["covering", "covering"]),
        "{}",
        "entity_and_cdata_identifiers_covering"
    );
    assert_eq!(
        json!(split_run.iter().all(|n| {
            n["sourceMap"]["segments"]
                .as_array()
                .is_some_and(|s| s.len() > 1)
        })),
        json!(true),
        "{}",
        "split_identifiers_keep_multiple_segments"
    );
    for (name, raw) in [
        (
            "invalid_entity",
            include_bytes!("../../../../tests/fixtures/source-native/xml-invalid-entity.xml")
                .as_slice(),
        ),
        (
            "invalid_structure",
            include_bytes!("../../../../tests/fixtures/source-native/xml-invalid-structure.xml")
                .as_slice(),
        ),
        (
            "invalid_dtd",
            include_bytes!("../../../../tests/fixtures/source-native/xml-invalid-dtd.xml")
                .as_slice(),
        ),
    ] {
        assert_eq!(
            json!(extract_production(name, raw).is_err()),
            json!(true),
            "{}",
            name
        );
    }
    let bad = extract_production(
        "xml-invalid-lua.xml",
        include_bytes!("../../../../tests/fixtures/source-native/xml-invalid-lua.xml"),
    )?;
    assert_eq!(
        json!({
        "state":bad.scripts[0].syntax["state"],"names":bad.scripts[0].syntax["names"].as_array().map(Vec::len),
        "calls":bad.scripts[0].syntax["calls"].as_array().map(Vec::len)}),
        json!({"state":"invalid","names":0,"calls":0}),
        "{}",
        "invalid_lua_no_syntax_facts"
    );
    assert_eq!(
        crate::xml_inline::scripts(&golden, GOLDEN).unwrap()[0]
            .compiler_line(2)
            .unwrap()["columnKnown"]
            .clone(),
        json!(false),
        "{}",
        "luac_column_remains_unknown"
    );
    let newlines = extract_production(
        "xml-newline-contract.xml",
        include_bytes!("../../../../tests/fixtures/source-native/xml-newline-contract.xml"),
    )?;
    let compiled_newlines = crate::xml_inline::scripts(
        &newlines,
        include_bytes!("../../../../tests/fixtures/source-native/xml-newline-contract.xml"),
    )
    .unwrap();
    for (index, (label, newline, line, span)) in [
        ("bare_cr", "\r", 2, Span::new(28, 37)),
        ("bare_lf", "\n", 2, Span::new(70, 79)),
        ("paired_crlf", "\r\n", 2, Span::new(117, 126)),
        ("paired_lfcr", "\n\r", 2, Span::new(164, 173)),
        ("repeated_cr", "\r\r", 3, Span::new(211, 220)),
        ("repeated_lf", "\n\n", 3, Span::new(258, 267)),
    ]
    .into_iter()
    .enumerate()
    {
        let script = &newlines.scripts[index];
        let mapping = compiled_newlines[index].compiler_line(line).unwrap();
        assert_eq!(
            json!({"input":script.compiler.text,"rawCovering":mapping["sourceMap"]["covering"],
                "columnKnown":mapping["columnKnown"],"nextLineOutside":compiled_newlines[index].compiler_line(line+1).is_err()}),
            json!({"input":format!("local x = 1{newline}local = 2"),
            "rawCovering":span,"columnKnown":false,"nextLineOutside":true}),
            "{}",
            &format!("lua51_line_mapping_{label}")
        );
    }
    for (name, raw, line, span, input) in [
        (
            "top",
            include_bytes!("../../../../tests/fixtures/source-native/xml-entity-cr-top.xml")
                .as_slice(),
            2,
            Span::new(28, 37),
            "local x = 1\rlocal = 2",
        ),
        (
            "handler",
            include_bytes!("../../../../tests/fixtures/source-native/xml-entity-cr-handler.xml")
                .as_slice(),
            3,
            Span::new(44, 53),
            "return function(...)\nlocal x = 1\rlocal = 2\nend\n",
        ),
    ] {
        let source = extract_production(name, raw)?;
        let script = &source.scripts[0];
        let mapping = crate::xml_inline::scripts(&source, raw).unwrap()[0]
            .compiler_line(line)
            .unwrap();
        assert_eq!(
            json!({"input":script.compiler.text,"rawCovering":mapping["sourceMap"]["covering"],
                "columnKnown":mapping["columnKnown"]}),
            json!({"input":input,"rawCovering":span,"columnKnown":false}),
            "{}",
            &format!("actual_cr_red_regression_{name}")
        );
    }
    let empty_control =
        include_bytes!("../../../../tests/fixtures/source-native/xml-empty-control.xml").as_slice();
    let empty_bom =
        include_bytes!("../../../../tests/fixtures/source-native/xml-empty-bom.xml").as_slice();
    assert_eq!(
        json!(empty_bom.starts_with(b"\xef\xbb\xbf") && empty_bom.get(3..) == Some(empty_control)),
        json!(true),
        "{}",
        "bom_empty_fixture_exact_prefix"
    );
    for (label, raw, prefix) in [
        ("control", empty_control, 0usize),
        ("bom", empty_bom, 3usize),
    ] {
        let program = extract_production(label, raw)?;
        let shifted = |start, end| Span::new(start + prefix, end + prefix);
        let expected_ranges: Vec<_> = [
            (0, 4),
            (4, 13),
            (13, 35),
            (35, 58),
            (58, 78),
            (78, 97),
            (97, 128),
            (128, 137),
            (137, 145),
            (182, 203),
            (203, 211),
        ]
        .into_iter()
        .map(|(start, end)| shifted(start, end))
        .collect();
        assert_eq!(
            json!(
                program
                    .occurrences
                    .iter()
                    .map(|o| o.raw)
                    .collect::<Vec<_>>()
            ),
            json!(expected_ranges),
            "{}",
            &format!("empty_elements_{label}_original_open_ranges")
        );
        assert_eq!(
            json!(
                program
                    .occurrences
                    .iter()
                    .map(|o| o.parent)
                    .collect::<Vec<_>>()
            ),
            json!([null, 0, 0, 0, 0, 4, 4, 4, 7, 0, 0]),
            "{}",
            &format!("empty_elements_{label}_ancestry")
        );
        assert_eq!(
            json!(
                program
                    .scripts
                    .iter()
                    .map(|s| json!({"tag":s.tag,"element":s.element,
                "decoded":s.decoded.text,"compiler":s.compiler.text}))
                    .collect::<Vec<_>>()
            ),
            json!([
            {"tag":"OnLoad","element":shifted(137,164),"decoded":"self:Run()","compiler":"return function(...)\nself:Run()\nend\n"},
            {"tag":"Script","element":shifted(203,228),"decoded":"Root = 1","compiler":"Root = 1"}]),
            "{}",
            &format!("empty_elements_{label}_whole_script_ranges_and_text")
        );
        let script = &program.scripts[0];
        let run = script.syntax["names"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|n| n["name"] == "Run");
        assert_eq!(
            json!({
            "prefix":program.coverage["rawCursorPrefixBytes"],"decodedRaw":script.decoded.segments[0].raw,
            "run":run.map(|n|n["sourceMap"]["covering"].clone()),
            "wrapperStart":script.compiler.segments.first().and_then(|s|s.anchor),
            "wrapperEnd":script.compiler.segments.last().and_then(|s|s.anchor)}),
            json!({
            "prefix":prefix,"decodedRaw":shifted(145,155),"run":shifted(150,153),
            "wrapperStart":shifted(137,137),"wrapperEnd":shifted(155,164)}),
            "{}",
            &format!("empty_elements_{label}_body_and_synthetic_anchors")
        );
    }
    Ok(())
}

#[test]
fn handler_catalog_ancestry_and_owner_mismatch() -> Result<()> {
    let golden = extract_production("xml-golden-204.xml", GOLDEN)?;
    let catalog_fixture = extract_production(
        "xml-handler-catalog.xml",
        include_bytes!("../../../../tests/fixtures/source-native/xml-handler-catalog.xml"),
    )?;
    let class_counts: BTreeMap<_, _> = catalog_fixture.coverage["handlerCatalogDeclarations"]
        .as_object()
        .ok_or("missing handler catalog")?
        .iter()
        .map(|(name, handlers)| (name, handlers.as_object().map_or(0, serde_json::Map::len)))
        .collect();
    assert_eq!(
        json!(class_counts),
        json!({"ScriptsType":75,"ActorScriptsType":6,"AnimScriptsType":6,"AnimGroupScriptsType":7}),
        "{}",
        "catalog_four_schema_classes"
    );
    assert_eq!(
        json!(
            catalog_fixture
                .scripts
                .iter()
                .map(|s| json!({"tag":s.tag,"element":s.element,"text":s.decoded.text}))
                .collect::<Vec<_>>()
        ),
        json!([
        {"tag":"OnModelLoading","element":Span::new(20,65),"text":"self:Ready()"},
        {"tag":"OnLoop","element":Span::new(108,136),"text":"self:Loop()"},
        {"tag":"OnPlay","element":Span::new(172,200),"text":"self:Play()"},
        {"tag":"OnPause","element":Span::new(200,231),"text":"self:Pause()"},
        {"tag":"OnStop","element":Span::new(231,259),"text":"self:Stop()"},
        {"tag":"OnFinished","element":Span::new(259,297),"text":"self:Finish()"}]),
        "{}",
        "catalog_six_missing_handler_exact_sources"
    );
    for (tag, owner, owner_type, class, provider, path, declaration) in [
        (
            "OnModelLoading",
            "Actor",
            "ModelSceneActorType",
            "ActorScriptsType",
            "ModelSceneActorType",
            vec!["ModelSceneActorType"],
            Span::new(49236, 49289),
        ),
        (
            "OnLoop",
            "AnimationGroup",
            "AnimationGroupType",
            "AnimGroupScriptsType",
            "AnimationGroupType",
            vec!["AnimationGroupType"],
            Span::new(61227, 61272),
        ),
        (
            "OnPlay",
            "LineTranslation",
            "LineTranslationType",
            "AnimScriptsType",
            "AnimationType",
            vec!["LineTranslationType", "TranslationType", "AnimationType"],
            Span::new(60511, 60556),
        ),
        (
            "OnPause",
            "LineTranslation",
            "LineTranslationType",
            "AnimScriptsType",
            "AnimationType",
            vec!["LineTranslationType", "TranslationType", "AnimationType"],
            Span::new(60561, 60607),
        ),
        (
            "OnStop",
            "LineTranslation",
            "LineTranslationType",
            "AnimScriptsType",
            "AnimationType",
            vec!["LineTranslationType", "TranslationType", "AnimationType"],
            Span::new(60612, 60657),
        ),
        (
            "OnFinished",
            "LineTranslation",
            "LineTranslationType",
            "AnimScriptsType",
            "AnimationType",
            vec!["LineTranslationType", "TranslationType", "AnimationType"],
            Span::new(60714, 60763),
        ),
    ] {
        let proof = catalog_fixture
            .occurrences
            .iter()
            .find(|o| o.tag == tag && o.kind == "handler_registration")
            .and_then(|o| o.handler_catalog.as_ref())
            .ok_or("missing handler provenance")?;
        assert_eq!(
            json!({"owner":proof["ownerElement"],"ownerType":proof["ownerType"],
                "catalogType":proof["scriptContainer"]["catalogType"],"provider":proof["scriptContainer"]["declaredByType"],
                "inheritance":proof["inheritance"].as_array().into_iter().flatten().map(|step|step["type"].clone()).collect::<Vec<_>>(),
                "declaration":proof["matchedDeclaration"],"resolution":proof["ownerResolution"]}),
            json!({"owner":owner,"ownerType":owner_type,
            "catalogType":class,"provider":provider,"inheritance":path,"declaration":declaration,"resolution":"matched"}),
            "{}",
            &format!("catalog_owner_provenance_{tag}")
        );
    }
    let frame_proof = golden
        .occurrences
        .iter()
        .find(|o| o.tag == "OnEvent" && o.kind == "handler_registration")
        .and_then(|o| o.handler_catalog.as_ref())
        .ok_or("missing frame handler provenance")?;
    assert_eq!(
        json!({"ownerType":frame_proof["ownerType"],"catalogType":frame_proof["scriptContainer"]["catalogType"],
            "provider":frame_proof["scriptContainer"]["declaredByType"],"via":frame_proof["scriptContainer"]["via"],
            "path":frame_proof["scriptContainer"]["substitutionPath"],"resolution":frame_proof["ownerResolution"]}),
        json!({"ownerType":"FrameType","catalogType":"ScriptsType",
        "provider":"FrameType","via":"element_reference_substitution","path":["Scripts","LayoutField"],"resolution":"matched"}),
        "{}",
        "catalog_frame_scripts_substitution_provenance"
    );
    assert_eq!(
        json!(
            catalog_fixture.scripts.iter().all(|s| s
                .compiler
                .text
                .starts_with("return function(...)\n")
                && s.implicit_parameters.starts_with("not_injected"))
        ),
        json!(true),
        "{}",
        "catalog_implicit_parameters_still_not_injected"
    );
    let unknown = extract_production(
        "unknown-handler",
        b"<Ui><AnimationGroup><Scripts><OnInvented/></Scripts></AnimationGroup></Ui>",
    )?;
    let occurrence = unknown
        .occurrences
        .iter()
        .find(|o| o.tag == "OnInvented")
        .unwrap();
    assert_eq!(occurrence.kind, "handler_registration");
    let provenance = occurrence.handler_catalog.as_ref().unwrap();
    assert_eq!(provenance["ownerResolution"], "known_owner_mismatch");
    assert!(provenance["matchedDeclaration"].is_null());
    assert_eq!(provenance["handlerDeclarations"], json!([]));
    let mismatch = extract_production(
        "owner-mismatch",
        b"<Ui><Frame><Scripts><OnLoop/></Scripts></Frame></Ui>",
    )?;
    let mismatch_proof = mismatch
        .occurrences
        .iter()
        .find_map(|o| o.handler_catalog.as_ref())
        .ok_or("missing mismatch provenance")?;
    assert_eq!(
        mismatch_proof["ownerResolution"].clone(),
        json!("known_owner_mismatch"),
        "{}",
        "catalog_owner_mismatch_is_explicit"
    );
    Ok(())
}

#[test]
fn xml_lexical_names_cdata_entities_and_outside_whitespace() -> Result<()> {
    // Keep the exact six independently observed malformed inputs unchanged.
    for (name, input) in [
        ("attribute_raw_lt", "<Ui a=\"<\"/>"),
        ("text_cdata_close", "<Ui>]]></Ui>"),
        ("tail_bom", "<Ui/>\u{feff}"),
        ("tail_nbsp", "<Ui/>\u{a0}"),
        ("invalid_element_name", "<Ui><1bad/></Ui>"),
        ("invalid_attribute_name", "<Ui 1bad=\"x\"/>"),
    ] {
        assert_eq!(
            json!(extract_production(name, input.as_bytes()).is_err()),
            json!(true),
            "{}",
            &format!("xml10_actual_negative_{name}")
        );
    }
    let lexical = extract_production(
        "xml-lexical-valid.xml",
        include_bytes!("../../../../tests/fixtures/source-native/xml-lexical-valid.xml"),
    )?;
    assert_eq!(
        json!({"scripts":lexical.scripts.len(),"text":lexical.scripts[0].compiler.text,
            "parsed":lexical.scripts[0].syntax["state"]=="parsed"}),
        json!({"scripts":1,
        "text":r#"local a = 1 < 2; local s = "]]>";local t = "]]>";"#,"parsed":true}),
        "{}",
        "xml10_legal_cdata_comments_escaped_close_and_spaces"
    );
    assert_eq!(
        json!({"tags":lexical.occurrences.iter().map(|o|&o.tag).collect::<Vec<_>>(),
            "namespace":lexical.occurrences[0].attributes["xmlns"].value,
            "namespaceAttribute":lexical.occurrences[0].attributes["xmlns:xsi"].value,
            "schemaLocation":lexical.occurrences[0].attributes["xsi:noNamespaceSchemaLocation"].value,
            "escapedLt":lexical.occurrences[0].attributes["窗口"].value,
            "middleDotName":lexical.occurrences[0].attributes["a·b"].value,
            "supplementaryName":lexical.occurrences[1].attributes["\u{10000}"].value}),
        json!({
        "tags":["Ui","Élément_名-0.\u{300}\u{203f}","\u{10000}","Script"],
        "namespace":"http://www.blizzard.com/wow/ui/","namespaceAttribute":"http://www.w3.org/2001/XMLSchema-instance",
        "schemaLocation":"UI.xsd","escapedLt":"<","middleDotName":"ok","supplementaryName":"v"}),
        "{}",
        "xml10_legal_unicode_names_and_namespace_attributes"
    );
    assert_eq!(
        json!({"segments":lexical.scripts[0].decoded.segments.len(),
            "entitySegments":lexical.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::Entity).count(),
            "accuracy":lexical.scripts[0].decoded.project(Span::new(0,lexical.scripts[0].decoded.text.len()))?["accuracy"]}),
        json!({"segments":5,"entitySegments":1,"accuracy":"covering"}),
        "{}",
        "xml10_legal_segmented_cdata_and_entity_map"
    );
    let valid_names = [
        "_a",
        "a-0.",
        "É",
        "名",
        "a\u{b7}",
        "a\u{300}",
        "a\u{203f}",
        "\u{200c}",
        "\u{10000}",
        "\u{effff}",
    ];
    assert_eq!(
        json!(
            valid_names
                .iter()
                .map(|name| extract_production(
                    "valid-name",
                    format!("<Ui><{name} {name}=\"v\"/></Ui>").as_bytes()
                )
                .is_ok())
                .collect::<Vec<_>>()
        ),
        json!(vec![true; valid_names.len()]),
        "{}",
        "xml10_unicode_name_start_and_continuation_acceptance"
    );
    let invalid_names = [
        "",
        "1bad",
        "-bad",
        ".bad",
        "\u{300}bad",
        "\u{b7}bad",
        "\u{37e}bad",
        "bad\u{d7}",
        "bad\u{200b}",
        "bad\u{fdd0}",
        "bad\u{f0000}",
        "bad?",
        "bad<",
    ];
    assert_eq!(
        json!(
            invalid_names
                .iter()
                .map(|name| xml_name(name))
                .collect::<Vec<_>>()
        ),
        json!(vec![false; invalid_names.len()]),
        "{}",
        "xml10_name_range_gaps_and_invalid_starts"
    );
    assert_eq!(
        json!(
            extract_production("leading-bom-whitespace", b"\xef\xbb\xbf \t\r\n<Ui/> \t\r\n")
                .is_ok()
        ),
        json!(true),
        "{}",
        "xml10_leading_bom_and_all_four_outside_spaces"
    );
    assert_eq!(
        json!(
            extract_production("misplaced-declaration", b" <?xml version=\"1.0\"?><Ui/>").is_err()
        ),
        json!(true),
        "{}",
        "xml10_misplaced_declaration_rejected"
    );
    Ok(())
}

#[test]
fn bindings_keep_order_metadata_source_ranges_and_unresolved_root_text() -> Result<()> {
    let binding_raw =
        include_bytes!("../../../../tests/fixtures/source-native/xml-bindings-394.xml");
    let bindings = extract_production("xml-bindings-394.xml", binding_raw)?;
    assert_eq!(
        json!({"bytes":binding_raw.len(),"sha256":hash(binding_raw)}),
        json!({"bytes":394,"sha256":"aca963840dc4e0201609be4443c35f4985199d72556500c95e6957db18c16e96"}),
        "{}",
        "bindings_exact_fixture"
    );
    assert_eq!(
        json!({"profile":bindings.source_profile,"scripts":bindings.scripts.len(),
            "kinds":bindings.occurrences.iter().map(|o|&o.kind).collect::<Vec<_>>(),
            "ranges":bindings.occurrences.iter().map(|o|o.raw).collect::<Vec<_>>()}),
        json!({"profile":"Bindings","scripts":2,
        "kinds":["bindings_root","binding_declaration","modified_click","binding_declaration","binding_declaration"],
        "ranges":[Span::new(0,394),Span::new(12,149),Span::new(198,246),Span::new(248,312),Span::new(314,381)]}),
        "{}",
        "bindings_profile_counts_and_order"
    );
    assert_eq!(
        json!({"names":bindings.occurrences.iter().filter_map(|o|o.attributes.get("name").map(|a|&a.value)).collect::<Vec<_>>(),
            "emptyScript":bindings.occurrences[3].script,"context":bindings.occurrences[3].attributes["bindingContext"].value,
            "runOnUp":bindings.occurrences[1].attributes["runOnUp"].value,"header":bindings.occurrences[1].attributes["header"].value,
            "category":bindings.occurrences[1].attributes["category"].value}),
        json!({"names":["A&B","Empty","A&B"],"emptyScript":null,
        "context":"HousingEditorLayoutMode","runOnUp":"true","header":"H","category":"C"}),
        "{}",
        "bindings_duplicate_names_and_empty_metadata"
    );
    assert_eq!(
        json!({"action":bindings.occurrences[2].attributes["action"].value,"default":bindings.occurrences[2].attributes["default"].value,
            "script":bindings.occurrences[2].script}),
        json!({"action":"SELFCAST","default":"ALT","script":null}),
        "{}",
        "bindings_modified_click_is_metadata"
    );
    assert_eq!(
        json!({"text":bindings.scripts.iter().map(|s|&s.decoded.text).collect::<Vec<_>>(),
            "bodies":bindings.scripts.iter().map(|s|s.binding_body_raw).collect::<Vec<_>>(),
            "elements":bindings.scripts.iter().map(|s|s.element).collect::<Vec<_>>()}),
        json!({"text":["Run(keystate)\nRun(keystate)\rreturn","local keystate=\"up\";Run(keystate)"],
        "bodies":[Span::new(75,139),Span::new(338,371)],"elements":[Span::new(12,149),Span::new(314,381)]}),
        "{}",
        "bindings_decoded_bodies_and_raw_spans"
    );
    let binding_runs: Vec<_> = bindings
        .scripts
        .iter()
        .flat_map(|s| s.syntax["names"].as_array().into_iter().flatten())
        .filter(|n| n["name"] == "Run")
        .map(|n| n["sourceMap"].clone())
        .collect();
    assert_eq!(
        json!(
            binding_runs
                .iter()
                .map(|m| &m["covering"])
                .collect::<Vec<_>>()
        ),
        json!([Span::new(75, 92), Span::new(107, 118), Span::new(358, 361)]),
        "{}",
        "bindings_run_token_covering_ranges"
    );
    assert_eq!(
        json!(
            binding_runs
                .iter()
                .map(|m| m["segments"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|s| s["raw"].clone())
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>()
        ),
        json!([
            [Span::new(75, 76), Span::new(76, 82), Span::new(91, 92)],
            [Span::new(107, 108), Span::new(116, 118)],
            [Span::new(358, 361)]
        ]),
        "{}",
        "bindings_run_token_exact_segment_ranges"
    );
    assert_eq!(
        json!({
        "origins":binding_runs.iter().map(|m|m["segments"].as_array().into_iter().flatten().map(|s|s["origin"].clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "accuracy":binding_runs.iter().map(|m|&m["accuracy"]).collect::<Vec<_>>()}),
        json!({"origins":[["Exact","Entity","Exact"],["Exact","Exact"],["Exact"]],
        "accuracy":["covering","covering","exact"]}),
        "{}",
        "bindings_run_token_origins_and_accuracy"
    );
    assert_eq!(
        json!({
        "physical":bindings.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::NewlineNormalization).map(|s|s.raw).collect::<Vec<_>>(),
        "entityCR":bindings.scripts[0].decoded.segments.iter().filter(|s|s.origin==Origin::Entity && &bindings.scripts[0].decoded.text[s.output.start..s.output.end]=="\r").map(|s|s.raw).collect::<Vec<_>>()}),
        json!({"physical":[Span::new(105,107)],"entityCR":[Span::new(128,133)]}),
        "{}",
        "bindings_newline_origins"
    );
    assert_eq!(
        json!(bindings.scripts[0].decoded.reverse(Span::new(108, 116))?),
        json!([]),
        "{}",
        "bindings_comment_gap_maps_to_no_output"
    );
    assert_eq!(
        json!(
            bindings
                .scripts
                .iter()
                .flat_map(|s| s.syntax["names"].as_array().into_iter().flatten())
                .filter(|n| n["name"] == "keystate")
                .map(|n| n["sourceMap"]["covering"].clone())
                .collect::<Vec<_>>()
        ),
        json!([
            Span::new(96, 104),
            Span::new(119, 127),
            Span::new(344, 352),
            Span::new(362, 370)
        ]),
        "{}",
        "bindings_keystate_tokens_include_unresolved_local_shadow"
    );
    assert_eq!(
        json!(
            bindings
                .scripts
                .iter()
                .flat_map(|s| s.syntax["names"].as_array().into_iter().flatten())
                .all(|n| !matches!(n["name"].as_str(), Some("FAKE" | "Ghost" | "A&B" | "Empty")))
        ),
        json!(true),
        "{}",
        "bindings_comment_fake_and_binding_identifiers_are_not_lua_names"
    );
    assert_eq!(
        json!(bindings.scripts.iter().all(|s| s.kind == "binding"
            && s.compiler.text == format!("return function(...)\n{}\nend\n", s.decoded.text)
            && s.syntax["state"] == "parsed"
            && s.implicit_parameters.starts_with("not_injected"))),
        json!(true),
        "{}",
        "bindings_wrapper_without_signature_invention"
    );
    assert_eq!(
        json!({
        "semanticComplete":bindings.coverage["semanticComplete"],"schema":bindings.coverage["schemaValidation"],
        "bindingFacts":bindings.scripts.iter().map(|s|&s.syntax["bindingFactsExported"]).collect::<Vec<_>>()}),
        json!({"semanticComplete":false,
        "schema":"not_performed; UI.xsd is not a Bindings schema","bindingFacts":[false,false]}),
        "{}",
        "bindings_no_semantic_or_schema_claim"
    );
    let metadata=extract_production("binding-metadata",b"<Bindings xmlns=\"urn:test\" custom=\"root\"><Binding name=\"Open Issue Report\" default=\"F6\" hidden=\"true\"></Binding></Bindings>")?;
    assert_eq!(
        json!({"root":metadata.occurrences[0].attributes["custom"].value,
        "namespace":metadata.occurrences[0].attributes["xmlns"].value,"name":metadata.occurrences[1].attributes["name"].value,
        "default":metadata.occurrences[1].attributes["default"].value,"hidden":metadata.occurrences[1].attributes["hidden"].value,
        "scripts":metadata.scripts.len()}),
        json!({"root":"root","namespace":"urn:test","name":"Open Issue Report",
        "default":"F6","hidden":"true","scripts":0}),
        "{}",
        "bindings_root_unknown_attributes_space_name_paired_empty"
    );
    for (name, input) in [
        ("unknown_direct_child", "<Bindings><Unknown/></Bindings>"),
        (
            "nested_binding_markup",
            "<Bindings><Binding name=\"A\"><Frame/></Binding></Bindings>",
        ),
        (
            "nested_modified_click",
            "<Bindings><ModifiedClick><Binding/></ModifiedClick></Bindings>",
        ),
        (
            "modified_click_body",
            "<Bindings><ModifiedClick>Run()</ModifiedClick></Bindings>",
        ),
    ] {
        assert_eq!(
            json!(extract_production(name, input.as_bytes()).is_err()),
            json!(true),
            "{}",
            &format!("bindings_reject_{name}")
        );
    }
    let root_text = extract_production("root_executable_text", b"<Bindings>Run()</Bindings>")?;
    assert_eq!(
        json!({
        "text":root_text.occurrences[1].source_text,"span":root_text.occurrences[1].raw,"scripts":root_text.scripts.len(),
        "extractionComplete":root_text.coverage["extractionComplete"],"profileValidationPassed":root_text.coverage["profileValidationPassed"],
        "diagnostics":root_text.diagnostics}),
        json!({"text":"Run()","span":Span::new(10,15),
        "scripts":0,"extractionComplete":true,"profileValidationPassed":false,"diagnostics":[{
            "code":"binding_root_text","severity":"warning","occurrence":1,"raw":Span::new(10,15),
            "validationState":"unresolved","nativeRuntimeBehavior":"unknown"}]}),
        "{}",
        "bindings_root_executable_text_retained_with_invalid_profile"
    );
    let root_text_raw =
        include_bytes!("../../../../tests/fixtures/source-native/xml-bindings-root-text.xml");
    let root_text = extract_production("xml-bindings-root-text.xml", root_text_raw)?;
    assert_eq!(
        json!({"bytes":root_text_raw.len(),"sha256":hash(root_text_raw)}),
        json!({"bytes":90,"sha256":"a565f2446b7a976fd8f42b6acdc6e56808c7b2dc4d80a6d8a74f10a68e0cee5d"}),
        "{}",
        "bindings_root_text_fixture_exact_bytes"
    );
    assert_eq!(
        json!({
        "kinds":root_text.occurrences.iter().map(|o|&o.kind).collect::<Vec<_>>(),
        "ranges":root_text.occurrences.iter().map(|o|o.raw).collect::<Vec<_>>(),
        "names":root_text.occurrences.iter().filter_map(|o|o.attributes.get("name").map(|a|&a.value)).collect::<Vec<_>>()}),
        json!({
        "kinds":["bindings_root","binding_declaration","unexpected_profile_text","binding_declaration"],
        "ranges":[Span::new(0,90),Span::new(10,43),Span::new(43,45),Span::new(45,79)],"names":["A","B"]}),
        "{}",
        "bindings_root_text_retains_both_declarations_in_order"
    );
    assert_eq!(
        json!({"text":root_text.occurrences[2].source_text,"raw":root_text.occurrences[2].raw,"tag":root_text.occurrences[2].tag,
            "parent":root_text.occurrences[2].parent,"script":root_text.occurrences[2].script}),
        json!({"text":">\n","raw":Span::new(43,45),"tag":"#text","parent":0,"script":null}),
        "{}",
        "bindings_root_text_preserves_raw_event_and_parent"
    );
    assert_eq!(
        json!(root_text.diagnostics),
        json!([{"code":"binding_root_text","severity":"warning",
        "occurrence":2,"raw":Span::new(43,44),"validationState":"unresolved","nativeRuntimeBehavior":"unknown"}]),
        "{}",
        "bindings_root_text_diagnostic_nonspace_subrange"
    );
    assert_eq!(
        json!({
        "bodies":root_text.scripts.iter().map(|s|&s.decoded.text).collect::<Vec<_>>(),
        "ranges":root_text.scripts.iter().map(|s|s.binding_body_raw).collect::<Vec<_>>(),
        "names":root_text.scripts.iter().flat_map(|s|s.syntax["names"].as_array().into_iter().flatten()).map(|n|&n["name"]).collect::<Vec<_>>(),
        "calls":root_text.scripts.iter().map(|s|s.syntax["calls"].as_array().map_or(0,Vec::len)).sum::<usize>()}),
        json!({"bodies":["Run()","Next()"],
        "ranges":[Span::new(28,33),Span::new(63,69)],"names":["Run","Next"],"calls":2}),
        "{}",
        "bindings_root_text_keeps_only_binding_bodies"
    );

    assert_eq!(
        json!({"passed":bindings.coverage["profileValidationPassed"],"diagnostics":bindings.diagnostics}),
        json!({"passed":true,"diagnostics":[]}),
        "{}",
        "bindings_clean_fixture_has_no_profile_diagnostics"
    );
    Ok(())
}

#[test]
fn declarations_and_attributes_accept_only_complete_xml_framing() -> Result<()> {
    // Preserve the six actual failing source strings from the independent runner.
    for (name, input) in [
        ("attribute_missing_separator", r#"<Ui a="1"b="2"/>"#),
        (
            "declaration_standalone_invalid",
            r#"<?xml version="1.0" standalone="maybe"?><Ui/>"#,
        ),
        (
            "declaration_unknown_field",
            r#"<?xml version="1.0" other="x"?><Ui/>"#,
        ),
        (
            "declaration_duplicate_version",
            r#"<?xml version="1.0" version="1.0"?><Ui/>"#,
        ),
        (
            "declaration_field_order",
            r#"<?xml version="1.0" standalone="yes" encoding="UTF-8"?><Ui/>"#,
        ),
        (
            "declaration_invalid_tail",
            r#"<?xml version="1.0" encoding="UTF-8" broken?><Ui/>"#,
        ),
    ] {
        assert_eq!(
            json!(extract_production(name, input.as_bytes()).is_err()),
            json!(true),
            "{}",
            &format!("xml10_actual_declaration_negative_{name}")
        );
    }
    let legal_declarations = [
        "<?xml version=\"1.0\"?><Ui/>",
        "<?xml version='1.0' ?><Ui/>",
        "<?xml version='1.0' encoding='UTF-8'?><Ui/>",
        "<?xml version='1.0' standalone='yes'?><Ui/>",
        "<?xml version='1.0' encoding='utf-8' standalone='no'?><Ui/>",
        "\u{feff}<?xml\tversion\n=\r'1.0' \tencoding \r= \n'UtF-8' \rstandalone \t= \n'yes'\t?><Ui/>",
    ];
    assert_eq!(
        json!(
            legal_declarations
                .iter()
                .map(|input| extract_production("legal-declaration", input.as_bytes()).is_ok())
                .collect::<Vec<_>>()
        ),
        json!(vec![true; legal_declarations.len()]),
        "{}",
        "xml10_legal_complete_declaration_variants"
    );
    let attribute_separators = [' ', '\t', '\r', '\n'];
    assert_eq!(
        json!(
            attribute_separators
                .iter()
                .map(|separator| extract_production(
                    "legal-attribute-separator",
                    format!("<Ui a='甲'{separator}b = \"乙\"/>").as_bytes()
                )
                .is_ok_and(|program| program.occurrences[0].attributes["b"].value == "乙"))
                .collect::<Vec<_>>()
        ),
        json!([true, true, true, true]),
        "{}",
        "xml10_element_attributes_accept_each_xml_s_separator"
    );
    let declaration_raw =
        include_bytes!("../../../../tests/fixtures/source-native/xml-declaration-valid.xml");
    let declaration = extract_production("xml-declaration-valid.xml", declaration_raw)?;
    assert_eq!(
        json!({
        "bytes":declaration_raw.len(),"sha256":hash(declaration_raw),"prefix":declaration.coverage["rawCursorPrefixBytes"],
        "whitespace":declaration.occurrences[0].attributes["空白"].value,"name":declaration.occurrences[0].attributes["name"].value,
        "empty":declaration.occurrences[0].attributes["empty"].value,"body":declaration.scripts[0].decoded.text}),
        json!({"bytes":156,
        "sha256":"59b1714957341a5da89ffb260556b08cb758036bfe6f1247d8dd2d6a66a29144",
        "prefix":3,"whitespace":"甲 乙 丙 丁 戊","name":"A&B","empty":"","body":"Run()"}),
        "{}",
        "xml10_legal_declaration_unicode_attribute_fixture"
    );
    assert_eq!(
        json!(
            declaration.scripts[0].syntax["names"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|n| n["name"] == "Run")
                .map(|n| n["sourceMap"]["covering"].clone())
                .collect::<Vec<_>>()
        ),
        json!([Span::new(137, 140)]),
        "{}",
        "xml10_legal_declaration_preserves_original_body_range"
    );
    let invalid_declarations = [
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
    assert_eq!(
        json!(
            invalid_declarations
                .iter()
                .map(|input| extract_production(
                    "invalid-declaration-or-attribute",
                    input.as_bytes()
                )
                .is_err())
                .collect::<Vec<_>>()
        ),
        json!(vec![true; invalid_declarations.len()]),
        "{}",
        "xml10_declaration_complete_tail_and_attribute_framing_negatives"
    );
    assert_eq!(
        json!(
            extract_production(
                "late-invalid-attribute",
                b"<Ui><Script>Run()</Script><Frame a=\"1\"b=\"2\"/></Ui>"
            )
            .is_err()
        ),
        json!(true),
        "{}",
        "xml10_late_attribute_failure_is_transactional"
    );
    Ok(())
}
