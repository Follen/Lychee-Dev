//! Fixed Go 3.2.3 check/usage/compat contract regressions. These tests do not
//! replace the real luac/native XML integration suites.
use super::*;
use crate::{
    closure::Document,
    facts::{Facts, Relationship},
    index::Index,
};
use usage::Usage;

fn pin() -> SourcePin {
    SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: "fixture".into(),
        exact_commit: "a".repeat(40),
        parser_revision: "source-v1".into(),
    }
}
fn fixed_pin() -> SourcePin {
    SourcePin {
        exact_commit: "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b".into(),
        ..pin()
    }
}
fn symbol(kind: &str, name: &str, category: &str, path: &str, line: usize) -> Value {
    json!({"kind":kind,"name":name,"category":category,"path":path,"line":line,"endLine":line,"confidence":"exact","signature":format!("{name} signature")})
}
fn map(symbols: Vec<Value>, gaps: &[&str]) -> Index {
    serde_json::from_value(json!({"documents":[],"assets":[],"symbols":symbols,"summary":{"complete":gaps.is_empty()},"compatibilityGaps":gaps})).unwrap()
}
fn baseline(gaps: &[&str]) -> Index {
    let mut header = symbol("header", "Interface", "toc-field", "Game.toc", 1);
    header["target"] = json!("120100,50504");
    map(
        vec![
            symbol("declaration", "C_Good.Do", "api-function", "API.lua", 1),
            symbol("declaration", "GOOD_TICK", "api-event", "API.lua", 2),
            symbol("declaration", "BaseTemplate", "xml-Frame", "UI.xml", 3),
            symbol("declaration", "KnownMixin", "mixin", "Mixins.lua", 4),
            header,
        ],
        gaps,
    )
}
fn document(path: &str, raw: &[u8], facts: Facts) -> Document {
    Document {
        path: path.into(),
        bytes: raw.into(),
        facts,
        blob: json!({"sha256":"fixture","bytes":raw.len()}),
    }
}
fn closure(lua: &str, interface: &str) -> Closure {
    let toc = format!("{interface}Main.lua\n");
    Closure {
        root: "fixture-addon".into(),
        manifest: "Main.toc".into(),
        documents: vec![
            document(
                "Main.toc",
                toc.as_bytes(),
                crate::facts::toc(toc.as_bytes()),
            ),
            document(
                "Main.lua",
                lua.as_bytes(),
                crate::facts::lua(lua.as_bytes(), false),
            ),
        ],
        steps: vec![],
        issues: vec![],
        valid: true,
    }
}
fn pairs(usages: &[Usage]) -> Vec<(&str, &str)> {
    usages
        .iter()
        .map(|usage| (usage.kind.as_str(), usage.name.as_str()))
        .collect()
}

#[test]
fn single_check_keeps_reference_coverage_separate_like_go_check_test() {
    let closure = closure(
        "C_Good.Do()\nframe:RegisterEvent('GOOD_TICK')\nhandlers[key]()\nLocalExtension()\n",
        "## Interface: 120100\n",
    );
    let result = check::assess(&pin(), &closure, &baseline(&[]), &Default::default()).unwrap();
    assert_eq!(result["staticValid"], true);
    assert_eq!(result["resolved"], 2);
    assert_eq!(result["unresolved"], 3);
    assert_eq!(result["complete"], false);
    assert_eq!(result["interfaceStatus"], "unresolved");
    assert!(result.get("valid").is_none());
    assert!(result.get("facts").is_none());
    assert!(
        result["references"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["status"] != "source-present"
                || !row["evidence"].as_array().unwrap().is_empty())
    );
}
#[test]
fn single_missing_and_locally_declared_names_stay_unresolved_without_dedupe() {
    let closure = closure(
        "function C_Missing.Do() end\nC_Missing.Do(); C_Missing.Do()\nlocal function LocalExtension() end\nLocalExtension()\n",
        "",
    );
    let result = check::assess(&pin(), &closure, &baseline(&[]), &Default::default()).unwrap();
    assert_eq!(result["staticValid"], true);
    assert_eq!(result["unresolved"], 3);
    assert_eq!(result["references"].as_array().unwrap().len(), 3);
}
#[test]
fn single_ignores_alias_call_extension_and_uses_callback_declarations() {
    let mut closure = closure("", "");
    closure.documents[1].facts.relationships = vec![
        Relationship {
            from: "<file>".into(),
            to: "C_Good.Do".into(),
            category: "alias-call".into(),
            confidence: "static-resolved".into(),
            line: 1,
        },
        Relationship {
            from: "<file>".into(),
            to: "Callback".into(),
            category: "call".into(),
            confidence: "inferred".into(),
            line: 2,
        },
    ];
    let result = check::assess(
        &pin(),
        &closure,
        &map(
            vec![symbol(
                "declaration",
                "Callback",
                "api-callback",
                "Callbacks.lua",
                1,
            )],
            &[],
        ),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(result["resolved"], 1);
    assert_eq!(result["references"].as_array().unwrap().len(), 1);
}
#[test]
fn single_interface_missing_ambiguous_mismatch_and_match_are_distinct() {
    for (headers, status, valid, complete) in [
        ("", "unresolved", true, false),
        (
            "## Interface: 120100\n## Interface: 120100\n",
            "ambiguous",
            false,
            false,
        ),
        ("## Interface: 99999\n", "mismatch", false, false),
        ("## Interface: 50504,120100\n", "matched", true, true),
    ] {
        let result = check::assess(
            &fixed_pin(),
            &closure("", headers),
            &baseline(&[]),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(result["interfaceStatus"], status);
        assert_eq!(result["staticValid"], valid);
        assert_eq!(result["complete"], complete);
        assert_eq!(result["load"]["compatibilityComplete"], complete);
    }
}
#[test]
fn single_cycle_is_invalid_while_shared_native_closure_remains_unchanged() {
    let mut closure = closure("", "## Interface: 120100\n");
    closure.issues.push(json!({"code":"load_cycle","severity":"warning","path":"UI.xml","line":1,"reference":"UI.xml","message":"recursive XML load reference"}));
    let result =
        check::assess(&fixed_pin(), &closure, &baseline(&[]), &Default::default()).unwrap();
    assert_eq!(result["staticValid"], false);
    assert_eq!(result["complete"], false);
    assert_eq!(result["load"]["loadValid"], false);
    assert_eq!(result["load"]["issues"][0]["severity"], "error");
    assert!(closure.valid);
    assert_eq!(closure.issues[0]["severity"], "warning");
}
#[test]
fn single_ambiguity_keeps_twenty_site_limit_and_stable_location_order() {
    let closure = closure("C_Good.Do()", "");
    let records = (0..21)
        .map(|line| {
            symbol(
                "declaration",
                "C_Good.Do",
                "api-function",
                "API.lua",
                21 - line,
            )
        })
        .collect::<Vec<_>>();
    let result = check::assess(
        &pin(),
        &closure,
        &map(records[..20].to_vec(), &[]),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(result["references"][0]["evidence"][0]["line"], 2);
    let error =
        check::assess(&pin(), &closure, &map(records, &[]), &Default::default()).unwrap_err();
    assert_eq!(error.code, "codebase.reference_ambiguity_budget");
}
#[test]
fn single_reference_budget_counts_duplicates_and_does_not_hide_them() {
    let mut closure = closure("", "");
    closure.documents[1].facts.relationships = vec![
        Relationship {
            from: "<file>".into(),
            to: "Missing".into(),
            category: "call".into(),
            confidence: "inferred".into(),
            line: 1
        };
        100_001
    ];
    let error = check::assess(&pin(), &closure, &baseline(&[]), &Default::default()).unwrap_err();
    assert_eq!(error.code, "codebase.reference_budget");
}
#[test]
fn matrix_usage_preserves_go_order_kinds_and_dynamic_reasons() {
    let raw=b"local alias=C_Missing.Do\nalias()\nframe:RegisterEvent('GOOD_TICK')\nframe:RegisterEvent(eventName)\nCreateFrame('Frame',nil,nil,' BaseTemplate, Other ')\nCreateFromMixins(Mixins.A,Compute())\nhandlers[GetName()]()\nobject[C_Key()] = C_Good.Do()\n";
    let (usages, unresolved) = usage::lua("Main.lua", raw, &Default::default()).unwrap();
    assert_eq!(
        pairs(&usages),
        vec![
            ("api-candidate", "alias"),
            ("event", "GOOD_TICK"),
            ("api-candidate", "CreateFrame"),
            ("frame-type", "Frame"),
            ("template", "BaseTemplate"),
            ("template", "Other"),
            ("api-candidate", "CreateFromMixins"),
            ("mixin", "Mixins.A"),
            ("api-candidate", "Compute"),
            ("api-candidate", "GetName"),
            ("api", "C_Good.Do")
        ]
    );
    assert_eq!(
        unresolved
            .iter()
            .map(|row| row["reason"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "event name is computed",
            "Mixin is computed",
            "call target is computed"
        ]
    );
    assert_eq!(unresolved[0]["expression"], "eventName");
    assert_eq!(unresolved[2]["expression"], "<dynamic call>");
}
#[test]
fn matrix_labels_variable_brackets_like_go_and_keeps_candidate_calls() {
    let (usages, unresolved) = usage::lua(
        "Main.lua",
        b"C_Test[key](); C_NoDot(); frame:Other(); C_Test.Object:Method(); (C_Test.Do)()",
        &Default::default(),
    )
    .unwrap();
    assert!(unresolved.is_empty());
    assert_eq!(
        pairs(&usages),
        vec![
            ("api", "C_Test.key"),
            ("api-candidate", "C_NoDot"),
            ("api", "C_Test.Object:Method"),
            ("api", "C_Test.Do")
        ]
    );
}
#[test]
fn matrix_usage_parse_failure_and_shebang_do_not_reuse_public_fact_rows() {
    for raw in [
        b"local broken =\nC_Good.Do()".as_slice(),
        b"#!lua\nC_Good.Do()",
        b"\xef\xbb\xbf#!lua\nC_Good.Do()",
        b"local x = 5 // 2; C_Good.Do()",
        b"local x<const> = 5; C_Good.Do()",
        b"local x = 0x1p2; C_Good.Do()",
    ] {
        let (usages, unresolved) = usage::lua("Main.lua", raw, &Default::default()).unwrap();
        assert!(usages.is_empty());
        assert!(unresolved.is_empty());
    }
}
#[test]
fn matrix_lua_comments_do_not_shift_arguments_and_lines_follow_go_newlines() {
    let raw = b"frame:RegisterEvent(-- first\r\n ('READY'));\rC_Good.Do();\n\rCreateFrame(-- type\n 'Frame', nil, nil, -- inherits\n ('BaseTemplate'))";
    let (usages, unresolved) = usage::lua("Main.lua", raw, &Default::default()).unwrap();
    assert!(unresolved.is_empty());
    assert_eq!(
        pairs(&usages),
        vec![
            ("event", "READY"),
            ("api", "C_Good.Do"),
            ("api-candidate", "CreateFrame"),
            ("frame-type", "Frame"),
            ("template", "BaseTemplate")
        ]
    );
    assert_eq!(
        usages.iter().map(|usage| usage.line).collect::<Vec<_>>(),
        [1, 3, 4, 4, 4]
    );
    let (parenthesized, _) =
        usage::lua("Main.lua", b"(\n C_Good.Do\n)()", &Default::default()).unwrap();
    assert_eq!(parenthesized[0].name, "C_Good.Do");
    assert_eq!(parenthesized[0].line, 1);
}
#[test]
fn matrix_dedupe_preserves_first_usage_and_file_line_identity() {
    let input = vec![
        Usage::new("api", "C_Good.Do", "A.lua", 1),
        Usage::new("api", "C_Good.Do", "A.lua", 1),
        Usage::new("api", "C_Good.Do", "A.lua", 2),
        Usage::new("api", "C_Good.Do", "B.lua", 1),
        Usage::new("api-candidate", "C_Good.Do", "A.lua", 1),
    ];
    let result = usage::dedupe(input);
    assert_eq!(result.len(), 4);
    assert_eq!(result[1].line, 2);
    assert_eq!(result[2].file, "B.lua");
}
#[test]
fn matrix_raw_escaped_event_bytes_do_not_match_json_replacement_or_dedupe_together() {
    let raw = r#"frame:RegisterEvent('\255'); frame:RegisterEvent('\254'); frame:RegisterEvent('�'); frame:RegisterEvent('\255'); frame:RegisterEvent('\226\130')"#;
    let (usages, unresolved) = usage::lua("Main.lua", raw.as_bytes(), &Default::default()).unwrap();
    assert!(unresolved.is_empty());
    let usages = usage::dedupe(usages);
    assert_eq!(usages.len(), 4);
    assert_eq!(
        usages
            .iter()
            .map(|usage| usage.name.as_str())
            .collect::<Vec<_>>(),
        ["�", "�", "�", "��"]
    );
    assert_eq!(
        usages[0].invalid_name_bytes.as_deref(),
        Some([255].as_slice())
    );
    assert_eq!(
        usages[1].invalid_name_bytes.as_deref(),
        Some([254].as_slice())
    );
    assert!(usages[2].invalid_name_bytes.is_none());
    let index = map(
        vec![
            symbol("declaration", "�", "api-event", "API.lua", 1),
            symbol("declaration", "��", "api-event", "API.lua", 2),
        ],
        &[],
    );
    let result = compat::assess(&pin(), "PIN", &index, &usages, "", &Default::default()).unwrap();
    assert!(result.unresolved.is_empty());
    assert_eq!(
        result
            .facts
            .iter()
            .map(|fact| fact["exists"] == true)
            .collect::<Vec<_>>(),
        [false, false, true, false]
    );
    assert_eq!(result.diagnostics.len(), 3);
}
#[test]
fn matrix_raw_escaped_attribute_and_csv_names_retain_internal_byte_identity() {
    let raw = r#"C_Test['\255'](); C_Test['�'](); CreateFrame('Frame',nil,nil,' \255,\254, � ')"#;
    let (usages, unresolved) = usage::lua("Main.lua", raw.as_bytes(), &Default::default()).unwrap();
    assert!(unresolved.is_empty());
    let usages = usage::dedupe(usages);
    let index = map(
        vec![
            symbol("declaration", "C_Test.�", "api-function", "API.lua", 1),
            symbol("declaration", "�", "xml-Frame", "UI.xml", 2),
        ],
        &[],
    );
    let result = compat::assess(&pin(), "PIN", &index, &usages, "", &Default::default()).unwrap();
    assert_eq!(result.unresolved.len(), 1); // CreateFrame is an unindexed candidate.
    assert_eq!(
        result
            .facts
            .iter()
            .map(|fact| fact["exists"] == true)
            .collect::<Vec<_>>(),
        [false, true, true, false, false, true]
    );
    assert_eq!(result.diagnostics.len(), 3);
}
#[test]
fn matrix_xml_uses_anonymous_frames_and_inherits_without_inline_call_pollution() {
    let (usages,unresolved)=usage::xml("UI.xml",b"<Ui>\n<Frame inherits='BaseTemplate, $parentOther'><Scripts><OnLoad function='Do'>C_Missing.Do()</OnLoad></Scripts></Frame><Include file='Child.xml'/></Ui>",&Default::default()).unwrap();
    assert_eq!(
        pairs(&usages),
        vec![
            ("frame-type", "Frame"),
            ("template", "BaseTemplate"),
            ("frame-type", "OnLoad")
        ]
    );
    assert_eq!(usages[0].line, 2);
    assert_eq!(unresolved[0]["reason"], "XML inheritance is dynamic");
    assert_eq!(unresolved[0]["expression"], "$parentOther");
}
#[test]
fn matrix_xml_error_returns_only_usages_before_failed_token() {
    for raw in [
        b"<Ui><Frame/><Button other='&unknown;'/><Texture/></Ui>".as_slice(),
        b"<Ui><Frame/>&unknown;<Button/></Ui>",
        b"<Ui><Frame/>&#0;<Button/></Ui>",
        b"<Ui><Frame/>\x01<Button/></Ui>",
        b"<Ui><Frame/><![CDATA[\x01]]><Button/></Ui>",
        b"<Ui><Frame/>]]><Button/></Ui>",
        b"<Ui><Frame/><!-- a--b --><Button/></Ui>",
    ] {
        let (usages, _) = usage::xml("UI.xml", raw, &Default::default()).unwrap();
        assert_eq!(pairs(&usages), vec![("frame-type", "Frame")]);
    }
}
#[test]
fn matrix_xml_keeps_go_attribute_normalization_and_repeated_inherits() {
    let (usages, unresolved) = usage::xml(
        "UI.xml",
        b"<Ui><Frame inherits='A\r\nB, C&#13;D' inherits='E'/></Ui>",
        &Default::default(),
    )
    .unwrap();
    assert!(unresolved.is_empty());
    assert_eq!(
        pairs(&usages),
        vec![
            ("frame-type", "Frame"),
            ("template", "A\nB"),
            ("template", "C\rD"),
            ("template", "E")
        ]
    );
}
#[test]
fn matrix_known_categories_prove_absence_but_index_gaps_preserve_positive_evidence() {
    let usages = vec![
        Usage::new("api", "C_Good.Do", "A.lua", 1),
        Usage::new("api", "C_Missing.Do", "A.lua", 2),
        Usage::new("event", "MISSING_TICK", "A.lua", 3),
    ];
    let complete = compat::assess(
        &pin(),
        "PIN-TEST",
        &baseline(&[]),
        &usages,
        "120100",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(complete.facts.len(), 4);
    assert_eq!(complete.diagnostics.len(), 2);
    assert!(complete.unresolved.is_empty());
    assert_eq!(
        complete.diagnostics[0]["message"],
        "API C_Missing.Do does not exist in the target snapshot"
    );
    let gap = compat::assess(
        &pin(),
        "PIN-TEST",
        &baseline(&["api", "event"]),
        &usages,
        "120100",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(gap.facts.len(), 2);
    assert_eq!(gap.unresolved.len(), 2);
    assert!(gap.diagnostics.is_empty());
    assert_eq!(gap.facts[0]["name"], "C_Good.Do");
}
#[test]
fn matrix_unrelated_category_gap_does_not_erase_known_api_absence() {
    let usages = vec![
        Usage::new("api", "C_Missing.Do", "A.lua", 1),
        Usage::new("template", "MissingTemplate", "UI.xml", 2),
    ];
    let result = compat::assess(
        &pin(),
        "PIN",
        &baseline(&["template", "frame-type"]),
        &usages,
        "",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(result.facts.len(), 1);
    assert_eq!(result.diagnostics[0]["code"], "api_not_found");
    assert_eq!(result.unresolved.len(), 1);
}
#[test]
fn matrix_frame_mixin_and_candidate_absence_remain_unresolved() {
    let usages = vec![
        Usage::new("frame-type", "Frame", "A.lua", 1),
        Usage::new("frame-type", "UnknownFrame", "A.lua", 2),
        Usage::new("mixin", "KnownMixin", "A.lua", 3),
        Usage::new("mixin", "UnknownMixin", "A.lua", 4),
        Usage::new("api-candidate", "AddonOwned", "A.lua", 5),
        Usage::new("api-candidate", "C_Good.Do", "A.lua", 6),
    ];
    let result = compat::assess(
        &pin(),
        "PIN-TEST",
        &baseline(&[]),
        &usages,
        "",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(result.facts.len(), 3);
    assert_eq!(result.unresolved.len(), 3);
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.facts[2]["kind"], "api");
    assert_eq!(
        result.unresolved[2]["reason"],
        "the call is not an indexed API and may be an AddOn-defined global"
    );
}
#[test]
fn matrix_callback_category_is_not_an_api_category() {
    let result = compat::assess(
        &pin(),
        "PIN",
        &map(
            vec![symbol(
                "declaration",
                "Callback",
                "api-callback",
                "API.lua",
                1,
            )],
            &[],
        ),
        &[Usage::new("api-candidate", "Callback", "A.lua", 1)],
        "",
        &Default::default(),
    )
    .unwrap();
    assert!(result.facts.is_empty());
    assert_eq!(result.unresolved.len(), 1);
    assert!(result.diagnostics.is_empty());
}
#[test]
fn matrix_interface_uses_index_headers_and_fixed_fallback_without_inventing_absence() {
    let known = compat::assess(
        &pin(),
        "PIN",
        &baseline(&[]),
        &[],
        "50504,120100",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(known.facts[0]["exists"], true);
    let unknown = compat::assess(
        &pin(),
        "PIN",
        &map(vec![], &[]),
        &[],
        "120100",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        unknown.unresolved[0]["reason"],
        "the indexed snapshot has no authoritative Interface evidence"
    );
    let fallback = compat::assess(
        &fixed_pin(),
        "PIN",
        &map(vec![], &["interface"]),
        &[],
        "120100",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        fallback.facts[0]["evidence"]["matches"][0]["path"],
        "selection-baseline"
    );
    let missing = compat::assess(
        &fixed_pin(),
        "PIN",
        &map(vec![], &[]),
        &[],
        "99999",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(missing.diagnostics[0]["code"], "toc_interface_mismatch");
    assert_eq!(missing.diagnostics[0]["file"], "");
}
#[test]
fn matrix_match_evidence_caps_at_five_and_keeps_each_usage_location() {
    let records = (0..70)
        .map(|line| {
            let mut row = symbol(
                "declaration",
                "C_Good.Do",
                "api-function",
                "API.lua",
                70 - line,
            );
            row["signature"] = json!(format!("signature-{}", 70 - line));
            row
        })
        .collect();
    let usages = vec![
        Usage::new("api", "C_Good.Do", "A.lua", 2),
        Usage::new("api", "C_Good.Do", "B.lua", 8),
    ];
    let result = compat::assess(
        &pin(),
        "PIN-TEST",
        &map(records, &[]),
        &usages,
        "",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(result.facts.len(), 2);
    let evidence = &result.facts[0]["evidence"];
    assert_eq!(evidence["matchCount"], 51);
    assert_eq!(evidence["matchesTruncated"], true);
    assert_eq!(evidence["matches"].as_array().unwrap().len(), 5);
    assert_eq!(evidence["matches"][0]["line"], 66);
    assert_eq!(result.facts[0]["signature"], "signature-66");
    assert_eq!(result.facts[1]["signature"], "signature-66");
    assert_eq!(evidence["usage"]["file"], "A.lua");
    assert_eq!(result.facts[1]["evidence"]["usage"]["file"], "B.lua");
    assert!(evidence["matchedTag"].is_null());
    assert_eq!(evidence["snapshotId"], "PIN-TEST");
}
#[test]
fn matrix_many_requested_names_use_fixed_resolutions_and_keep_all_absences() {
    let usages = (0..64)
        .map(|line| Usage::new("api", format!("C_Missing.Func{line}"), "A.lua", line + 1))
        .collect::<Vec<_>>();
    let result = compat::assess(
        &pin(),
        "PIN",
        &baseline(&[]),
        &usages,
        "120100",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(result.facts.len(), 65);
    assert_eq!(result.diagnostics.len(), 64);
    assert!(result.unresolved.is_empty());
}
#[test]
fn matrix_usages_read_authenticated_bytes_and_native_inline_inputs_remain_independent() {
    let mut closure = closure("C_Good.Do()", "");
    closure.documents[1].facts = Facts::default();
    let xml =
        b"<Ui><Frame><Scripts><OnLoad>local value = C_Good.Do()</OnLoad></Scripts></Frame></Ui>";
    closure
        .documents
        .push(document("UI.xml", xml, Facts::default()));
    let (usages, _) = matrix_usages(&closure, &Default::default()).unwrap();
    assert_eq!(
        usages
            .iter()
            .filter(|usage| usage.name == "C_Good.Do")
            .count(),
        1
    );
    let (inputs, inline) = crate::xml_inline::inputs(&closure.documents).unwrap();
    assert_eq!(inline.len(), 1);
    assert!(
        inputs
            .values()
            .any(|bytes| String::from_utf8_lossy(bytes).contains("local value = C_Good.Do()"))
    );
    closure.documents[1]
        .facts
        .diagnostics
        .push(crate::facts::SyntaxNote {
            message: "parse failure".into(),
            line: 1,
        });
    let (independent, _) = matrix_usages(&closure, &Default::default()).unwrap();
    assert!(independent.iter().any(|usage| usage.name == "C_Good.Do"));
}
#[test]
fn both_static_consumers_preserve_cancellation_even_for_empty_inputs() {
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert!(check::assess(&pin(), &closure("", ""), &baseline(&[]), &control).is_err());
    assert!(compat::assess(&pin(), "PIN", &baseline(&[]), &[], "", &control).is_err());
    assert!(usage::lua("Main.lua", b"", &control).is_err());
    assert!(usage::xml("UI.xml", b"", &control).is_err());
}
#[test]
fn both_validation_entries_reject_bad_source_before_addon_or_syntax_captures() {
    let home = std::env::temp_dir().join(format!(
        "lycheedev-validate-order-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    lycheedev_core::workspace::initialize(&home).unwrap();
    let invalid = SourcePin {
        exact_commit: "not-a-commit".into(),
        ..pin()
    };
    let root = home.join("missing-addon");
    let tools = SourceTools {
        luac: None,
        luals: None,
    };
    let single = addon_with_environment(
        &home,
        &invalid,
        "PIN",
        &root,
        "not-a-toc",
        &tools,
        false,
        &Default::default(),
        None,
    )
    .unwrap_err();
    let matrix = toc_with_control(
        &home,
        &invalid,
        "",
        &root,
        "not-a-toc",
        &tools,
        false,
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(single.code, "codebase.invalid_source_pin");
    assert_eq!(matrix.code, "codebase.invalid_source_pin");
    let metadata = lycheedev_core::metadata::Metadata::open(&home, false).unwrap();
    assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
    metadata.close().unwrap();
    assert!(!root.exists());
    // Preserve this fresh failure fixture for consolidated test evidence.
}
