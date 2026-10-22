//! Exact pre-change generated traversal retained only as a differential oracle.
//! Source: 5766dccc1fec7d20f5d8120bca7dcd79eed726c4 lua_tree.rs.
use super::*;
use crate::facts::ParseMode;
use std::time::Instant;

/// A fixture JSON row carries public facts only; the fresh parser's raw target
/// identity starts empty because no pre-JSON bytes are part of the fixture.
fn expected_fresh(value: serde_json::Value) -> Facts {
    let mut facts: Facts = serde_json::from_value(value).unwrap();
    facts.raw_relationship_targets = Some(std::collections::BTreeMap::new());
    facts
}

fn on_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(test)
        .unwrap()
        .join()
        .unwrap();
}
fn same_complete(actual: &Facts, expected: &Facts, label: &str) {
    assert_eq!(
        serde_json::to_vec(actual).unwrap(),
        serde_json::to_vec(expected).unwrap(),
        "all public fields and ordering: {label}"
    );
    assert_eq!(
        actual.raw_relationship_targets, expected.raw_relationship_targets,
        "raw targets: {label}"
    );
    for ordinal in 0..expected.relationships.len() {
        assert_eq!(
            actual.relationship_target_bytes(ordinal),
            expected.relationship_target_bytes(ordinal),
            "raw ordinal {ordinal}: {label}"
        );
    }
}
#[test]
fn generated_boundaries_preserve_outer_origins_table_ends_and_first_operator_failures() {
    on_stack(|| {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/source-native/facts/generated/boundary-cases.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 11);
        for case in cases {
            let raw = case["source"].as_str().unwrap().as_bytes();
            let id = case["id"].as_str().unwrap();
            let expected = expected_fresh(case["expectedRustFacts"].clone());
            let actual = crate::facts::lua(raw, true);
            same_complete(&actual, &expected, id);
            let worker = crate::facts::for_worker(
                raw,
                ParseMode::LuaGenerated,
                &OperationControl::default(),
                4 << 20,
            )
            .expect("complete small boundary input fits the unchanged guard");
            same_complete(&worker, &expected, id);
            let row = serde_json::json!({
                "id":case["id"], "path":case["path"], "facts":&actual, "error":null
            });
            // Independent Go observations use the unchanged overlay with the
            // boundary fixture. This marker is a candidate contract, not a
            // claim that Go has accurate multiline table spans.
            println!("SOURCE_FACT_BOUNDARY_CANDIDATE_JSON {row}");
            if !expected.diagnostics.is_empty() {
                let ordinary = crate::facts::lua(raw, false);
                same_complete(&ordinary, &expected, id);
                let worker = crate::facts::for_worker(
                    raw,
                    ParseMode::LuaOrdinary,
                    &OperationControl::default(),
                    4 << 20,
                )
                .unwrap();
                same_complete(&worker, &expected, id);
            }
        }
    });
}

#[test]
fn floor_division_detection_uses_only_real_binary_operators() {
    on_stack(|| {
        for raw in [
            b"local text='//'; -- //\nBefore(); return 1 / 2".as_slice(),
            b"local text=[=[//]=]; --[=[//]=]\nBefore()".as_slice(),
        ] {
            let document = SyntaxDocument::new_public(raw).unwrap();
            assert!(document.public_parse_note().is_none());
            let facts = document.facts(true);
            assert!(facts.diagnostics.is_empty());
            assert!(facts.relationships.iter().any(|edge| edge.to == "Before"));
            let worker = crate::facts::for_worker(
                raw,
                ParseMode::LuaGenerated,
                &OperationControl::default(),
                4 << 20,
            )
            .unwrap();
            same_complete(
                &worker,
                &facts,
                "quoted/comment slash text is not an operator",
            );
        }
        // Moving the bitwise check to its operator must retain the old generic
        // message and expression-start line (2), rather than operator line (3).
        let raw = b"return\n(1\n& 2)\n// 3";
        let expected = expected_fresh(serde_json::json!({
            "declarations":[],"relationships":[],"loads":[],"headers":[],
            "diagnostics":[{"message":"Syntax unavailable in the current Lua parser","line":2}]
        }));
        same_complete(
            &crate::facts::lua(raw, true),
            &expected,
            "earlier bitwise origin",
        );
        let worker = crate::facts::for_worker(
            raw,
            ParseMode::LuaGenerated,
            &OperationControl::default(),
            4 << 20,
        )
        .unwrap();
        same_complete(&worker, &expected, "guarded earlier bitwise origin");
        // A recovered ERROR does not gain the binary second-slash rule merely
        // because its raw text contains '//'. Test the exact recognition gate.
        let document = SyntaxDocument::new_public(b"return // 2").unwrap();
        let mut cursor = document.root().walk();
        loop {
            let node = cursor.node();
            if node.kind() == "ERROR" {
                assert!(!fixed_floor_division_operator(node));
            }
            if node.kind() == "//" && fixed_floor_division_operator(node) {
                let parent = node.parent().unwrap();
                assert_eq!(parent.kind(), "binary_expression");
                assert_eq!(parent.child_by_field_name("operator"), Some(node));
            }
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return;
                }
            }
        }
    });
}
#[test]
fn generated_nested_roots_are_repeated_after_ordinary_observations_in_exact_order() {
    let raw = b"Before(); local T={Type='System',Name='Root',Namespace='C_Root',Tables={{Type='System',Name='Inner',Namespace='C_Inner',Functions={{Type='Function',Name='Run'}}},{Type='Structure',Name='Sibling'}}}; After()";
    let facts = crate::facts::lua(raw, true);
    assert_eq!(
        facts
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Root",
            "Inner",
            "C_Inner.Run",
            "C_Root.Sibling",
            "Inner",
            "C_Inner.Run"
        ]
    );
    assert_eq!(
        facts
            .relationships
            .iter()
            .take(2)
            .map(|r| r.to.as_str())
            .collect::<Vec<_>>(),
        ["Before", "After"]
    );
}
#[test]
fn generated_deep_documentation_uses_heap_continuations_on_default_stack() {
    on_stack(|| {
        let depth = 1800;
        let mut raw = String::from("local T={Type='System',Name='Root',Tables={");
        raw.push_str(&"{Tables={".repeat(depth));
        raw.push_str("{Type='Function',Name='Deep'}");
        raw.push_str(&"}}".repeat(depth));
        raw.push_str("}}");
        assert!(raw.len() < 64 << 10);
        let facts = crate::facts::lua(raw.as_bytes(), true);
        assert_eq!(facts.declarations.last().unwrap().name, "Deep");
        assert!(
            facts
                .diagnostics
                .iter()
                .any(|note| note.message == "source_ast_depth_limit")
        );
        let control = OperationControl::default();
        assert!(
            crate::facts::for_worker(raw.as_bytes(), ParseMode::LuaGenerated, &control, 4 << 20)
                .is_none(),
            "ordinary guarded walk may abandon; no partial result is publishable"
        );
        assert!(control.check().is_ok());
    });
}
#[test]
fn generated_large_namespace_amplification_abandons_all_partial_worker_facts() {
    on_stack(|| {
        let raw = format!(
            "Before(); local T={{Type='System',Name='Root',Namespace='{}',Functions={{{}}}}}; After()",
            "N".repeat(8192),
            (0..512)
                .map(|i| format!("{{Type='Function',Name='F{i}',Arguments={{{{}}}}}},"))
                .collect::<String>()
        );
        assert!(raw.len() < 64 << 10);
        let control = OperationControl::default();
        assert!(
            crate::facts::for_worker(raw.as_bytes(), ParseMode::LuaGenerated, &control, 4 << 20)
                .is_none()
        );
        assert!(control.check().is_ok());
        let complete = crate::facts::lua(raw.as_bytes(), true);
        assert_eq!(complete.declarations.len(), 513);
        assert_eq!(complete.declarations[0].name,"Root");
    });
}
#[test]
fn generated_signature_dynamic_and_replacement_expansion_keep_exact_bytes() {
    on_stack(|| {
        let raw = format!(
            r#"local T={{Type='System',Name='C_Test',Functions={{{{Type='Function',Name='Run',Arguments={{{}}},Returns={{{{Name='\255\254',Type='\253'}}}}}}}},Events={{{{Type='Event',Name='E',LiteralName='\252',Payload={{{}}}}}}}}}"#,
            "{},".repeat(800),
            "{},".repeat(600)
        );
        let actual = crate::facts::for_worker(
            raw.as_bytes(),
            ParseMode::LuaGenerated,
            &OperationControl::default(),
            4 << 20,
        )
        .unwrap();
        assert_eq!(actual.declarations[0].name,"C_Test");
        assert!(actual.declarations[1].signature.contains("�"));
    });
}
#[test]
fn generated_shared_scratch_and_cancelled_guards_never_publish_partial_facts() {
    on_stack(|| {
        let raw = b"local T={Type='System',Name='Root',Functions={{Type='Function',Name='Run'}}}";
        let control = OperationControl::default();
        let guard = WorkerGuard::new(&control, 4 << 20);
        let _outer = guard
            .reserve_scratch((8 << 20) - raw.len() * 64 - 4095)
            .unwrap();
        assert!(generated_facts_with_worker_guard(raw, true, &guard).is_none());
        assert!(control.check().is_ok());
        for expired in [false, true] {
            let control = if expired {
                OperationControl::uncancelable(Instant::now())
            } else {
                let control = OperationControl::default();
                control.tracked_cancel_handle().unwrap().cancel();
                control
            };
            assert!(
                crate::facts::for_worker(raw, ParseMode::LuaGenerated, &control, 4 << 20).is_none()
            );
        }
    });
}

#[test]
fn lua_linear_grammar_and_lexical_scans_bound_mid_scan_cancellation() {
    on_stack(|| {
        for source in [
            format!("--{}\nCall()", "x".repeat(20000)),
            format!("local s='{}'", "x".repeat(20000)),
            format!("local s=[=[{}]=]", "x".repeat(20000)),
            format!("--[{}[x]{}]", "=".repeat(9000), "=".repeat(9000)),
            format!("{}()", "a".repeat(20000)),
            format!("local s='{}'", "\\q".repeat(10000)),
        ] {
            for lexical in [false, true] {
                let control = OperationControl::default();
                let guard = WorkerGuard::new(&control, 4 << 20);
                if lexical {
                    assert_eq!(
                        public_lexical_note(source.as_bytes(), Some(&guard))
                            .map(|note| (note.message, note.line)),
                        public_lexical_note(source.as_bytes(), None)
                            .map(|note| (note.message, note.line))
                    );
                } else {
                    assert_eq!(
                        lua51_grammar_input_with_guard(source.as_bytes(), false, Some(&guard)),
                        lua51_grammar_input(source.as_bytes(), false)
                    );
                }
                let (steps, clocks) = guard.scan_counts_for_test();
                assert!(steps >= source.len());
                assert!(clocks < steps / 100 + 10, "{lexical}: {clocks}/{steps}");
                let control = OperationControl::default();
                let cancel = control.tracked_cancel_handle().unwrap();
                let observer = |at| {
                    if at == 101 {
                        cancel.cancel();
                    }
                };
                let mut guard = WorkerGuard::new(&control, 4 << 20);
                guard.observe_scan_for_test(&observer);
                if lexical {
                    let _ = public_lexical_note(source.as_bytes(), Some(&guard));
                } else {
                    let _ = lua51_grammar_input_with_guard(source.as_bytes(), false, Some(&guard));
                }
                assert!(!guard.check());
                assert!(guard.scan_counts_for_test().0 <= 101 + 255);
            }
        }
    });
}

#[test]
fn lua_scanner_last_byte_cancellation_cannot_escape_as_complete_facts() {
    on_stack(|| {
        let raw = b"Call()";
        for lexical in [false, true] {
            let control = OperationControl::default();
            let probe = WorkerGuard::new(&control, 4 << 20);
            if lexical {
                let _ = public_lexical_note(raw, Some(&probe));
            } else {
                let _ = lua51_grammar_input_with_guard(raw, false, Some(&probe));
            }
            let last = probe.scan_counts_for_test().0;
            assert!(last > 0 && last < 255);
            let control = OperationControl::default();
            let cancel = control.tracked_cancel_handle().unwrap();
            let observer = |at| {
                if at == last {
                    cancel.cancel();
                }
            };
            let mut guard = WorkerGuard::new(&control, 4 << 20);
            guard.observe_scan_for_test(&observer);
            if lexical {
                let document = SyntaxDocument::new_public(raw).unwrap();
                assert!(document.facts_with_guard(true, Some(&guard)).is_none());
            } else {
                assert!(generated_facts_with_worker_guard(raw, true, &guard).is_none());
            }
            assert!(!guard.check());
            assert_eq!(
                guard.scan_counts_for_test().0,
                last,
                "exit observation catches cancellation without another byte block"
            );
        }
    });
}
