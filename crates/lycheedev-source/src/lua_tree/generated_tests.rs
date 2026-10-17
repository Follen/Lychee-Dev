//! Exact pre-change generated traversal retained only as a differential oracle.
//! Source: 5766dccc1fec7d20f5d8120bca7dcd79eed726c4 lua_tree.rs.
use super::*;
use crate::facts::ParseMode;
use std::time::Instant;

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
fn reference(raw: &[u8]) -> Facts {
    match SyntaxDocument::new_public(raw) {
        Ok(document) => document.generated_reference_facts(true, None).unwrap(),
        Err(_) => crate::facts::lua(raw, true),
    }
}
#[test]
fn generated_guard_and_linear_views_match_all_20_complete_reference_cases() {
    on_stack(|| {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/rust-parity/source-facts-generated/cases.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["oracleCommit"],
            "3766f9cacb52e93a803a626187e2a1d54a445468"
        );
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 20);
        for case in cases {
            let raw = case["source"].as_str().unwrap().as_bytes();
            let id = case["id"].as_str().unwrap();
            let expected = reference(raw);
            let reference_row = serde_json::json!({
                "id": case["id"], "path": case["path"], "facts": &expected, "error": null
            });
            println!("SOURCE_FACT_RUST_REFERENCE_JSON {reference_row}");
            let actual = crate::facts::lua(raw, true);
            same_complete(&actual, &expected, id);
            let worker = crate::facts::for_worker(
                raw,
                ParseMode::LuaGenerated,
                &OperationControl::default(),
                4 << 20,
            )
            .expect("small full generated fixture must fit");
            same_complete(&worker, &expected, id);
        }
    });
}
#[test]
fn generated_guard_keeps_original_29_lua_boundary_outputs() {
    on_stack(|| {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/rust-parity/source-facts-malformed/cases.json"
        ))
        .unwrap();
        let mut count = 0;
        for case in fixture["cases"].as_array().unwrap() {
            if !case["path"].as_str().unwrap().ends_with(".lua") {
                continue;
            }
            count += 1;
            let raw = case["source"].as_str().unwrap().as_bytes();
            let expected = reference(raw);
            let worker = crate::facts::for_worker(
                raw,
                ParseMode::LuaGenerated,
                &OperationControl::default(),
                4 << 20,
            )
            .unwrap();
            same_complete(&worker, &expected, case["id"].as_str().unwrap());
        }
        assert_eq!(count, 29);
    });
}
#[test]
fn generated_fixed_views_scan_each_direct_field_once_and_keep_invalid_first_match() {
    let raw = br#"local T={Type=false, Type='System', ['Name']=false, Name='Later', Nilable=false, Nilable=true, Functions={}, Events={}, Tables={}, Structures={}, Enumerations={}, ScriptObjects={}, Callbacks={}, Arguments={}, Returns={}, Payload={}}"#;
    let document = SyntaxDocument::new_public(raw).unwrap();
    let mut pending = vec![document.root()];
    let table = loop {
        let node = pending.pop().unwrap();
        if node.kind() == "table_constructor" {
            break node;
        }
        pending.extend(children(node).into_iter().rev());
    };
    let fields = generated::LiteralFields::read(raw, table, generated::DOC_KEYS, None).unwrap();
    let mut cursor = table.walk();
    assert_eq!(fields.visited, table.named_children(&mut cursor).count());
    assert_eq!(fields.values[0].unwrap().kind(), "false");
    assert_eq!(fields.values[1].unwrap().kind(), "false");
    for (ordinal, key) in generated::DOC_KEYS.into_iter().enumerate() {
        assert_eq!(
            fields.values[ordinal],
            reference_field(raw, table, key),
            "{key}"
        );
    }
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
    same_complete(&facts, &reference(raw), "nested roots remain repeated");
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
        same_complete(
            &complete,
            &reference(raw.as_bytes()),
            "original complete fallback",
        );
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
        let expected = reference(raw.as_bytes());
        let actual = crate::facts::for_worker(
            raw.as_bytes(),
            ParseMode::LuaGenerated,
            &OperationControl::default(),
            4 << 20,
        )
        .unwrap();
        same_complete(
            &actual,
            &expected,
            "max dynamic-to-source ratio and invalid UTF8 byte replacement",
        );
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

impl SyntaxDocument {
    fn generated_reference_facts(
        &self,
        generated: bool,
        guard: Option<&WorkerGuard<'_>>,
    ) -> Option<Facts> {
        if !self.lua51_identifiers
            && let Some(note) = self.public_parse_note_with_guard(guard)
        {
            let mut facts = Facts::fresh();
            if !worker::charge(guard, note.message.capacity()) {
                return None;
            }
            worker::push(guard, &mut facts.diagnostics, note);
            return worker::check(guard).then_some(facts);
        }
        if !worker::charge(guard, "<file>".len()) {
            return None;
        }
        let mut scopes = Vec::new();
        if !worker::prepare_push(guard, &mut scopes) {
            return None;
        }
        scopes.push("<file>".into());
        let mut w = Walker {
            raw: &self.bytes,
            facts: Facts::fresh(),
            scopes,
            guard,
            api_roots: Vec::new(),
            rejected_syntax: false,
            generated,
            work: 0,
            depth: 0,
            source_lines: lua_line_starts(&self.bytes),
            lines: std::str::from_utf8(&self.bytes)
                .unwrap_or("")
                .split('\n')
                .collect(),
        };
        w.walk(self.tree.root_node());
        // Go rejects a malformed program as a whole, rather than publishing
        // declarations recovered by tree-sitter. The native tree stays intact.
        if self.tree.root_node().has_error() || w.rejected_syntax {
            w.facts.declarations.clear();
            w.facts.relationships.clear();
            if let Some(targets) = &mut w.facts.raw_relationship_targets {
                targets.clear();
            }
        } else {
            for root in std::mem::take(&mut w.api_roots) {
                w.reference_documentation(root, "");
            }
        }
        worker::check(guard).then_some(w.facts)
    }
}
fn reference_field<'a>(raw: &[u8], table: Node<'a>, key: &str) -> Option<Node<'a>> {
    for f in children(table).into_iter().filter(|f| f.kind() == "field") {
        let n = f.child_by_field_name("name");
        if n.is_some_and(|n| text(raw, n) == key || string(raw, n).as_deref() == Some(key)) {
            return f.child_by_field_name("value").map(unparenthesized);
        }
    }
    None
}
fn reference_table_text(raw: &[u8], table: Node<'_>, key: &str) -> String {
    reference_field(raw, table, key)
        .and_then(|n| string(raw, n))
        .unwrap_or_default()
}

impl<'tree> Walker<'_, 'tree> {
    fn reference_documentation(&mut self, n: Node<'_>, namespace: &str) {
        let kind = reference_table_text(self.raw, n, "Type").to_ascii_lowercase();
        let display = reference_table_text(self.raw, n, "Name");
        let mut namespace = namespace.to_owned();
        if kind == "system" {
            namespace = reference_table_text(self.raw, n, "Namespace");
            if namespace.is_empty() && display.starts_with("C_") {
                namespace = display.clone();
            }
            self.declare(n, display.clone(), "api-system", false, Some(display));
        } else if !display.is_empty()
            && [
                "function",
                "event",
                "structure",
                "enumeration",
                "scriptobject",
                "callback",
            ]
            .contains(&kind.as_str())
        {
            let qualified = if namespace.is_empty() {
                display
            } else {
                format!("{namespace}.{display}")
            };
            self.declare(
                n,
                qualified.clone(),
                &format!("api-{kind}"),
                false,
                Some(self.reference_signature(n, &qualified, &kind)),
            );
            if !namespace.is_empty() {
                self.link(n, namespace.clone(), qualified.clone(), "contains", "exact");
            }
            let literal_name = reference_table_text(self.raw, n, "LiteralName");
            if kind == "event" && !literal_name.is_empty() && literal_name != qualified {
                self.declare(
                    n,
                    literal_name.clone(),
                    "api-event",
                    false,
                    Some(self.reference_signature(n, &literal_name, &kind)),
                );
                self.link(n, literal_name, qualified, "event-name", "exact");
            }
        }
        for key in [
            "Functions",
            "Events",
            "Tables",
            "Structures",
            "Enumerations",
            "ScriptObjects",
            "Callbacks",
        ] {
            if let Some(list) = reference_field(self.raw, n, key) {
                for f in children(list) {
                    if let Some(t) = f.child_by_field_name("value")
                        && t.kind() == "table_constructor"
                    {
                        self.reference_documentation(t, &namespace);
                    }
                }
            }
        }
    }
    fn reference_signature(&self, n: Node<'_>, name: &str, kind: &str) -> String {
        let params = |key: &str| -> String {
            let Some(list) = reference_field(self.raw, n, key) else {
                return String::new();
            };
            children(list)
                .into_iter()
                .filter_map(|f| f.child_by_field_name("value"))
                .filter(|n| n.kind() == "table_constructor")
                .map(|p| {
                    let mut name = reference_table_text(self.raw, p, "Name");
                    let mut kind = reference_table_text(self.raw, p, "Type");
                    if name.is_empty() {
                        name = "<dynamic>".into();
                    }
                    if kind.is_empty() {
                        kind = "<dynamic>".into();
                    }
                    if reference_field(self.raw, p, "Nilable").is_some_and(|n| n.kind() == "true") {
                        name.push('?');
                    }
                    format!("{name}: {kind}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        match kind {
            "function" | "scriptobject" | "callback" => {
                let mut s = format!("{name}({})", params("Arguments"));
                let out = params("Returns");
                if !out.is_empty() {
                    s.push_str(&format!(" -> {out}"));
                }
                s
            }
            "event" => format!("{name}({})", params("Payload")),
            _ => name.into(),
        }
    }
}
