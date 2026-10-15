use super::*;
use crate::resource::Cost;
use std::sync::Arc;

fn input(rows: Vec<Vec<Scalar>>) -> Source {
    Source {
        columns: vec!["ID".into(), "Name".into()],
        rows,
        complete: true,
    }
}
fn work(bytes: usize, units: u64) -> Work<'static> {
    Work {
        left: units,
        bytes,
        deadline: Instant::now() + Duration::from_secs(10),
        cancel: None,
        shared: None,
        planning: false,
        selection_sources: Vec::new(),
        providers: &[],
    }
}
fn capabilities() -> BTreeMap<String, String> {
    BTreeMap::from([("t".into(), "ID".into())])
}
fn execute(
    predicate: &str,
    rows: Vec<Vec<Scalar>>,
    parameters: &BTreeMap<String, Scalar>,
    limits: Limits,
) -> Result<QueryResult> {
    Program::compile(&format!(
        "SELECT ID,Name FROM t WHERE {predicate} ORDER BY ID,Name"
    ))?
    .execute_prepared_budgeted(
        &BTreeMap::from([("t".into(), input(rows))]),
        parameters,
        limits,
        None,
        None,
        &capabilities(),
    )
}
#[derive(Default)]
struct Fixture {
    rows: Vec<Vec<Scalar>>,
    lookups: usize,
    decline: Option<usize>,
    failure: Option<usize>,
    reuse: bool,
    cancel: Option<Arc<AtomicBool>>,
}
impl Equality for Fixture {
    fn prepare(&mut self, _: usize, _: &mut Temporary<'_>, _: &mut Work<'_>) -> Result<bool> {
        Ok(true)
    }
    fn equal(
        &mut self,
        column: usize,
        value: &Scalar,
        work: &mut Work<'_>,
        emit: &mut Yield<'_>,
    ) -> Result<bool> {
        self.lookups += 1;
        if self.failure == Some(self.lookups) {
            return Err(fault("fixture.lookup_failed", 5, "provider failure"));
        }
        if self.decline == Some(self.lookups) {
            if let Some(cancel) = &self.cancel {
                cancel.store(true, AtomicOrdering::Release);
            }
            return Ok(false);
        }
        if self.reuse {
            if value == &Scalar::Float(u64::MAX as f64) {
                return Ok(true);
            }
            self.rows[0][0] = value.clone();
            self.rows[0][1] = Scalar::String(match value {
                Scalar::Float(n) => n.to_string(),
                Scalar::UInt(n) => n.to_string(),
                Scalar::Int(n) => n.to_string(),
                _ => String::new(),
            });
            emit(&self.rows[0], work)?;
        } else {
            for row in &self.rows {
                if value != &Scalar::Null
                    && row.get(column) != Some(&Scalar::Null)
                    && compare(row.get(column).ok_or_else(binding)?, value)? == Ordering::Equal
                {
                    emit(row, work)?;
                }
            }
        }
        Ok(true)
    }
}
fn collected(
    predicate: &str,
    params: &BTreeMap<String, Scalar>,
    provider: &mut Fixture,
    work: &mut Work<'_>,
) -> Result<(bool, Vec<Vec<Scalar>>)> {
    let program = Program::compile(&format!("SELECT ID,Name FROM t WHERE {predicate}"))?;
    let columns = vec!["ID".into(), "Name".into()];
    let mut rows = vec![];
    let handled = collect(
        program.filter.as_ref(),
        &columns,
        "t",
        params,
        provider,
        work,
        &mut |row, work| {
            // This is the same original-predicate guard used by both real Program
            // consumers. The collector itself never replaces SQL NULL semantics.
            if truth(&eval(
                program.filter.as_ref().unwrap(),
                row,
                &columns,
                "t",
                params,
                work,
            )?)? == Some(true)
            {
                rows.push(row.to_vec());
            }
            Ok(())
        },
    )?;
    Ok((handled, rows))
}

#[test]
fn exact_numeric_keys_match_rational_equality_and_go_lengths() {
    let values = vec![
        Scalar::Int(i64::MIN),
        Scalar::Int(-1),
        Scalar::Int(0),
        Scalar::Int(1),
        Scalar::UInt(1),
        Scalar::UInt(u64::MAX),
        Scalar::Float(-0.0),
        Scalar::Float(1.0),
        Scalar::Float(1.5),
        Scalar::Float(u64::MAX as f64),
        Scalar::Float(f64::MIN_POSITIVE),
        Scalar::Float(f64::from_bits(1)),
        Scalar::Float(f64::MAX),
        Scalar::Float(-f64::MAX),
    ];
    for left in &values {
        let key = Key::of(left).unwrap();
        assert_eq!(
            key.go_len(),
            1 + rational(left).unwrap().to_string().len(),
            "{left:?}"
        );
        for right in &values {
            assert_eq!(
                key == Key::of(right).unwrap(),
                compare(left, right).unwrap() == Ordering::Equal,
                "{left:?} / {right:?}"
            );
        }
    }
    assert_eq!(Key::Null.go_len(), 1);
    assert_eq!(
        Key::of(&Scalar::Float(f64::NAN)).unwrap_err().code,
        "relational.numeric_range"
    );
}

#[test]
fn real_program_uses_one_index_within_35_id_work_budget() {
    let rows: Vec<_> = (1..=1000)
        .map(|id| vec![Scalar::Int(id), Scalar::String(format!("spell-{id}"))])
        .collect();
    let list = (1..=35)
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let predicate = format!("ID IN ({list})");
    let limits = Limits {
        work: 5000,
        memory_bytes: 1 << 20,
        ..Limits::default()
    };
    let got = execute(&predicate, rows.clone(), &BTreeMap::new(), limits).unwrap();
    assert_eq!(got.rows, rows[..35]);
    // Wrapping in AND disables only the optional bare-list index. A full
    // 1000-row evaluation cannot complete within the same real SQL budget.
    assert_eq!(
        execute(
            &format!("({predicate}) AND TRUE"),
            rows,
            &BTreeMap::new(),
            limits
        )
        .unwrap_err()
        .code,
        "relational.resource_limit"
    );
}

#[test]
fn fixed_go_35_candidate_provider_uses_35_lookups() {
    let mut provider = Fixture {
        rows: (1..=1000)
            .map(|id| vec![Scalar::Int(id), Scalar::String(format!("spell-{id}"))])
            .collect(),
        ..Fixture::default()
    };
    let list = (1..=35)
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let (handled, rows) = collected(
        &format!("ID IN ({list})"),
        &BTreeMap::new(),
        &mut provider,
        &mut work(1 << 20, 5000),
    )
    .unwrap();
    assert!(handled);
    assert_eq!(rows, provider.rows[..35]);
    assert_eq!(provider.lookups, 35);
}

#[test]
fn real_program_preserves_candidate_then_provider_order_without_order_by() {
    let source = input(vec![
        vec![Scalar::Int(1), Scalar::String("a".into())],
        vec![Scalar::UInt(1), Scalar::String("b".into())],
        vec![Scalar::Int(2), Scalar::String("c".into())],
        vec![Scalar::Int(3), Scalar::String("d".into())],
        vec![Scalar::Null, Scalar::String("null".into())],
    ]);
    let got = Program::compile("SELECT ID,Name FROM t WHERE ID IN (3,1,1.0,2,NULL)")
        .unwrap()
        .execute_prepared_budgeted(
            &BTreeMap::from([("t".into(), source)]),
            &BTreeMap::new(),
            Limits::default(),
            None,
            None,
            &capabilities(),
        )
        .unwrap();
    assert_eq!(
        got.rows,
        vec![
            vec![Scalar::Int(3), Scalar::String("d".into())],
            vec![Scalar::Int(1), Scalar::String("a".into())],
            vec![Scalar::UInt(1), Scalar::String("b".into())],
            vec![Scalar::Int(2), Scalar::String("c".into())]
        ]
    );
}

#[test]
fn supported_and_fallback_predicates_match_ordered_scan_results_or_errors() {
    let rows = vec![
        vec![Scalar::Int(1), Scalar::String("a".into())],
        vec![Scalar::UInt(1), Scalar::String("b".into())],
        vec![Scalar::Float(2.0), Scalar::String("c".into())],
        vec![Scalar::Int(3), Scalar::String("d".into())],
        vec![Scalar::Null, Scalar::String("null".into())],
    ];
    for (predicate, params) in [
        (
            "ID IN (2,1,1.0,:one,NULL,99)",
            BTreeMap::from([("one".into(), Scalar::UInt(1))]),
        ),
        (
            "ID IN (:two,:none,:negative)",
            BTreeMap::from([
                ("two".into(), Scalar::Float(2.0)),
                ("none".into(), Scalar::Null),
                ("negative".into(), Scalar::Int(-1)),
            ]),
        ),
        ("ID IN (-1,+1,2.0,2.5)", BTreeMap::new()),
        (
            "ID IN (:bad)",
            BTreeMap::from([("bad".into(), Scalar::Bool(true))]),
        ),
        ("ID IN (NULL,NULL)", BTreeMap::new()),
        ("ID IN (1,'bad')", BTreeMap::new()),
        ("ID IN (1,18446744073709551616)", BTreeMap::new()),
        ("ID IN (1,1/0)", BTreeMap::new()),
        ("ID NOT IN (1,2)", BTreeMap::new()),
    ] {
        let indexed = execute(predicate, rows.clone(), &params, Limits::default());
        let scanned = execute(
            &format!("({predicate}) AND TRUE"),
            rows.clone(),
            &params,
            Limits::default(),
        );
        match (indexed, scanned) {
            (Ok(indexed), Ok(scanned)) => {
                assert_eq!(indexed.rows, scanned.rows, "{predicate}");
                assert_eq!(indexed.complete, scanned.complete);
            }
            (Err(indexed), Err(scanned)) => assert_eq!(indexed.code, scanned.code, "{predicate}"),
            (indexed, scanned) => panic!("{predicate}: indexed={indexed:?}, scanned={scanned:?}"),
        }
    }
}

#[test]
fn reused_provider_rows_are_owned_and_rounded_u64_is_not_deduplicated() {
    let mut provider = Fixture {
        rows: vec![vec![Scalar::Null, Scalar::Null]],
        reuse: true,
        ..Fixture::default()
    };
    let params = BTreeMap::from([
        ("high".into(), Scalar::UInt(u64::MAX)),
        ("round".into(), Scalar::Float(u64::MAX as f64)),
        ("duplicate".into(), Scalar::Float(1.0)),
    ]);
    let (handled, rows) = collected(
        "ID IN (:high,:round,:duplicate,1)",
        &params,
        &mut provider,
        &mut work(1 << 20, 10000),
    )
    .unwrap();
    assert!(handled);
    assert_eq!(provider.lookups, 3);
    assert_eq!(
        rows,
        vec![
            vec![Scalar::UInt(u64::MAX), Scalar::String(u64::MAX.to_string())],
            vec![Scalar::Float(1.0), Scalar::String("1".into())]
        ]
    );
}

#[test]
fn half_buffer_exhaustion_discards_before_yield_and_refunds_its_memory() {
    let mut provider = Fixture {
        rows: vec![
            vec![Scalar::Int(1), Scalar::String("x".repeat(1800))],
            vec![Scalar::Int(2), Scalar::String("y".repeat(1800))],
        ],
        ..Fixture::default()
    };
    let mut work = work(2048, 10000);
    let (handled, rows) =
        collected("ID IN (1,2)", &BTreeMap::new(), &mut provider, &mut work).unwrap();
    assert!(!handled);
    assert!(rows.is_empty());
    assert_eq!(provider.lookups, 1);
    assert_eq!(work.bytes, 2048);
}

#[test]
fn real_grouped_consumer_falls_back_to_streaming_when_index_will_not_fit() {
    let source = input(vec![
        vec![Scalar::Int(1), Scalar::String("x".repeat(1800))],
        vec![Scalar::Int(2), Scalar::String("y".repeat(1800))],
    ]);
    let got = Program::compile("SELECT COUNT(*) FROM t WHERE ID IN (1,2)")
        .unwrap()
        .execute_prepared_budgeted(
            &BTreeMap::from([("t".into(), source)]),
            &BTreeMap::new(),
            Limits {
                work: 10000,
                memory_bytes: 2048,
                ..Limits::default()
            },
            None,
            None,
            &capabilities(),
        )
        .unwrap();
    assert_eq!(got.rows, vec![vec![Scalar::Int(2)]]);
}

#[test]
fn late_provider_decline_discards_all_prior_candidates() {
    for decline in [1, 2] {
        let mut provider = Fixture {
            rows: vec![
                vec![Scalar::Int(1), Scalar::Int(1)],
                vec![Scalar::Int(2), Scalar::Int(2)],
            ],
            decline: Some(decline),
            ..Fixture::default()
        };
        let mut work = work(1 << 20, 10000);
        let (handled, rows) =
            collected("ID IN (1,2)", &BTreeMap::new(), &mut provider, &mut work).unwrap();
        assert!(!handled);
        assert!(rows.is_empty());
        assert_eq!(provider.lookups, decline);
        assert_eq!(work.bytes, 1 << 20);
    }
}

#[test]
fn provider_failure_is_fatal_without_yield_or_fallback() {
    let mut provider = Fixture {
        rows: vec![vec![Scalar::Int(1), Scalar::Null]],
        failure: Some(2),
        ..Fixture::default()
    };
    let mut work = work(1 << 20, 10000);
    let mut yielded = 0;
    let program = Program::compile("SELECT ID FROM t WHERE ID IN (1,2)").unwrap();
    let error = collect(
        program.filter.as_ref(),
        &["ID".into(), "Name".into()],
        "t",
        &BTreeMap::new(),
        &mut provider,
        &mut work,
        &mut |_, _| {
            yielded += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.lookup_failed");
    assert_eq!(yielded, 0);
    assert_eq!(work.bytes, 1 << 20);
}

#[test]
fn independent_budget_exhaustion_never_becomes_optional_fallback() {
    let budget = Budget::new(
        Cost {
            retained_bytes: 511,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(5),
        None,
    );
    let mut work = Work {
        shared: Some(&budget),
        ..work(1 << 20, 10000)
    };
    let mut provider = Fixture::default();
    assert_eq!(
        collected("ID IN (1)", &BTreeMap::new(), &mut provider, &mut work)
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(provider.lookups, 0);
    assert_eq!(work.bytes, 1 << 20);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn cancellation_during_decline_is_fatal_and_shared_memory_is_released() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(5),
        Some(cancelled.clone()),
    );
    let mut work = Work {
        shared: Some(&budget),
        ..work(1 << 20, 10000)
    };
    let mut provider = Fixture {
        decline: Some(2),
        cancel: Some(cancelled),
        rows: vec![vec![Scalar::Int(1), Scalar::Null]],
        ..Fixture::default()
    };
    assert_eq!(
        collected("ID IN (1,2)", &BTreeMap::new(), &mut provider, &mut work)
            .unwrap_err()
            .code,
        "operation.cancelled"
    );
    assert_eq!(provider.lookups, 2);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert!(budget.snapshot().unwrap().decode_work > 0);
    assert_eq!(work.bytes, 1 << 20);
}

#[test]
fn work_limit_and_deadline_fail_without_lookup_or_fallback() {
    for expired in [false, true] {
        let mut work = work(1 << 20, if expired { 10000 } else { 0 });
        if expired {
            work.deadline = Instant::now();
        }
        let mut provider = Fixture::default();
        assert_eq!(
            collected("ID IN (1,2)", &BTreeMap::new(), &mut provider, &mut work)
                .unwrap_err()
                .code,
            "relational.resource_limit"
        );
        assert_eq!(provider.lookups, 0);
    }
}

#[test]
fn unary_parameters_and_non_bare_predicates_never_request_lookup() {
    for predicate in [
        "ID IN (-:one)",
        "ID IN (+ID)",
        "ID IN (1+1)",
        "ID NOT IN (1)",
        "ID IN (1) AND TRUE",
        "(ID+0) IN (1)",
    ] {
        let params = if predicate.contains(":one") {
            BTreeMap::from([("one".into(), Scalar::Int(1))])
        } else {
            BTreeMap::new()
        };
        let mut provider = Fixture::default();
        let (handled, rows) =
            collected(predicate, &params, &mut provider, &mut work(1 << 20, 10000)).unwrap();
        assert!(!handled, "{predicate}");
        assert!(rows.is_empty());
        assert_eq!(provider.lookups, 0);
    }
}

#[test]
fn invalid_literal_on_empty_source_preserves_scan_error_timing() {
    assert!(
        execute(
            "ID IN (1,18446744073709551616)",
            vec![],
            &BTreeMap::new(),
            Limits::default()
        )
        .unwrap()
        .rows
        .is_empty()
    );
    assert_eq!(
        execute(
            "ID IN (1,18446744073709551616)",
            vec![vec![Scalar::Int(1), Scalar::Null]],
            &BTreeMap::new(),
            Limits::default()
        )
        .unwrap_err()
        .code,
        "relational.numeric_range"
    );
}

#[test]
fn real_provider_does_not_hide_heterogeneous_or_malformed_rows() {
    for rows in [
        vec![
            vec![Scalar::Int(1), Scalar::Null],
            vec![Scalar::Bool(true), Scalar::Null],
        ],
        vec![vec![Scalar::Int(1), Scalar::Null], vec![Scalar::Int(2)]],
    ] {
        let indexed = execute(
            "ID IN (1)",
            rows.clone(),
            &BTreeMap::new(),
            Limits::default(),
        )
        .unwrap_err();
        let scanned = execute(
            "ID IN (1) AND TRUE",
            rows,
            &BTreeMap::new(),
            Limits::default(),
        )
        .unwrap_err();
        assert_eq!(indexed.code, scanned.code);
    }
}

#[test]
fn output_charges_survive_scoped_candidate_refund() {
    let program = Program::compile("SELECT ID FROM t WHERE ID IN (1)").unwrap();
    let mut provider = Fixture {
        rows: vec![vec![Scalar::Int(1), Scalar::Null]],
        ..Fixture::default()
    };
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(5),
        None,
    );
    let mut work = Work {
        shared: Some(&budget),
        ..work(1 << 20, 10000)
    };
    assert!(
        collect(
            program.filter.as_ref(),
            &["ID".into(), "Name".into()],
            "t",
            &BTreeMap::new(),
            &mut provider,
            &mut work,
            &mut |_, work| work.retain(777)
        )
        .unwrap()
    );
    assert_eq!(work.bytes, (1 << 20) - 777);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 777);
}

#[test]
fn explain_binds_but_does_not_build_index_or_read_malformed_rows() {
    let result = Program::compile("EXPLAIN SELECT ID FROM t WHERE ID IN (1,2)")
        .unwrap()
        .execute_prepared_budgeted(
            &BTreeMap::from([("t".into(), input(vec![vec![]]))]),
            &BTreeMap::new(),
            Limits::default(),
            None,
            None,
            &capabilities(),
        )
        .unwrap();
    assert!(result.plan.is_some());
    assert!(result.rows.is_empty());
}

#[test]
fn unprepared_sources_never_acquire_implicit_equality_capability() {
    let sources = BTreeMap::from([(
        "t".into(),
        input(vec![
            vec![Scalar::Int(1), Scalar::Null],
            vec![Scalar::Int(2), Scalar::Null],
        ]),
    )]);
    let result = Program::compile("SELECT ID FROM t WHERE ID IN (2,1)")
        .unwrap()
        .execute(&sources, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]]
    );
}

#[test]
fn cte_and_derived_rows_do_not_inherit_physical_lookup_capability() {
    let sources = BTreeMap::from([(
        "t".into(),
        input(vec![
            vec![Scalar::Int(1), Scalar::Null],
            vec![Scalar::Int(2), Scalar::Null],
        ]),
    )]);
    for sql in [
        "WITH c AS (SELECT ID FROM t) SELECT ID FROM c WHERE ID IN (2,1)",
        "SELECT ID FROM (SELECT ID FROM t) AS d WHERE ID IN (2,1)",
    ] {
        // A physical root remains eligible inside the CTE body, but the newly
        // materialized output is a different Source and must retain scan order.
        let result = Program::compile(sql)
            .unwrap()
            .execute_prepared_budgeted(
                &sources,
                &BTreeMap::new(),
                Limits::default(),
                None,
                None,
                &capabilities(),
            )
            .unwrap();
        assert_eq!(
            result.rows,
            vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]],
            "{sql}"
        );
    }
}

#[test]
fn capability_uses_bound_identity_ordinal_and_respects_aliases() {
    let sources = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["Name".into(), "RecordID".into(), "Other".into()],
            rows: vec![
                vec![Scalar::Null, Scalar::Int(1), Scalar::Int(1)],
                vec![Scalar::Null, Scalar::Int(2), Scalar::Int(2)],
            ],
            complete: true,
        },
    )]);
    let capability = BTreeMap::from([("T".into(), "recordid".into())]);
    for (predicate, expected) in [
        (
            "x.RecordID IN (2,1)",
            vec![vec![Scalar::Int(2)], vec![Scalar::Int(1)]],
        ),
        (
            "x.Other IN (2,1)",
            vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]],
        ),
    ] {
        let result = Program::compile(&format!("SELECT x.RecordID FROM t AS x WHERE {predicate}"))
            .unwrap()
            .execute_prepared_budgeted(
                &sources,
                &BTreeMap::new(),
                Limits::default(),
                None,
                None,
                &capability,
            )
            .unwrap();
        assert_eq!(result.rows, expected);
    }
}

#[test]
fn capability_does_not_hide_source_binding_failures() {
    let source = input(vec![vec![Scalar::Int(1), Scalar::Null]]);
    let sources = BTreeMap::from([("t".into(), source)]);
    for capability in [
        BTreeMap::from([("missing".into(), "ID".into())]),
        BTreeMap::from([("t".into(), "missing".into())]),
    ] {
        assert_eq!(
            Program::compile("SELECT ID FROM t WHERE ID IN (1)")
                .unwrap()
                .execute_prepared_budgeted(
                    &sources,
                    &BTreeMap::new(),
                    Limits::default(),
                    None,
                    None,
                    &capability
                )
                .unwrap_err()
                .code,
            "relational.invalid_binding"
        );
    }
}

#[test]
fn nonnumeric_parameter_decline_still_propagates_evaluation_budget_stop() {
    for value in [Scalar::Bool(true), Scalar::String("text".into())] {
        let params = BTreeMap::from([("bad".into(), value)]);
        let mut provider = Fixture::default();
        assert_eq!(
            collected("ID IN (:bad)", &params, &mut provider, &mut work(2048, 0))
                .unwrap_err()
                .code,
            "relational.resource_limit"
        );
        assert_eq!(provider.lookups, 0);
    }
}
