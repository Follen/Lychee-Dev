//! Fixed Go 3.2.0 EXPLAIN slice. Rust execution status: NOT_RUN.
use lycheedev_data::relational::{Limits, Program, QueryResult, Scalar as S, Source};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::atomic::AtomicBool, time::Instant};
fn inputs() -> BTreeMap<String, Source> {
    ["a", "b", "c"]
        .into_iter()
        .map(|name| {
            (
                name.into(),
                Source {
                    columns: vec!["ID".into(), "Value".into()],
                    // These rows cannot be evaluated, projected, indexed or width-checked.
                    rows: vec![vec![S::Float(f64::NAN)], vec![]],
                    complete: name != "b",
                },
            )
        })
        .collect()
}
fn query(sql: &str) -> lycheedev_core::Result<QueryResult> {
    Program::compile(sql)?.execute(&inputs(), &BTreeMap::new(), Limits::default(), None)
}
fn plan(sql: &str) -> Value {
    serde_json::to_value(query(sql).unwrap()).unwrap()["plan"].clone()
}
#[test]
fn explain_does_not_visit_rows_and_serializes_null_results() {
    let result =
        query("EXPLAIN SELECT a.ID FROM a LEFT JOIN b ON a.ID=b.ID ORDER BY a.ID LIMIT 2").unwrap();
    assert!(!result.complete);
    assert!(result.rows.is_empty() && result.columns.is_empty());
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({"columns":null,"rows":null,"plan":{
        "outputColumns":["ID"], "steps":[
            {"id":1,"parent":0,"operation":"select"},
            {"id":2,"parent":1,"operation":"input","detail":"a"},
            {"id":3,"parent":1,"operation":"adaptive-equality-join","detail":"left; bounded hash index when eligible; nested-loop fallback"},
            {"id":4,"parent":3,"operation":"input","detail":"b"},
            {"id":5,"parent":1,"operation":"order","detail":"bounded stable Top-K when offset+limit <= 1000000; otherwise stable merge sort; validates complete input"},
            {"id":6,"parent":1,"operation":"slice","detail":"offset then limit"}
        ]}})
    );
}
#[test]
fn explain_prefix_comments_case_with_and_trailing_statement_rules() {
    for sql in [
        "explain select ID from a;",
        "/* 界 */\n ExPlAiN /*x*/ SELECT ID FROM a",
        "EXPLAIN WITH x AS (SELECT ID FROM a) SELECT * FROM x",
    ] {
        assert!(query(sql).is_ok(), "{sql}");
    }
    for sql in [
        "ANALYZE SELECT ID FROM a",
        "EXPLAIN EXPLAIN SELECT ID FROM a",
        "EXPLAIN SELECT 1",
        "EXPLAIN SELECT ID FROM a; SELECT ID FROM b",
        "EXPLAIN VERBOSE SELECT ID FROM a",
    ] {
        assert!(query(sql).is_err(), "{sql}");
    }
}
#[test]
fn analyze_is_explicitly_unavailable_without_fabricated_measurements() {
    let error = match Program::compile("EXPLAIN ANALYZE SELECT ID FROM a") {
        Ok(_) => panic!("unverified ANALYZE must not publish"),
        Err(e) => e,
    };
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("relational.unsupported", 3)
    );
    assert!(plan("EXPLAIN SELECT ID FROM a").get("measured").is_none());
}
#[test]
fn explain_binds_unreachable_expressions_empty_relations_and_grouping() {
    for sql in [
        "EXPLAIN SELECT CASE WHEN FALSE THEN Missing ELSE ID END FROM a",
        "EXPLAIN SELECT ID FROM a WHERE Missing=1 LIMIT 0",
        "EXPLAIN SELECT ID,COUNT(*) FROM a",
        "EXPLAIN SELECT ID FROM a LEFT JOIN b ON b.Missing=a.ID",
        "EXPLAIN WITH RECURSIVE r AS (SELECT ID FROM a WHERE FALSE UNION ALL SELECT Missing FROM r) SELECT ID FROM r",
        "EXPLAIN SELECT LOWER(ID,Value) FROM a",
    ] {
        assert!(query(sql).is_err(), "{sql}");
    }
}
#[test]
fn explain_never_evaluates_projection_literals_but_checks_limit_values() {
    for sql in [
        "EXPLAIN SELECT 1e999 FROM a",
        "EXPLAIN SELECT 18446744073709551616 FROM a",
        "EXPLAIN SELECT ID+'x' FROM a",
        "EXPLAIN SELECT CASE WHEN FALSE THEN 1e999 ELSE ID END FROM a",
    ] {
        assert!(query(sql).is_ok(), "{sql}");
    }
    let p = Program::compile("EXPLAIN SELECT ID FROM a LIMIT :limit").unwrap();
    for value in [S::Int(-1), S::Float(1.5), S::Null] {
        assert!(
            p.execute(
                &inputs(),
                &BTreeMap::from([("limit".into(), value)]),
                Limits::default(),
                None
            )
            .is_err()
        );
    }
    assert!(query("EXPLAIN SELECT ID FROM a LIMIT 18446744073709551616").is_err());
}
#[test]
fn explain_cte_recursive_and_derived_names_never_leak_synthetic_inputs() {
    let p = plan(
        "EXPLAIN WITH RECURSIVE r AS (SELECT ID FROM a WHERE FALSE UNION ALL SELECT ID FROM r) SELECT d.ID FROM (SELECT ID FROM r) AS d",
    );
    assert_eq!(p["steps"][1]["operation"], "recursive-delta");
    assert_eq!(p["steps"][1]["detail"], "r");
    assert!(!p.to_string().contains("\\u0000"));
    assert_eq!(p["outputColumns"], json!(["ID"]));
    assert_eq!(
        plan("EXPLAIN WITH RECURSIVE x AS (SELECT ID FROM a) SELECT ID FROM x")["steps"][1]["operation"],
        "materialized-cte"
    );
}
#[test]
fn explain_union_suffix_belongs_to_last_real_branch() {
    let p = plan(
        "EXPLAIN SELECT ID FROM a UNION ALL SELECT ID FROM b UNION ALL SELECT ID FROM c ORDER BY ID LIMIT 2",
    );
    let steps = p["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 10);
    for (i, (parent, operation)) in [
        (0, "select"),
        (1, "input"),
        (1, "union-all"),
        (3, "select"),
        (4, "input"),
        (4, "union-all"),
        (6, "select"),
        (7, "input"),
        (7, "order"),
        (7, "slice"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(steps[i]["id"], i + 1);
        assert_eq!(steps[i]["parent"], parent);
        assert_eq!(steps[i]["operation"], operation);
    }
}
#[test]
fn explain_join_labels_describe_eligibility_not_a_fabricated_hash_use() {
    for (clause, operation, detail) in [
        (
            "JOIN b ON 1=1",
            "adaptive-equality-join",
            "inner; bounded hash index when eligible; nested-loop fallback",
        ),
        (
            "JOIN b ON a.ID=b.ID AND a.Value=b.Value",
            "nested-loop-join",
            "inner; materialize right",
        ),
        (
            "CROSS JOIN b",
            "nested-loop-join",
            "cross; materialize right",
        ),
    ] {
        let p = plan(&format!("EXPLAIN SELECT a.ID FROM a {clause}"));
        assert_eq!(p["steps"][2]["operation"], operation);
        assert_eq!(p["steps"][2]["detail"], detail);
    }
}
#[test]
fn explain_group_having_distinct_order_and_slice_are_ordered() {
    let p = plan(
        "EXPLAIN SELECT DISTINCT ID,COUNT(*) AS n FROM a WHERE ID>0 GROUP BY ID HAVING COUNT(*)>0 ORDER BY n LIMIT 2",
    );
    assert_eq!(
        p["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["operation"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "select", "input", "filter", "group", "distinct", "order", "slice"
        ]
    );
    assert_eq!(p["steps"][5]["detail"], "stable merge sort");
}
#[test]
fn explain_topk_description_is_conditional_and_offset_only_is_not_topk() {
    for (suffix, topk) in [
        ("ORDER BY ID LIMIT 1000001", true),
        ("ORDER BY ID,Value LIMIT 2", false),
        ("ORDER BY ID OFFSET 1", false),
    ] {
        let p = plan(&format!("EXPLAIN SELECT ID FROM a {suffix}"));
        let order = p["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["operation"] == "order")
            .unwrap();
        assert_eq!(order["detail"].as_str().unwrap().contains("Top-K"), topk);
    }
    assert!(
        !plan("EXPLAIN SELECT ID FROM a LIMIT 0")["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["operation"] == "order")
    );
}
#[test]
fn explain_duplicate_columns_budget_deadline_and_cancellation_fail_closed() {
    let p = Program::compile("EXPLAIN SELECT ID FROM a").unwrap();
    let mut bad = inputs();
    bad.get_mut("a").unwrap().columns = vec!["ID".into(), "id".into()];
    assert_eq!(
        p.execute(&bad, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    for limits in [
        Limits {
            work: 1,
            ..Limits::default()
        },
        Limits {
            memory_bytes: 1,
            ..Limits::default()
        },
        Limits {
            deadline: Instant::now(),
            ..Limits::default()
        },
    ] {
        assert!(
            p.execute(&inputs(), &BTreeMap::new(), limits, None)
                .is_err()
        );
    }
    let cancel = AtomicBool::new(true);
    assert_eq!(
        p.execute(
            &inputs(),
            &BTreeMap::new(),
            Limits::default(),
            Some(&cancel)
        )
        .unwrap_err()
        .exit_code,
        7
    );
}

#[test]
fn join_on_aggregates_fail_schema_preflight_for_plain_and_explain_with_any_rows() {
    for prefix in ["", "EXPLAIN "] {
        for condition in [
            "COUNT(*)>0",
            "COALESCE(SUM(a.ID),0)>0",
            "CASE WHEN FALSE THEN COUNT(*) ELSE 0 END=0",
            "a.ID IN (1,COUNT(*))",
        ] {
            for shape in 0..4 {
                let rows = match shape {
                    0 => vec![],
                    1 => vec![vec![S::Int(1), S::Int(2)]],
                    2 => vec![vec![]],
                    _ => vec![vec![S::Float(f64::NAN), S::Int(2)]],
                };
                let sources = ["a", "b"]
                    .into_iter()
                    .map(|name| {
                        (
                            name.into(),
                            Source {
                                columns: vec!["ID".into(), "Value".into()],
                                rows: rows.clone(),
                                complete: true,
                            },
                        )
                    })
                    .collect();
                let sql = format!("{prefix}SELECT a.ID FROM a JOIN b ON {condition}");
                let error = Program::compile(&sql)
                    .unwrap()
                    .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                    .unwrap_err();
                assert_eq!(
                    error.code, "relational.type_mismatch",
                    "{sql}; shape={shape}"
                );
            }
        }
    }
}

#[test]
fn all_join_conditions_bind_before_an_earlier_right_source_is_indexed() {
    for prefix in ["", "EXPLAIN "] {
        let error = query(&format!(
            "{prefix}SELECT a.ID FROM a JOIN b ON a.ID=b.ID JOIN c ON COUNT(*)>0"
        ))
        .unwrap_err();
        assert_eq!(error.code, "relational.type_mismatch");
    }
    // Removing an invalid ON aggregate must not make bad source rows valid.
    let error = query("SELECT a.ID FROM a JOIN b ON a.ID=b.ID").unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
}

#[test]
fn compound_duplicate_output_names_use_internal_ordinals_in_both_modes() {
    for prefix in ["", "EXPLAIN "] {
        for projection in ["ID,ID", "ID AS ID,Value AS ID"] {
            let sources = [
                (
                    "a".into(),
                    Source {
                        columns: vec!["ID".into(), "Value".into()],
                        rows: vec![vec![S::Int(1), S::Int(11)]],
                        complete: true,
                    },
                ),
                (
                    "b".into(),
                    Source {
                        columns: vec!["ID".into(), "Value".into()],
                        rows: vec![vec![S::Int(2), S::Int(22)]],
                        complete: true,
                    },
                ),
            ]
            .into_iter()
            .collect();
            let sql =
                format!("{prefix}SELECT {projection} FROM a UNION ALL SELECT {projection} FROM b");
            let result = Program::compile(&sql)
                .unwrap()
                .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                .unwrap();
            if prefix.is_empty() {
                assert_eq!(result.columns, ["ID", "ID"]);
                let expected = if projection == "ID,ID" {
                    vec![vec![S::Int(1), S::Int(1)], vec![S::Int(2), S::Int(2)]]
                } else {
                    vec![vec![S::Int(1), S::Int(11)], vec![S::Int(2), S::Int(22)]]
                };
                assert_eq!(
                    result.rows, expected,
                    "Ordinals must preserve distinct values behind duplicate names"
                );
            } else {
                assert_eq!(result.plan.unwrap().output_columns, ["ID", "ID"]);
                assert!(result.rows.is_empty());
            }
        }
    }
}

#[test]
fn compound_duplicate_name_ordering_stays_ambiguous_with_empty_inputs() {
    for prefix in ["", "EXPLAIN "] {
        for empty in [false, true] {
            let sources = ["a", "b"]
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        Source {
                            columns: vec!["ID".into(), "Value".into()],
                            rows: if empty {
                                vec![]
                            } else {
                                vec![vec![S::Int(1), S::Int(2)]]
                            },
                            complete: true,
                        },
                    )
                })
                .collect();
            let sql = format!(
                "{prefix}SELECT ID,Value AS ID FROM a UNION ALL SELECT ID,Value FROM b ORDER BY ID"
            );
            let error = Program::compile(&sql)
                .unwrap()
                .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                .unwrap_err();
            assert_eq!(
                error.code, "relational.invalid_binding",
                "{sql}; empty={empty}"
            );
        }
    }
}

#[test]
fn compound_ordinal_projection_keeps_unique_ordering_and_bounded_slice() {
    let sources = [
        (
            "a".into(),
            Source {
                columns: vec!["ID".into(), "Value".into()],
                rows: vec![vec![S::Int(1), S::Int(20)]],
                complete: true,
            },
        ),
        (
            "b".into(),
            Source {
                columns: vec!["ID".into(), "Value".into()],
                rows: vec![vec![S::Int(2), S::Int(10)]],
                complete: true,
            },
        ),
    ]
    .into_iter()
    .collect();
    for (suffix, expected) in [
        (
            "ORDER BY Sort LIMIT 1",
            vec![vec![S::Int(2), S::Int(10), S::Int(10)]],
        ),
        (
            "LIMIT 1 OFFSET 1",
            vec![vec![S::Int(2), S::Int(10), S::Int(10)]],
        ),
    ] {
        let sql = format!(
            "SELECT ID AS Same,Value AS Same,Value AS Sort FROM a UNION ALL SELECT ID,Value,Value FROM b {suffix}"
        );
        let result = Program::compile(&sql)
            .unwrap()
            .execute(&sources, &BTreeMap::new(), Limits::default(), None)
            .unwrap();
        assert_eq!(result.columns, ["Same", "Same", "Sort"]);
        assert_eq!(result.rows, expected);
        let explained = Program::compile(&format!("EXPLAIN {sql}"))
            .unwrap()
            .execute(&sources, &BTreeMap::new(), Limits::default(), None)
            .unwrap();
        assert_eq!(
            explained.plan.unwrap().output_columns,
            ["Same", "Same", "Sort"]
        );
    }
}

#[test]
fn join_on_schema_errors_keep_expression_traversal_order() {
    for prefix in ["", "EXPLAIN "] {
        for (condition, code) in [
            ("a.Missing=COUNT(*)", "relational.invalid_binding"),
            ("COUNT(*)=a.Missing", "relational.type_mismatch"),
        ] {
            let error =
                query(&format!("{prefix}SELECT a.ID FROM a JOIN b ON {condition}")).unwrap_err();
            assert_eq!(error.code, code);
        }
    }
}

#[test]
fn compound_ordinal_outputs_reject_aggregate_ordering_in_both_modes() {
    for prefix in ["", "EXPLAIN "] {
        for empty in [false, true] {
            let sources = ["a", "b"]
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        Source {
                            columns: vec!["ID".into()],
                            rows: if empty { vec![] } else { vec![vec![S::Int(2)]] },
                            complete: true,
                        },
                    )
                })
                .collect();
            for projection in ["ID", "ID,ID"] {
                for order in ["COUNT(*)", "SUM(ID)", "1+COUNT(*)", "COALESCE(COUNT(*),0)"] {
                    let sql = format!(
                        "{prefix}SELECT {projection} FROM a UNION ALL SELECT {projection} FROM b ORDER BY {order}"
                    );
                    let error = Program::compile(&sql)
                        .unwrap()
                        .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                        .unwrap_err();
                    assert_eq!(
                        error.code, "relational.invalid_binding",
                        "{sql}; empty={empty}"
                    );
                }
            }
        }
    }
}

#[test]
fn compound_ordinal_outputs_keep_branch_aggregates_and_named_ordering() {
    for prefix in ["", "EXPLAIN "] {
        for empty in [false, true] {
            let sources = ["a", "b"]
                .into_iter()
                .map(|name| {
                    (
                        name.into(),
                        Source {
                            columns: vec!["ID".into()],
                            rows: if empty {
                                vec![]
                            } else if name == "a" {
                                vec![vec![S::Int(2)], vec![S::Int(3)]]
                            } else {
                                vec![vec![S::Int(4)]]
                            },
                            complete: true,
                        },
                    )
                })
                .collect();
            let sql = format!(
                "{prefix}SELECT COUNT(*) AS n FROM a UNION ALL SELECT COUNT(*) FROM b ORDER BY n"
            );
            let result = Program::compile(&sql)
                .unwrap()
                .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                .unwrap();
            if prefix.is_empty() {
                assert_eq!(result.columns, ["n"]);
                let counts = if empty { [0, 0] } else { [1, 2] };
                assert_eq!(
                    result.rows,
                    counts.map(|n| vec![S::Int(n)]),
                    "{sql}; empty={empty}"
                );
            } else {
                assert_eq!(result.plan.unwrap().output_columns, ["n"]);
                assert!(result.rows.is_empty());
            }
        }
    }
}
