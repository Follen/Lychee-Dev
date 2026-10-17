use super::*;
use lycheedev_core::control::OperationControl;
use std::{collections::BTreeSet, thread, time::Instant};

const FACT_LIMIT: usize = 4 << 20;
const SOURCE_LIMIT: usize = 64 << 10;

fn on_worker_stack(test: impl FnOnce() + Send + 'static) {
    thread::Builder::new()
        .name("source-facts-regression".into())
        .stack_size(2 << 20)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}

fn assert_full_facts(actual: &Facts, expected: &Facts, context: &str) {
    assert_eq!(
        serde_json::to_vec(actual).unwrap(),
        serde_json::to_vec(expected).unwrap(),
        "complete serialized fields and ordering: {context}"
    );
    assert_eq!(
        actual.raw_relationship_targets, expected.raw_relationship_targets,
        "nonserialized raw target identity: {context}"
    );
    for ordinal in 0..expected.relationships.len() {
        assert_eq!(
            actual.relationship_target_bytes(ordinal),
            expected.relationship_target_bytes(ordinal),
            "target ordinal {ordinal}: {context}"
        );
    }
}

fn worker_matches_original(raw: &[u8], mode: ParseMode, context: &str) -> Facts {
    let control = OperationControl::default();
    let expected = mode.parse(raw);
    let actual = for_worker(raw, mode, &control, FACT_LIMIT)
        .unwrap_or_else(|| panic!("bounded supported document was abandoned: {context}"));
    assert_full_facts(&actual, &expected, context);
    assert!(control.check().is_ok());
    actual
}

#[test]
fn xml_worker_preserves_all_fields_across_tokens_bom_and_newlines() {
    on_worker_stack(|| {
        let documents = [
            (
                "full facts",
                "<Ui>\n<Frame name=\"Owner\" inherits=\"Base, Other\"><Scripts>\n<OnLoad>local function Inner() return 1 end\nInner(); Run()</OnLoad>\n<OnShow function=\"Shown\" method=\"Display\"/></Scripts></Frame>\n<Script FiLe=\"Full.lua\"/><Include file=\"Next.xml\"/>\n</Ui>",
            ),
            (
                "entities, CDATA and comments",
                "<Ui>\n<Frame name=\"Entity\"><OnLoad>R<![CDATA[u]]><!-- omitted\n -->n(); frame:RegisterEvent(&quot;READY&quot;)</OnLoad></Frame>\n</Ui>",
            ),
            (
                "namespace and ignored attributes",
                "<?xml version=\"1.0\"?><!DOCTYPE Root><?tool ignored?>\n<Root xmlns:x=\"urn:test\"><x:Frame x:name=\"Namespace\" ignored=\"valid &amp; value\"><OnLoad>Run()</OnLoad></x:Frame></Root>",
            ),
            (
                "duplicate local names keep their last value",
                "<Ui xmlns:x=\"urn:test\">\n<Frame name=\"First\" x:NAME=\"Last\" inherits=\"Base\"/><Script file=\"First.lua\" FiLe=\"Last.lua\"/>\n</Ui>",
            ),
            (
                "attribute normalization precedes reference expansion",
                "<Ui>\n<Frame name=\"A\nB\tC&#13;D&#10;E\"/>\n</Ui>",
            ),
            (
                "invalid ignored entity retains prior facts",
                "<Ui>\n<Frame name=\"Before\"/><Frame ignored=\"&notKnown;\"/>\n</Ui>",
            ),
            (
                "invalid ignored character retains prior facts",
                "<Ui>\n<Frame name=\"Before\"/><Frame ignored=\"&#0;\"/>\n</Ui>",
            ),
            (
                "invalid ignored lexical value retains prior facts",
                "<Ui>\n<Frame name=\"Before\"/><Frame ignored=\"<\"/>\n</Ui>",
            ),
        ];
        for bom in ["", "\u{feff}"] {
            for newline in ["\n", "\r\n", "\r"] {
                for (name, document) in documents {
                    let text = format!("{bom}{}", document.replace('\n', newline));
                    let context = format!("{name}, BOM={}, newline={newline:?}", !bom.is_empty());
                    let actual = worker_matches_original(text.as_bytes(), ParseMode::Xml, &context);
                    assert!(actual.raw_relationship_targets.is_some());
                    assert!(!actual.declarations.is_empty(), "{context}");
                    if name.starts_with("invalid ignored") {
                        assert_eq!(actual.declarations[0].name, "Before");
                        assert_eq!(actual.diagnostics.len(), 1, "{context}");
                    } else {
                        assert!(
                            actual.diagnostics.is_empty(),
                            "{context}: {:?}",
                            actual.diagnostics
                        );
                    }
                    if name == "full facts" {
                        assert_eq!(actual.declarations.len(), 2);
                        assert_eq!(actual.loads.len(), 2);
                        assert_eq!(actual.loads[0].path, "Full.lua");
                        assert_eq!(actual.loads[1].path, "Next.xml");
                        assert!(!actual.relationships.is_empty());
                    }
                    if name.starts_with("duplicate") {
                        assert_eq!(actual.declarations[0].name, "Last");
                        assert_eq!(actual.loads[0].path, "Last.lua");
                    }
                }
            }
        }
    });
}

#[test]
fn xml_worker_keeps_inline_raw_targets_at_appended_ordinals() {
    on_worker_stack(|| {
        let raw = br#"<Ui><Frame name="F" inherits="T"><Scripts><OnLoad>C_X["\255"](); frame:RegisterEvent("\254")</OnLoad><OnShow>frame:RegisterEvent("\253")</OnShow></Scripts></Frame></Ui>"#;
        let actual = worker_matches_original(raw, ParseMode::Xml, "multiple inline raw targets");
        assert!(actual.diagnostics.is_empty());
        assert_eq!(actual.relationship_target_bytes(0), Some(b"T".as_slice()));
        assert_eq!(
            actual.relationship_target_bytes(1),
            Some(b"C_X.\xff".as_slice())
        );
        assert_eq!(
            actual.relationship_target_bytes(3),
            Some(b"\xfe".as_slice())
        );
        assert_eq!(
            actual.relationship_target_bytes(5),
            Some(b"\xfd".as_slice())
        );
        assert_eq!(
            actual
                .raw_relationship_targets
                .as_ref()
                .unwrap()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [1, 3, 5]
        );
        let public = serde_json::to_value(&actual).unwrap();
        assert_eq!(public.as_object().unwrap().len(), 5);
        let cached: Facts = serde_json::from_value(public).unwrap();
        assert!(cached.raw_relationship_targets.is_none());
    });
}

#[test]
fn toc_worker_preserves_bom_headers_trimmed_loads_and_line_order() {
    on_worker_stack(|| {
        let document = "  ## Interface : 120100  \n\t## Title: First: detail\t\n## Title: Second\n## invalid header\n# comment\n \t\n  Folder\\Full.lua  \n\tNext.xml\t\n  ## Empty :  \n界.lua\n";
        for bom in ["", "\u{feff}"] {
            for newline in ["\n", "\r\n"] {
                let text = format!("{bom}{}", document.replace('\n', newline));
                let actual =
                    worker_matches_original(text.as_bytes(), ParseMode::Toc, "TOC line forms");
                assert_eq!(
                    actual
                        .headers
                        .iter()
                        .map(|h| (h.key.as_str(), h.value.as_str(), h.line))
                        .collect::<Vec<_>>(),
                    [
                        ("Interface", "120100", 1),
                        ("Title", "First: detail", 2),
                        ("Title", "Second", 3),
                        ("Empty", "", 9),
                    ]
                );
                assert_eq!(
                    actual
                        .loads
                        .iter()
                        .map(|load| (load.path.as_str(), load.line, load.kind.as_str()))
                        .collect::<Vec<_>>(),
                    [
                        ("Folder\\Full.lua", 7, "toc"),
                        ("Next.xml", 8, "toc"),
                        ("界.lua", 10, "toc"),
                    ]
                );
                assert!(actual.diagnostics.is_empty());
                assert!(actual.raw_relationship_targets.as_ref().unwrap().is_empty());
            }
        }
    });
}

#[test]
fn worker_matches_all_84_fixed_malformed_and_xml_diagnostic_cases() {
    on_worker_stack(|| {
        let fixtures = [
            (
                include_str!("../../../../tests/rust-parity/source-facts-malformed/cases.json"),
                52,
            ),
            (
                include_str!(
                    "../../../../tests/rust-parity/source-facts-xml-diagnostics/cases.json"
                ),
                32,
            ),
        ];
        let mut seen = BTreeSet::new();
        let mut lua_cases = 0;
        let mut xml_cases = 0;
        for (fixture, count) in fixtures {
            let input: serde_json::Value = serde_json::from_str(fixture).unwrap();
            assert_eq!(input["schema"], "lycheedev.source-facts-malformed-cases.v1");
            assert_eq!(
                input["oracleCommit"],
                "3766f9cacb52e93a803a626187e2a1d54a445468"
            );
            let cases = input["cases"].as_array().unwrap();
            assert_eq!(cases.len(), count);
            for case in cases {
                let id = case["id"].as_str().unwrap();
                assert!(seen.insert(id.to_owned()), "duplicate fixed case {id}");
                let path = case["path"].as_str().unwrap();
                let raw = case["source"].as_str().unwrap().as_bytes();
                let mode = ParseMode::for_path(path);
                match mode {
                    ParseMode::LuaOrdinary => lua_cases += 1,
                    ParseMode::Xml => xml_cases += 1,
                    _ => panic!("unexpected fixed fixture mode: {path}"),
                }
                worker_matches_original(raw, mode, id);
            }
        }
        assert_eq!(seen.len(), 84);
        assert_eq!((lua_cases, xml_cases), (29, 55));
    });
}

#[test]
fn xml_inline_handlers_share_one_document_owned_facts_allowance() {
    on_worker_stack(|| {
        const SMALL_FACT_LIMIT: usize = 20 << 10;
        const HANDLERS: usize = 64;
        let control = OperationControl::default();
        let mut combined = String::from("<Ui><Frame name=\"Owner\"><Scripts>");
        let mut output_only = combined.clone();
        for ordinal in 0..HANDLERS {
            let handler = format!("<OnLoad>Call{ordinal}()</OnLoad>");
            let single =
                format!("<Ui><Frame name=\"Owner\"><Scripts>{handler}</Scripts></Frame></Ui>");
            let actual = for_worker(
                single.as_bytes(),
                ParseMode::Xml,
                &control,
                SMALL_FACT_LIMIT,
            )
            .expect("every inline handler individually fits the same allowance");
            assert_full_facts(&actual, &xml(single.as_bytes()), "individual handler");
            combined.push_str(&handler);
            output_only.push_str(&format!("<OnLoad function=\"Call{ordinal}\"/>"));
        }
        combined.push_str("</Scripts></Frame></Ui>");
        output_only.push_str("</Scripts></Frame></Ui>");
        assert!(combined.len() < SOURCE_LIMIT);
        // The output-only XML control fits. Nested Lua's intermediate Facts,
        // scopes and final merged records must all share the document allowance.
        assert!(
            for_worker(
                output_only.as_bytes(),
                ParseMode::Xml,
                &control,
                SMALL_FACT_LIMIT
            )
            .is_some()
        );
        assert!(
            for_worker(
                combined.as_bytes(),
                ParseMode::Xml,
                &control,
                SMALL_FACT_LIMIT
            )
            .is_none()
        );
        assert!(
            control.check().is_ok(),
            "speculative ownership is not an operation budget"
        );
        let complete = worker_matches_original(
            combined.as_bytes(),
            ParseMode::Xml,
            "combined handlers with production allowance",
        );
        assert_eq!(complete.relationships.len(), HANDLERS);
        assert!(complete.diagnostics.is_empty());
        for (ordinal, relationship) in complete.relationships.iter().enumerate() {
            assert_eq!(relationship.to, format!("Call{ordinal}"));
        }
    });
}

#[test]
fn worker_refuses_oversized_non_utf8_generated_and_exhausted_facts_inputs() {
    on_worker_stack(|| {
        let control = OperationControl::default();
        for mode in [ParseMode::LuaOrdinary, ParseMode::Xml, ParseMode::Toc] {
            let mut raw = vec![b' '; SOURCE_LIMIT];
            assert!(
                for_worker(&raw, mode, &control, FACT_LIMIT).is_some(),
                "exact source limit: {mode:?}"
            );
            raw.push(b' ');
            assert!(for_worker(&raw, mode, &control, FACT_LIMIT).is_none());
            assert!(for_worker(b"\xff", mode, &control, FACT_LIMIT).is_none());
        }
        assert!(for_worker(b"", ParseMode::LuaGenerated, &control, FACT_LIMIT).is_none());
        for (mode, raw) in [
            (ParseMode::LuaOrdinary, b"function Full() end".as_slice()),
            (
                ParseMode::Xml,
                b"<Ui><Frame name=\"Full\"/></Ui>".as_slice(),
            ),
            (ParseMode::Toc, b"## Title: Full\nFull.lua\n".as_slice()),
        ] {
            assert!(
                for_worker(raw, mode, &control, 0).is_none(),
                "zero Facts allowance: {mode:?}"
            );
            worker_matches_original(raw, mode, "ordinary fallback remains complete");
        }
        assert!(control.check().is_ok());
    });
}

#[test]
fn worker_observes_cancellation_and_deadline_without_selecting_new_public_errors() {
    on_worker_stack(|| {
        for expired in [false, true] {
            let control = if expired {
                OperationControl::uncancelable(Instant::now())
            } else {
                let control = OperationControl::default();
                control.tracked_cancel_handle().unwrap().cancel();
                control
            };
            for (mode, raw) in [
                (ParseMode::LuaOrdinary, b"function Full() end".as_slice()),
                (
                    ParseMode::Xml,
                    b"<Ui><Frame name=\"Full\"><OnLoad>Run()</OnLoad></Frame></Ui>".as_slice(),
                ),
                (ParseMode::Toc, b"## Title: Full\nFull.lua\n".as_slice()),
            ] {
                assert!(
                    for_worker(raw, mode, &control, FACT_LIMIT).is_none(),
                    "{mode:?}"
                );
                assert_eq!(
                    control.check().unwrap_err().code,
                    if expired {
                        "command.deadline"
                    } else {
                        "command.cancelled"
                    }
                );
            }
        }
    });
}
