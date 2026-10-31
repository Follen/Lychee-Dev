use super::*;
use lycheedev_core::control::{OperationControl, TrackedCancellation};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const FILES: &[(&str, &str)] = &[
    (
        "query-roles.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/query-roles.lua"),
    ),
    (
        "query-captures.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/query-captures.lua"),
    ),
    (
        "query-load-a.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/query-load-a.lua"),
    ),
    (
        "query-load-b.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/query-load-b.lua"),
    ),
    (
        "scope.lua",
        include_str!("../../../../tests/fixtures/source-native/golden/scope.lua"),
    ),
    (
        "writes-redeclaration.lua",
        include_str!("../../../../tests/fixtures/source-native/golden/writes-redeclaration.lua"),
    ),
    (
        "lexical-negative.lua",
        include_str!(
            "../../../../tests/fixtures/source-native/wow/native/query-lexical-negative.lua"
        ),
    ),
    (
        "query-loop-scopes.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/query-loop-scopes.lua"),
    ),
    (
        "query-vararg-assignment.lua",
        include_str!(
            "../../../../tests/fixtures/source-native/wow/native/query-vararg-assignment.lua"
        ),
    ),
    (
        "query-empty-function.lua",
        include_str!(
            "../../../../tests/fixtures/source-native/wow/native/query-empty-function.lua"
        ),
    ),
];
fn text(file: &str) -> &'static str {
    FILES.iter().find(|(name, _)| *name == file).unwrap().1
}
fn site(file: &str, marker: &str, token: &str) -> Site {
    let source = text(file);
    let at = source.find(marker).unwrap();
    assert!(
        !source[at + marker.len()..].contains(marker),
        "ambiguous independent source marker"
    );
    let start = at + marker.find(token).unwrap();
    Site {
        file: file.into(),
        start: start as u32,
        end: (start + token.len()) as u32,
    }
}
fn control() -> OperationControl {
    OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        TrackedCancellation::new(),
    )
}
fn prepared(files: &[&str], ordered: &[&str], c: &OperationControl) -> PreparedSnapshot {
    let mut config = Emmyrc::default();
    config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    let input = SourceInputSpec {
        files: files
            .iter()
            .map(|file| {
                let raw = text(file);
                SourceFileInput {
                    path: (*file).into(),
                    original_utf8: raw.into(),
                    analysis_utf8: raw.into(),
                    content_sha256: hash(raw.as_bytes()),
                    map: SourceMap::Identity,
                }
            })
            .collect(),
        workspace_root: std::env::current_dir().unwrap().join("query-contract"),
        source_identity: json!({"fixture":"source-query-contract"}),
        phase: "after_load".into(),
        ordered_occurrences: ordered.iter().map(|s| (*s).into()).collect(),
        config,
        host: TrustedHostEnvironment::from_host_material(
            "closed-query-contract".into(),
            true,
            vec![],
            vec![],
            c,
        )
        .unwrap(),
        luac: LuacIdentity {
            version: "5.1.5".into(),
            platform: "windows-amd64".into(),
            executable_sha256: "a".repeat(64),
            executable_bytes: 1,
        },
        limits: SourceLimits::default(),
    };
    prepare(SourceInputs::admit(input, c).unwrap(), c).unwrap()
}
fn lexical(result: &DefinitionResult) -> Option<&Site> {
    match &result.lexical {
        QueryFact::Known { value } => Some(&value.site),
        _ => None,
    }
}
fn selected_function(result: &DefinitionResult) -> Option<&Site> {
    match &result.selected {
        QueryFact::Known { value } => value.function_declaration.as_ref(),
        _ => None,
    }
}

#[test]
fn syntax_definitions_preserve_scope_redeclaration_initializer_and_ignore_fake_calls() {
    let c = control();
    let p = prepared(
        &[
            "scope.lua",
            "writes-redeclaration.lua",
            "lexical-negative.lua",
        ],
        &[],
        &c,
    );
    let query = p.source_queries(&c).unwrap();
    for (file, marker, token, declaration, declared_token) in [
        (
            "scope.lua",
            "local captured = value",
            "value",
            "local value = 1",
            "value",
        ),
        (
            "scope.lua",
            "local inner_read = value",
            "value",
            "        local value = 2",
            "value",
        ),
        (
            "scope.lua",
            "return value, captured",
            "value",
            "local value = 1",
            "value",
        ),
        (
            "writes-redeclaration.lua",
            "reused = 1",
            "reused",
            "local reused = 0",
            "reused",
        ),
        (
            "writes-redeclaration.lua",
            "local old = reused",
            "reused",
            "local reused = 0",
            "reused",
        ),
        (
            "writes-redeclaration.lua",
            "reused = 3",
            "reused",
            "local reused = 2",
            "reused",
        ),
        (
            "writes-redeclaration.lua",
            "return old, reused",
            "reused",
            "local reused = 2",
            "reused",
        ),
        (
            "lexical-negative.lua",
            "local z=value",
            "value",
            "for value=1,2 do",
            "value",
        ),
        (
            "lexical-negative.lua",
            " return value",
            "value",
            "local function f(value)",
            "value",
        ),
    ] {
        let result = query.definitions(&site(file, marker, token)).unwrap();
        assert_eq!(
            lexical(&result),
            Some(&site(file, declaration, declared_token)),
            "{file}: {marker}"
        );
        assert!(matches!(result.selected, QueryFact::NotApplicable));
    }
    let mut rhs = site("lexical-negative.lua", "local value=value", "value");
    rhs.start += 6;
    rhs.end += 6;
    assert_eq!(
        lexical(&query.definitions(&rhs).unwrap()),
        Some(&site("lexical-negative.lua", "local value=1", "value"))
    );
    assert!(
        query
            .calls(QueryTarget::All, PageRequest::default())
            .unwrap()
            .rows
            .is_empty()
    );
}

#[test]
fn loop_headers_use_outer_bindings_and_bodies_use_their_own_declarations() {
    let c = control();
    let p = prepared(&["query-loop-scopes.lua"], &[], &c);
    let query = p.source_queries(&c).unwrap();
    for (marker, expression, offset, declaration) in [
        (
            "for value = value, value + 2 do",
            "= value",
            2,
            "local value = 10",
        ),
        (
            "for value = value, value + 2 do",
            "value + 2",
            0,
            "local value = 10",
        ),
        (
            "local numericBody = value",
            "value",
            0,
            "for value = value, value + 2 do",
        ),
        ("local numericAfter = value", "value", 0, "local value = 10"),
        (
            "for value in Iterator(value) do",
            "Iterator(value)",
            9,
            "local value = 10",
        ),
        (
            "local genericBody = value",
            "value",
            0,
            "for value in Iterator(value) do",
        ),
        ("local genericAfter = value", "value", 0, "local value = 10"),
        (
            "    for value = value, value + 1 do",
            "= value",
            2,
            "for value = 1, 2 do",
        ),
        (
            "    for value = value, value + 1 do",
            "value + 1",
            0,
            "for value = 1, 2 do",
        ),
        (
            "local nestedBody = value",
            "value",
            0,
            "    for value = value, value + 1 do",
        ),
        (
            "local afterInner = value",
            "value",
            0,
            "for value = 1, 2 do",
        ),
        ("local nestedAfter = value", "value", 0, "local value = 10"),
        ("    return value", "value", 0, "local value = 10"),
    ] {
        let mut selected = site("query-loop-scopes.lua", marker, expression);
        selected.start += offset;
        selected.end = selected.start + 5;
        assert_eq!(
            lexical(&query.definitions(&selected).unwrap()),
            Some(&site("query-loop-scopes.lua", declaration, "value")),
            "{marker}: {expression}"
        );
    }
    assert_eq!(
        lexical(
            &query
                .definitions(&site(
                    "query-loop-scopes.lua",
                    "if n then return Recursive(false) end",
                    "Recursive"
                ))
                .unwrap()
        ),
        Some(&site(
            "query-loop-scopes.lua",
            "local function Recursive(n)",
            "Recursive"
        ))
    );
}

#[test]
fn selected_function_aliases_members_and_exact_calls_remain_distinct() {
    let c = control();
    let p = prepared(&["query-roles.lua"], &["query-roles.lua"], &c);
    let graph = p
        .build_plan(
            "roles",
            "after_load",
            LoadPlan::Known(p.inputs.ordered.clone()),
            None,
            None,
            &c,
        )
        .unwrap();
    let syntax = p.source_queries(&c).unwrap();
    let query = syntax
        .with_execution(&graph, QuerySelector::LoadOccurrence { occurrence: 0 }, &c)
        .unwrap();
    let original = site("query-roles.lua", "local function Original(v)", "Original");
    let replacement = site(
        "query-roles.lua",
        "local function Replacement(v)",
        "Replacement",
    );
    for (marker, token, declared, name, original_selected) in [
        (
            "local alias = Original",
            "Original",
            Some("local function Original(v)"),
            "Original",
            true,
        ),
        (
            "local saved = alias",
            "alias",
            Some("local alias = Original"),
            "alias",
            true,
        ),
        (
            "local functionValueRead = saved",
            "saved",
            Some("local saved = alias"),
            "saved",
            true,
        ),
        (
            "local firstCall = saved(1)",
            "saved",
            Some("local saved = alias"),
            "saved",
            true,
        ),
        (
            "local secondCall = alias(\"two\")",
            "alias",
            Some("local alias = Original"),
            "alias",
            false,
        ),
        (
            "local shadowCall = saved(3)",
            "saved",
            Some("    local saved = Replacement"),
            "saved",
            false,
        ),
        (
            "local memberSaved = object.run",
            "object.run",
            None,
            "",
            true,
        ),
        (
            "local savedMemberCall = memberSaved(4)",
            "memberSaved",
            Some("local memberSaved = object.run"),
            "memberSaved",
            true,
        ),
        (
            "local currentMemberCall = object.run(5)",
            "object.run",
            None,
            "",
            false,
        ),
        (
            "local otherMemberCall = other.run(6)",
            "other.run",
            None,
            "",
            false,
        ),
    ] {
        let result = query
            .definitions(&site("query-roles.lua", marker, token))
            .unwrap();
        assert_eq!(
            lexical(&result),
            declared.map(|m| site("query-roles.lua", m, name)).as_ref()
        );
        assert_eq!(
            selected_function(&result),
            Some(if original_selected {
                &original
            } else {
                &replacement
            })
        );
    }
    assert!(matches!(
        query
            .definitions(&site(
                "query-roles.lua",
                "local dynamicMember = object[unknownKey]",
                "object[unknownKey]"
            ))
            .unwrap()
            .selected,
        QueryFact::Unavailable { .. }
    ));
    let QueryFact::Known { value } = query
        .definitions(&site(
            "query-roles.lua",
            "local functionValueRead = saved",
            "saved",
        ))
        .unwrap()
        .selected
    else {
        panic!("selected value");
    };
    let target = QueryTarget::Value(value.value_id);
    assert_eq!(
        query
            .calls(target, PageRequest::default())
            .unwrap()
            .rows
            .iter()
            .map(|row| &row.site)
            .collect::<Vec<_>>(),
        [
            &site("query-roles.lua", "local firstCall = saved(1)", "saved(1)"),
            &site(
                "query-roles.lua",
                "local savedMemberCall = memberSaved(4)",
                "memberSaved(4)"
            )
        ]
    );
}

#[test]
fn capture_queries_require_an_occurrence_and_keep_changed_writes_and_factory_cells() {
    let c = control();
    let p = prepared(&["query-captures.lua"], &["query-captures.lua"], &c);
    let graph = p
        .build_plan(
            "captures",
            "after_load",
            LoadPlan::Known(p.inputs.ordered.clone()),
            None,
            None,
            &c,
        )
        .unwrap();
    let syntax = p.source_queries(&c).unwrap();
    let body = site(
        "query-captures.lua",
        "local function Read() return value end",
        "value",
    );
    assert!(matches!(
        syntax
            .with_execution(&graph, QuerySelector::AllOccurrences, &c)
            .unwrap()
            .definitions(&body)
            .unwrap()
            .selected,
        QueryFact::SelectorRequired { .. }
    ));
    let mut values = Vec::new();
    for (marker, origin, token) in [
        ("local first = Read()", "local value = 1", "1"),
        ("local second = Read()", "value = \"later\"", "\"later\""),
    ] {
        let call_site = site("query-captures.lua", marker, "Read()");
        let call = graph
            .calls
            .iter()
            .find(|call| call.site == call_site && call.context.starts_with("load:"))
            .unwrap();
        let frame = graph
            .native_invocations
            .as_ref()
            .unwrap()
            .frames
            .iter()
            .find(|frame| frame.call == Some(call.id))
            .unwrap();
        let result = syntax
            .with_execution(&graph, QuerySelector::Invocation { frame: frame.id }, &c)
            .unwrap()
            .definitions(&body)
            .unwrap();
        assert_eq!(
            lexical(&result),
            Some(&site("query-captures.lua", "local value = 1", "value"))
        );
        let QueryFact::Known { value } = result.selected else {
            panic!("captured value");
        };
        let expected = site("query-captures.lua", origin, token);
        assert_eq!(
            serde_json::to_value(&value.origin).unwrap(),
            json!({"file":expected.file,"start":expected.start,"end":expected.end})
        );
        values.push(value);
    }
    assert_ne!(values[0].writes, values[1].writes);
    assert_ne!(values[0].value_id, values[1].value_id);
    let declaration = site("query-captures.lua", "    local value = seed", "value");
    let cells: BTreeSet<_> = graph
        .native_invocations
        .as_ref()
        .unwrap()
        .capture_facts
        .iter()
        .filter(|capture| capture.declaration == declaration)
        .map(|capture| &capture.cell)
        .collect();
    assert_eq!(cells.len(), 2);
}

#[test]
fn selected_load_order_and_repeated_occurrences_do_not_invent_one_definition() {
    for (order, occurrence, known) in [
        (vec!["query-load-a.lua", "query-load-b.lua"], 0, false),
        (vec!["query-load-b.lua", "query-load-a.lua"], 1, true),
        (
            vec!["query-load-a.lua", "query-load-b.lua", "query-load-a.lua"],
            0,
            false,
        ),
        (
            vec!["query-load-a.lua", "query-load-b.lua", "query-load-a.lua"],
            2,
            true,
        ),
    ] {
        let c = control();
        let p = prepared(&["query-load-a.lua", "query-load-b.lua"], &order, &c);
        let graph = p
            .build_plan(
                "loads",
                "after_load",
                LoadPlan::Known(p.inputs.ordered.clone()),
                None,
                None,
                &c,
            )
            .unwrap();
        let syntax = p.source_queries(&c).unwrap();
        let input = site("query-load-a.lua", "local saved = Shared", "Shared");
        let result = syntax
            .with_execution(&graph, QuerySelector::LoadOccurrence { occurrence }, &c)
            .unwrap()
            .definitions(&input)
            .unwrap();
        if known {
            assert_eq!(
                selected_function(&result),
                Some(&site(
                    "query-load-b.lua",
                    "function Shared(value)",
                    "Shared"
                ))
            );
        } else {
            assert!(matches!(result.selected, QueryFact::KnownAbsent { .. }));
        }
        if occurrence == 2 {
            assert!(matches!(
                syntax
                    .with_execution(&graph, QuerySelector::AllOccurrences, &c)
                    .unwrap()
                    .definitions(&input)
                    .unwrap()
                    .selected,
                QueryFact::SelectorRequired { .. }
            ));
        }
    }
}

#[test]
fn call_cursors_bind_page_limits_target_selector_and_owner() {
    let c = control();
    let p = prepared(&["query-roles.lua"], &["query-roles.lua"], &c);
    let graph = p
        .build_plan(
            "paging",
            "after_load",
            LoadPlan::Known(vec!["query-roles.lua".into()]),
            None,
            None,
            &c,
        )
        .unwrap();
    let syntax = p.source_queries(&c).unwrap();
    let query = syntax
        .with_execution(&graph, QuerySelector::LoadOccurrence { occurrence: 0 }, &c)
        .unwrap();
    let full = query
        .calls(QueryTarget::All, PageRequest::default())
        .unwrap();
    let first = query
        .calls(
            QueryTarget::All,
            PageRequest {
                limit: 2,
                bytes: 65536,
                cursor: None,
            },
        )
        .unwrap();
    let cursor = first.next_cursor.clone().unwrap();
    let mut joined = first.rows.clone();
    let mut next = first.next_cursor.clone();
    for _ in 0..16 {
        let Some(cursor) = next.take() else { break };
        let page = query
            .calls(
                QueryTarget::All,
                PageRequest {
                    limit: 2,
                    bytes: 65536,
                    cursor: Some(cursor),
                },
            )
            .unwrap();
        assert!(!page.rows.is_empty() || !page.has_more);
        joined.extend(page.rows);
        next = page.next_cursor;
    }
    assert!(next.is_none());
    assert_eq!(
        joined.iter().map(|r| &r.id).collect::<Vec<_>>(),
        full.rows.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    assert_eq!(
        joined.iter().map(|r| &r.id).collect::<BTreeSet<_>>().len(),
        joined.len()
    );
    let repeated = query
        .calls(
            QueryTarget::All,
            PageRequest {
                limit: 2,
                bytes: 65536,
                cursor: None,
            },
        )
        .unwrap();
    assert_eq!(
        repeated.rows.iter().map(|r| &r.id).collect::<Vec<_>>(),
        first.rows.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    for result in [
        query.calls(
            QueryTarget::All,
            PageRequest {
                limit: 3,
                bytes: 65536,
                cursor: Some(cursor.clone()),
            },
        ),
        query.calls(
            QueryTarget::Site(site(
                "query-roles.lua",
                "local firstCall = saved(1)",
                "saved(1)",
            )),
            PageRequest {
                limit: 2,
                bytes: 65536,
                cursor: Some(cursor.clone()),
            },
        ),
        syntax
            .with_execution(&graph, QuerySelector::AllOccurrences, &c)
            .unwrap()
            .calls(
                QueryTarget::All,
                PageRequest {
                    limit: 2,
                    bytes: 65536,
                    cursor: Some(cursor.clone()),
                },
            ),
        query.calls(
            QueryTarget::All,
            PageRequest {
                limit: 2,
                bytes: 65535,
                cursor: Some(cursor.clone()),
            },
        ),
    ] {
        assert_eq!(result.err().unwrap().exit_code, 4);
    }
    let foreign = prepared(&["query-roles.lua"], &["query-roles.lua"], &c);
    assert_eq!(
        foreign
            .source_queries(&c)
            .unwrap()
            .calls(
                QueryTarget::All,
                PageRequest {
                    limit: 2,
                    bytes: 65536,
                    cursor: Some(cursor)
                }
            )
            .err()
            .unwrap()
            .exit_code,
        4
    );
    assert_eq!(
        query
            .calls(
                QueryTarget::All,
                PageRequest {
                    limit: 2,
                    bytes: 1,
                    cursor: None
                }
            )
            .err()
            .unwrap()
            .code,
        "source.native_query_record_too_large"
    );
    c.tracked_cancel_handle().unwrap().cancel();
    let error = query
        .definitions(&site(
            "query-roles.lua",
            "local functionValueRead = saved",
            "saved",
        ))
        .err()
        .unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
}

#[test]
fn syntax_call_paging_covers_both_files_in_exact_order() {
    let c = control();
    let p = prepared(&["query-captures.lua", "query-roles.lua"], &[], &c);
    let query = p.source_queries(&c).unwrap();
    let full = query
        .calls(
            QueryTarget::All,
            PageRequest {
                limit: 200,
                bytes: 1048576,
                cursor: None,
            },
        )
        .unwrap();
    assert!(!full.has_more);
    assert_eq!(
        full.rows
            .iter()
            .map(|row| row.site.file.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["query-captures.lua", "query-roles.lua"])
    );
    let mut rows = Vec::new();
    let mut next = None;
    for _ in 0..32 {
        let page = query
            .calls(
                QueryTarget::All,
                PageRequest {
                    limit: 2,
                    bytes: 65536,
                    cursor: next,
                },
            )
            .unwrap();
        assert!(!page.has_more || !page.rows.is_empty());
        rows.extend(page.rows);
        next = page.next_cursor;
        if next.is_none() {
            break;
        }
    }
    assert!(next.is_none());
    assert_eq!(
        serde_json::to_value(rows).unwrap(),
        serde_json::to_value(full.rows).unwrap()
    );
}

#[test]
fn open_vararg_slots_remain_unknown_while_the_fixed_prefix_is_retained() {
    let c = control();
    let p = prepared(
        &["query-vararg-assignment.lua"],
        &["query-vararg-assignment.lua"],
        &c,
    );
    let graph = p
        .build_plan(
            "varargs",
            "after_load",
            LoadPlan::Known(p.inputs.ordered.clone()),
            None,
            None,
            &c,
        )
        .unwrap();
    let syntax = p.source_queries(&c).unwrap();
    let query = syntax
        .with_execution(&graph, QuerySelector::LoadOccurrence { occurrence: 0 }, &c)
        .unwrap();
    for (marker, token) in [
        ("local observedFirst = first", "first"),
        ("local observedSecond = second", "second"),
        ("local observedThird = third", "third"),
        (
            "local observedShiftedSecond = shiftedSecond",
            "shiftedSecond",
        ),
        ("local observedShiftedThird = shiftedThird", "shiftedThird"),
        (
            "local observedAssignedFirst = assignedFirst",
            "assignedFirst",
        ),
        (
            "local observedAssignedSecond = assignedSecond",
            "assignedSecond",
        ),
        (
            "local observedAssignedThird = assignedThird",
            "assignedThird",
        ),
    ] {
        assert!(
            matches!(query.definitions(&site("query-vararg-assignment.lua", marker, token)).unwrap().selected, QueryFact::Unavailable { reason } if reason == "variadic_argument_slot_unavailable")
        );
    }
    let QueryFact::Known { value } = query
        .definitions(&site(
            "query-vararg-assignment.lua",
            "local observedShiftedFirst = shiftedFirst",
            "shiftedFirst",
        ))
        .unwrap()
        .selected
    else {
        panic!("fixed prefix");
    };
    assert_eq!(
        value.origin.source(),
        Some(&site(
            "query-vararg-assignment.lua",
            "local shiftedFirst, shiftedSecond, shiftedThird = 7, ...",
            "7"
        ))
    );
    for name in ["ReturnTail", "CallTail"] {
        let source = text("query-vararg-assignment.lua");
        let at = source.find(&format!("function {name}")).unwrap();
        let line = source[..at].bytes().filter(|byte| *byte == b'\n').count() + 1;
        let EntryResolution::Unique { candidate } = p
            .entry_candidates("query-vararg-assignment.lua", line, &c)
            .unwrap()
        else {
            panic!("unique entry");
        };
        let request = EntryRequest {
            candidate: candidate.id,
            load_occurrence: None,
            creation_context: None,
            arguments: EntryArguments::Unspecified,
        };
        assert!(
            p.build_plan(
                "open-tail",
                "after_load",
                LoadPlan::Known(vec!["query-vararg-assignment.lua".into()]),
                Some(request),
                None,
                &c
            )
            .err()
            .unwrap()
            .to_string()
            .contains("native invocation stopped at variadic argument sequence boundary")
        );
    }
}

#[test]
fn empty_function_results_pad_scalar_slots_but_tail_calls_keep_zero_arity() {
    let c = control();
    let p = prepared(
        &["query-empty-function.lua"],
        &["query-empty-function.lua"],
        &c,
    );
    let graph = p
        .build_plan(
            "empty",
            "after_load",
            LoadPlan::Known(p.inputs.ordered.clone()),
            None,
            None,
            &c,
        )
        .unwrap();
    let facts = graph.native_invocations.as_ref().unwrap();
    for (marker, call_text) in [
        ("one(1)", "one(1)"),
        ("anonymous()", "anonymous()"),
        ("\nobject:empty()\n", "object:empty()"),
    ] {
        let selected = site("query-empty-function.lua", marker, call_text);
        let call = graph
            .calls
            .iter()
            .find(|call| call.site == selected && call.context.starts_with("load:"))
            .unwrap();
        let frame = facts
            .frames
            .iter()
            .find(|frame| frame.call == Some(call.id))
            .unwrap();
        assert!(frame.return_facts.is_empty());
        assert_eq!(frame.return_paths.len(), 1);
        assert!(
            frame
                .return_paths
                .iter()
                .all(|path| matches!(path.outcome, NativeControl::Continue))
        );
        assert!(frame.effects_complete);
        assert!(call.effect_write_versions.is_empty());
        assert_eq!(
            graph.functions[frame.function].body_check_coverage,
            "return_expressions_only"
        );
    }
    let syntax = p.source_queries(&c).unwrap();
    let query = syntax
        .with_execution(&graph, QuerySelector::LoadOccurrence { occurrence: 0 }, &c)
        .unwrap();
    for (marker, token) in [
        ("local observedFirst = first", "first"),
        ("local observedSecond = second", "second"),
        ("local observedScalar = scalar", "scalar"),
        ("local observedExisting = existing", "existing"),
        ("local observedTailFirst = tailFirst", "tailFirst"),
        ("local observedTailSecond = tailSecond", "tailSecond"),
        ("local observedTailArgA = tailArgA", "tailArgA"),
        ("local observedTailArgB = tailArgB", "tailArgB"),
        ("local observedScalarArgA = scalarArgA", "scalarArgA"),
    ] {
        assert!(matches!(
            query
                .definitions(&site("query-empty-function.lua", marker, token))
                .unwrap()
                .selected,
            QueryFact::KnownAbsent { .. }
        ));
    }
    let QueryFact::Known { value } = query
        .definitions(&site(
            "query-empty-function.lua",
            "local observedScalarArgB = scalarArgB",
            "scalarArgB",
        ))
        .unwrap()
        .selected
    else {
        panic!("fixed following argument");
    };
    assert_eq!(
        value.origin.source(),
        Some(&site(
            "query-empty-function.lua",
            "local scalarArgA, scalarArgB = sink(one(7), 9)",
            "9"
        ))
    );
    for (text, arity) in [("sink(one(6))", 0), ("sink(one(7), 9)", 2)] {
        assert_eq!(
            graph
                .calls
                .iter()
                .find(
                    |call| call.site == site("query-empty-function.lua", text, text)
                        && call.context.starts_with("load:")
                )
                .unwrap()
                .arguments
                .len(),
            arity
        );
    }
    let forward = graph
        .calls
        .iter()
        .find(|call| {
            call.site
                == site(
                    "query-empty-function.lua",
                    "local tailFirst, tailSecond = forward()",
                    "forward()",
                )
                && call.context.starts_with("load:")
        })
        .unwrap();
    let frame = facts
        .frames
        .iter()
        .find(|frame| frame.call == Some(forward.id))
        .unwrap();
    assert!(frame.return_facts.is_empty());
    assert_eq!(frame.return_paths.len(), 1);
    assert!(
        matches!(&frame.return_paths[0].outcome, NativeControl::Return(values) if values.is_empty())
    );
    let EntryResolution::Unique { candidate } = p
        .entry_candidates("query-empty-function.lua", 2, &c)
        .unwrap()
    else {
        panic!("unique empty entry");
    };
    let graph = p
        .build_plan(
            "empty-entry",
            "after_load",
            LoadPlan::Known(vec!["query-empty-function.lua".into()]),
            Some(EntryRequest {
                candidate: candidate.id,
                load_occurrence: None,
                creation_context: None,
                arguments: EntryArguments::Unspecified,
            }),
            None,
            &c,
        )
        .unwrap();
    let entry = &graph.native_invocations.as_ref().unwrap().entries[0];
    assert!(entry.return_facts.is_empty());
    assert!(entry.effects_complete);
    assert!(entry.effect_write_versions.is_empty());
}
