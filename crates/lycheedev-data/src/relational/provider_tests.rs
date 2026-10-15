//! Query-scoped provider regressions against Go 3.2.3 commit
//! 3766f9cacb52e93a803a626187e2a1d54a445468:
//! execute.go (preflight/join ownership), selection.go (Equal/IN),
//! rows.go and topk.go (full validation despite LIMIT).
//! These are executable regression sources, not recorded passing results.
use super::*;
use crate::resource::{Budget, Cost};
use std::{cell::RefCell, rc::Rc, sync::Arc};

type Trace = Rc<RefCell<Vec<String>>>;

struct Fixture {
    name: &'static str,
    trace: Trace,
    rows: Vec<Vec<Scalar>>,
    lookup_rows: Option<Vec<Vec<Scalar>>>,
    capable: bool,
    projected: bool,
    lookup_all: bool,
    decline_on: Option<usize>,
    fail_on: Option<usize>,
    cancel_on: Option<(usize, Arc<AtomicBool>)>,
    scan_fail_after: Option<usize>,
    lookups: RefCell<Vec<Scalar>>,
    selections: RefCell<Vec<Vec<usize>>>,
}
impl Fixture {
    fn new(name: &'static str, trace: &Trace, rows: Vec<Vec<Scalar>>) -> Self {
        Self {
            name,
            trace: trace.clone(),
            rows,
            lookup_rows: None,
            capable: true,
            projected: false,
            lookup_all: false,
            decline_on: None,
            fail_on: None,
            cancel_on: None,
            scan_fail_after: None,
            lookups: RefCell::new(vec![]),
            selections: RefCell::new(vec![]),
        }
    }
    fn event(&self, action: &str) {
        self.trace
            .borrow_mut()
            .push(format!("{}.{action}", self.name));
    }
    fn scans(&self) -> usize {
        self.trace
            .borrow()
            .iter()
            .filter(|event| **event == format!("{}.scan", self.name))
            .count()
    }
    fn scan_rows(&self, selected: Option<&[usize]>, emit: &mut RowYield<'_>) -> Result<()> {
        self.event("scan");
        let mut scratch = vec![];
        for (at, row) in self.rows.iter().enumerate() {
            if self.scan_fail_after == Some(at) {
                return Err(fault("fixture.scan_failed", 5, "scan failed"));
            }
            scratch.clone_from(row);
            if let Some(selected) = selected {
                // Match ScanSelected's validation contract before null-filling
                // unused positions; never make projection hide bad scalars.
                row_cost(row)?;
                for (column, value) in scratch.iter_mut().enumerate() {
                    if !selected.contains(&column) {
                        *value = Scalar::Null;
                    }
                }
            }
            self.event("row");
            emit(&scratch)?;
            // Invalidate the reused row immediately after its callback.
            scratch.fill(Scalar::Null);
        }
        if self.scan_fail_after == Some(self.rows.len()) {
            return Err(fault("fixture.scan_failed", 5, "scan failed"));
        }
        Ok(())
    }
}
impl RowProvider for Fixture {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        self.scan_rows(None, emit)
    }
    fn scan_selected(&self, columns: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
        self.selections.borrow_mut().push(columns.to_vec());
        self.scan_rows(self.projected.then_some(columns), emit)
    }
    fn has_equal(&self) -> bool {
        self.capable
    }
    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        self.event("equal");
        self.lookups.borrow_mut().push(value.clone());
        let call = self.lookups.borrow().len();
        if self.fail_on == Some(call) {
            return Err(fault("fixture.lookup_failed", 5, "lookup failed"));
        }
        if let Some((at, cancelled)) = &self.cancel_on
            && *at == call
        {
            cancelled.store(true, AtomicOrdering::Release);
        }
        if self.decline_on == Some(call) || !column.eq_ignore_ascii_case("ID") {
            return Ok(EqualityOutcome::Declined);
        }
        let mut scratch = vec![];
        for row in self.lookup_rows.as_ref().unwrap_or(&self.rows) {
            if self.lookup_all
                || (value != &Scalar::Null
                    && row[0] != Scalar::Null
                    && compare(&row[0], value)? == Ordering::Equal)
            {
                scratch.clone_from(row);
                self.event("lookup_row");
                emit(&scratch)?;
                scratch.fill(Scalar::Null);
            }
        }
        Ok(EqualityOutcome::Handled)
    }
}
fn schema(names: &[&str]) -> Source {
    Source {
        columns: names.iter().map(|name| (*name).into()).collect(),
        rows: vec![],
        complete: true,
    }
}
fn row(id: i64, text: &str) -> Vec<Scalar> {
    vec![Scalar::Int(id), Scalar::String(text.into())]
}
fn run(
    sql: &str,
    roots: &[(&str, &Source, &dyn RowProvider)],
    parameters: &BTreeMap<String, Scalar>,
    limits: Limits,
    cancel: Option<&AtomicBool>,
) -> Result<QueryResult> {
    let sources = roots
        .iter()
        .map(|(name, source, _)| ((*name).into(), *source))
        .collect();
    let providers: Vec<_> = roots
        .iter()
        .map(|(_, schema, provider)| PreparedSource {
            schema: *schema,
            provider: *provider,
        })
        .collect();
    Program::compile(sql)?
        .execute_providers_budgeted(&sources, &providers, parameters, limits, cancel, None)
}
fn visit_selection(
    provider: &Fixture,
    source: &Source,
    predicate: &str,
    limits: Limits,
    cancel: Option<&AtomicBool>,
    shared: Option<&Budget>,
    emit: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
) -> Result<(usize, u64)> {
    let program = Program::compile(&format!("SELECT ID,Name FROM t WHERE {predicate}"))?;
    let providers = [PreparedSource {
        schema: source,
        provider,
    }];
    let mut work = Work {
        left: limits.work,
        bytes: limits.memory_bytes,
        deadline: limits.deadline,
        cancel,
        shared,
        planning: false,
        selection_sources: vec![],
        providers: &providers,
    };
    selection::visit(
        source,
        None,
        program.filter.as_ref(),
        "t",
        &BTreeMap::new(),
        &mut work,
        emit,
    )?;
    Ok((work.bytes, work.left))
}

#[test]
fn full_preflight_rejects_late_join_order_group_union_cte_and_recursive_errors_before_reads() {
    let queries = [
        "SELECT a.ID FROM t a JOIN t b ON a.ID=b.ID JOIN t c ON c.Missing=a.ID",
        "SELECT a.ID FROM t a JOIN t b ON a.ID=b.ID ORDER BY Missing",
        "SELECT a.ID FROM t a JOIN t b ON a.ID=b.ID GROUP BY Missing",
        "SELECT ID FROM t UNION ALL SELECT Missing FROM t",
        "WITH a AS (SELECT ID FROM t), b AS (SELECT Missing FROM t) SELECT ID FROM a",
        "SELECT d.Missing FROM (SELECT ID FROM t) d",
        "WITH RECURSIVE r AS (SELECT ID FROM t WHERE FALSE UNION ALL SELECT Missing FROM r) SELECT ID FROM r",
        "SELECT CASE WHEN TRUE THEN ID ELSE Missing END FROM t",
    ];
    for sql in queries {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let mut provider = Fixture::new("t", &trace, vec![row(1, "unread")]);
        provider.scan_fail_after = Some(0);
        provider.fail_on = Some(1);
        let error = run(
            sql,
            &[("t", &source, &provider)],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, "relational.invalid_binding", "{sql}");
        assert!(trace.borrow().is_empty(), "{sql}: {:?}", trace.borrow());
    }
}

#[test]
fn explain_binds_single_and_join_sources_without_scan_or_equal() {
    for sql in [
        "EXPLAIN SELECT ID FROM t WHERE ID IN (1,2)",
        "EXPLAIN SELECT a.ID,b.Name FROM t a LEFT JOIN t b ON a.ID=b.ID",
    ] {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let provider = Fixture::new("t", &trace, vec![row(1, "unread")]);
        let result = run(
            sql,
            &[("t", &source, &provider)],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap();
        assert!(result.plan.is_some());
        assert!(result.rows.is_empty());
        assert!(trace.borrow().is_empty());
    }
}

#[test]
fn join_materializes_reused_right_rows_once_in_join_order_before_streaming_left() {
    let trace = Trace::default();
    let left = schema(&["ID", "Name"]);
    let right = schema(&["ID", "Name"]);
    let tail = schema(&["ID", "Name"]);
    let a = Fixture::new("a", &trace, vec![row(2, "left-2"), row(1, "left-1")]);
    let b = Fixture::new(
        "b",
        &trace,
        vec![row(1, "first"), row(2, "second"), row(2, "third")],
    );
    let c = Fixture::new("c", &trace, vec![row(2, "tail")]);
    let result = run(
        "SELECT a.Name,b.Name,c.Name FROM a JOIN b ON a.ID=b.ID LEFT JOIN c ON b.ID=c.ID",
        &[("a", &left, &a), ("b", &right, &b), ("c", &tail, &c)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        result.rows,
        vec![
            vec![
                Scalar::String("left-2".into()),
                Scalar::String("second".into()),
                Scalar::String("tail".into())
            ],
            vec![
                Scalar::String("left-2".into()),
                Scalar::String("third".into()),
                Scalar::String("tail".into())
            ],
            vec![
                Scalar::String("left-1".into()),
                Scalar::String("first".into()),
                Scalar::Null
            ],
        ]
    );
    assert_eq!(
        *trace.borrow(),
        [
            "b.scan", "b.row", "b.row", "b.row", "c.scan", "c.row", "a.scan", "a.row", "a.row"
        ]
    );
    assert!(
        a.lookups.borrow().is_empty()
            && b.lookups.borrow().is_empty()
            && c.lookups.borrow().is_empty()
    );
}

#[test]
fn join_early_on_selection_uses_its_bound_prefix() {
    let trace = Trace::default();
    let sa = schema(&["ID", "Early"]);
    let sb = schema(&["ID"]);
    let sc = schema(&["Early"]);
    let mut a = Fixture::new("a", &trace, vec![vec![Scalar::Int(7), Scalar::Int(1)]]);
    let mut b = Fixture::new("b", &trace, vec![vec![Scalar::Int(7)]]);
    let mut c = Fixture::new("c", &trace, vec![vec![Scalar::Int(9)]]);
    a.projected = true;
    b.projected = true;
    c.projected = true;
    let result = run(
        "SELECT a.ID FROM a JOIN b ON Early=1 CROSS JOIN c",
        &[("a", &sa, &a), ("b", &sb, &b), ("c", &sc, &c)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(7)]]);
    assert_eq!(*a.selections.borrow(), vec![vec![0, 1]]);
}

#[test]
fn join_validates_right_even_with_empty_left_and_never_scans_left_after_right_failure() {
    let trace = Trace::default();
    let left = schema(&["ID", "Name"]);
    let right = schema(&["ID", "Name"]);
    let a = Fixture::new("a", &trace, vec![]);
    let b = Fixture::new("b", &trace, vec![vec![Scalar::Int(1)]]);
    let error = run(
        "SELECT a.ID FROM a CROSS JOIN b",
        &[("a", &left, &a), ("b", &right, &b)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "relational.invalid_binding");
    assert_eq!(*trace.borrow(), ["b.scan", "b.row"]);
}

#[test]
fn join_growth_failure_stops_left_stream_before_its_next_row() {
    let trace = Trace::default();
    let left = schema(&["ID", "Name"]);
    let right = schema(&["ID"]);
    let a = Fixture::new(
        "a",
        &trace,
        vec![
            row(1, "small"),
            row(2, &"x".repeat(100_000)),
            row(3, "unread"),
        ],
    );
    let b = Fixture::new("b", &trace, vec![vec![Scalar::Int(1)]]);
    let error = run(
        "SELECT a.Name FROM a CROSS JOIN b LIMIT 1",
        &[("a", &left, &a), ("b", &right, &b)],
        &BTreeMap::new(),
        Limits {
            memory_bytes: 16 << 10,
            ..Limits::default()
        },
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "relational.resource_limit");
    assert_eq!(
        *trace.borrow(),
        ["b.scan", "b.row", "a.scan", "a.row", "a.row"]
    );
}

#[test]
fn join_validates_unselected_left_scalar_before_another_provider_row() {
    for invalid in [
        Scalar::Float(f64::NAN),
        Scalar::String("x".repeat((1 << 20) + 1)),
    ] {
        let expected = if matches!(invalid, Scalar::Float(_)) {
            "relational.numeric_range"
        } else {
            "relational.resource_limit"
        };
        let trace = Trace::default();
        let sa = schema(&["ID", "Unused"]);
        let sb = schema(&["ID"]);
        let a = Fixture::new(
            "a",
            &trace,
            vec![
                vec![Scalar::Int(1), invalid],
                vec![Scalar::Int(2), Scalar::Null],
            ],
        );
        let b = Fixture::new("b", &trace, vec![vec![Scalar::Int(1)]]);
        let error = run(
            "SELECT a.ID FROM a CROSS JOIN b",
            &[("a", &sa, &a), ("b", &sb, &b)],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, expected);
        assert_eq!(*trace.borrow(), ["b.scan", "b.row", "a.scan", "a.row"]);
    }
}

#[test]
fn limit_bounds_output_without_hiding_later_scan_errors() {
    for limit in [0, 1] {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let mut provider = Fixture::new("t", &trace, vec![row(1, "first"), row(2, "second")]);
        provider.scan_fail_after = Some(1);
        let error = run(
            &format!("SELECT ID FROM t LIMIT {limit}"),
            &[("t", &source, &provider)],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, "fixture.scan_failed");
        assert_eq!(*trace.borrow(), ["t.scan", "t.row"]);
    }
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new(
        "t",
        &trace,
        vec![row(3, "third"), row(1, "first"), row(2, "second")],
    );
    let result = run(
        "SELECT ID FROM t LIMIT 1 OFFSET 1",
        &[("t", &source, &provider)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert_eq!(*trace.borrow(), ["t.scan", "t.row", "t.row", "t.row"]);
}

#[test]
fn absent_equal_capability_skips_all_speculative_candidate_work() {
    for capable in [false, true] {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let mut provider = Fixture::new("t", &trace, vec![]);
        provider.capable = capable;
        let result = visit_selection(
            &provider,
            &source,
            "ID IN (1,2,3)",
            Limits {
                work: 0,
                ..Limits::default()
            },
            None,
            None,
            &mut |_, _| panic!("empty provider yielded"),
        );
        if capable {
            assert_eq!(result.unwrap_err().code, "relational.resource_limit");
            assert_eq!(provider.scans(), 0);
        } else {
            assert_eq!(result.unwrap().1, 0);
            assert_eq!(provider.scans(), 1);
        }
        assert!(provider.lookups.borrow().is_empty());
    }
}

#[test]
fn in_uses_first_distinct_numeric_candidate_then_provider_order_and_owns_scratch() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new(
        "t",
        &trace,
        vec![
            row(1, "a"),
            vec![Scalar::UInt(1), Scalar::String("b".into())],
            row(2, "c"),
            row(3, "d"),
            vec![Scalar::Null, Scalar::String("null".into())],
        ],
    );
    let result = run(
        "SELECT ID,Name FROM t WHERE ID IN (3,1,1.0,2,NULL,NULL)",
        &[("t", &source, &provider)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        result.rows,
        vec![
            row(3, "d"),
            row(1, "a"),
            vec![Scalar::UInt(1), Scalar::String("b".into())],
            row(2, "c")
        ]
    );
    assert_eq!(
        *provider.lookups.borrow(),
        vec![Scalar::Int(3), Scalar::Int(1), Scalar::Int(2), Scalar::Null]
    );
    assert_eq!(provider.scans(), 0);
}

#[test]
fn in_exact_large_integers_and_signed_zero_do_not_use_lossy_deduplication() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new("t", &trace, vec![]);
    let params = BTreeMap::from([
        ("large".into(), Scalar::UInt(9_007_199_254_740_993)),
        ("rounded".into(), Scalar::Float(9_007_199_254_740_992.0)),
        ("max".into(), Scalar::UInt(u64::MAX)),
        ("max_round".into(), Scalar::Float(u64::MAX as f64)),
        ("negative_zero".into(), Scalar::Float(-0.0)),
    ]);
    run("SELECT ID FROM t WHERE ID IN (:large,:rounded,:large,:max,:max_round,:negative_zero,0,0.0,NULL,NULL)", &[("t", &source, &provider)], &params, Limits::default(), None).unwrap();
    assert_eq!(
        *provider.lookups.borrow(),
        vec![
            Scalar::UInt(9_007_199_254_740_993),
            Scalar::Float(9_007_199_254_740_992.0),
            Scalar::UInt(u64::MAX),
            Scalar::Float(u64::MAX as f64),
            Scalar::Float(-0.0),
            Scalar::Null
        ]
    );
    assert_eq!(provider.scans(), 0);
}

#[test]
fn in_finishes_every_lookup_before_yielding_any_candidate() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new("t", &trace, vec![row(1, "one"), row(2, "two")]);
    visit_selection(
        &provider,
        &source,
        "ID IN (2,1)",
        Limits::default(),
        None,
        None,
        &mut |_, _| {
            trace.borrow_mut().push("yield".into());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        *trace.borrow(),
        [
            "t.equal",
            "t.lookup_row",
            "t.equal",
            "t.lookup_row",
            "yield",
            "yield"
        ]
    );
}

#[test]
fn in_original_predicate_still_rejects_unmatched_and_null_provider_rows() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let mut provider = Fixture::new(
        "t",
        &trace,
        vec![
            row(1, "match"),
            row(2, "unmatched"),
            vec![Scalar::Null, Scalar::String("null".into())],
        ],
    );
    provider.lookup_all = true;
    let result = run(
        "SELECT ID,Name FROM t WHERE ID IN (1)",
        &[("t", &source, &provider)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.rows, vec![row(1, "match")]);
    assert_eq!(provider.scans(), 0);
}

#[test]
fn in_late_decline_discards_prior_candidates_then_scans_once() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let mut provider = Fixture::new("t", &trace, vec![row(2, "scan-two"), row(1, "scan-one")]);
    provider.lookup_rows = Some(vec![row(1, "discard")]);
    provider.decline_on = Some(2);
    let mut output = vec![];
    let (bytes, _) = visit_selection(
        &provider,
        &source,
        "ID IN (1,2)",
        Limits {
            memory_bytes: 4096,
            ..Limits::default()
        },
        None,
        None,
        &mut |row, work| {
            work.retain(17)?;
            output.push(row.to_vec());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, provider.rows);
    assert_eq!(provider.scans(), 1);
    assert_eq!(provider.lookups.borrow().len(), 2);
    assert_eq!(bytes, 4096 - 34, "only collector-owned memory is refunded");
}

#[test]
fn in_half_buffer_discards_already_copied_rows_and_refunds_only_collector_memory() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let mut provider = Fixture::new("t", &trace, vec![row(1, "scan-one"), row(2, "scan-two")]);
    provider.lookup_rows = Some(vec![row(1, "a"), row(1, "b"), row(1, "c")]);
    let mut output = vec![];
    let (bytes, _) = visit_selection(
        &provider,
        &source,
        "ID IN (1,2)",
        Limits {
            memory_bytes: 1024,
            ..Limits::default()
        },
        None,
        None,
        &mut |row, work| {
            work.retain(17)?;
            output.push(row.to_vec());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, provider.rows);
    assert_eq!(provider.scans(), 1);
    assert_eq!(provider.lookups.borrow().len(), 1);
    assert_eq!(bytes, 1024 - 34);
    assert_eq!(
        trace
            .borrow()
            .iter()
            .filter(|event| event.ends_with("lookup_row"))
            .count(),
        3
    );
}

#[test]
fn in_provider_error_is_fatal_without_partial_output_or_scan_fallback() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let mut provider = Fixture::new("t", &trace, vec![row(1, "one"), row(2, "two")]);
    provider.fail_on = Some(2);
    let mut yielded = 0;
    let error = visit_selection(
        &provider,
        &source,
        "ID IN (1,2)",
        Limits::default(),
        None,
        None,
        &mut |_, _| {
            yielded += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.lookup_failed");
    assert_eq!(yielded, 0);
    assert_eq!(provider.scans(), 0);
}

#[test]
fn in_cancelled_decline_never_falls_back_and_releases_shared_candidate_memory() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let cancelled = Arc::new(AtomicBool::new(false));
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        Some(cancelled.clone()),
    );
    let mut provider = Fixture::new("t", &trace, vec![row(1, "one"), row(2, "two")]);
    provider.decline_on = Some(2);
    provider.cancel_on = Some((2, cancelled.clone()));
    let mut yielded = 0;
    let error = visit_selection(
        &provider,
        &source,
        "ID IN (1,2)",
        Limits::default(),
        Some(&cancelled),
        Some(&budget),
        &mut |_, _| {
            yielded += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "operation.cancelled");
    assert_eq!(yielded, 0);
    assert_eq!(provider.scans(), 0);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert!(budget.snapshot().unwrap().decode_work > 0);
}

#[test]
fn in_safety_budget_failure_is_fatal_before_lookup_or_fallback() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new("t", &trace, vec![]);
    let budget = Budget::new(
        Cost {
            retained_bytes: 511,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(10),
        None,
    );
    let error = visit_selection(
        &provider,
        &source,
        "ID IN (1)",
        Limits::default(),
        None,
        Some(&budget),
        &mut |_, _| panic!("unexpected output"),
    )
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(trace.borrow().is_empty());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn in_callback_error_after_collection_stops_output_without_scan_fallback() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new("t", &trace, vec![row(1, "one"), row(2, "two")]);
    let mut yielded = 0;
    let error = visit_selection(
        &provider,
        &source,
        "ID IN (1,2)",
        Limits::default(),
        None,
        None,
        &mut |_, _| {
            yielded += 1;
            Err(fault("fixture.consumer_failed", 5, "consumer failed"))
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.consumer_failed");
    assert_eq!(yielded, 1);
    assert_eq!(provider.lookups.borrow().len(), 2);
    assert_eq!(provider.scans(), 0);
}

#[test]
fn bare_equal_uses_provider_in_both_operand_orders_without_broadening_rhs_grammar() {
    for (predicate, expected_lookups) in [("ID=2", 1), ("2=ID", 1), ("ID=(1+1)", 0), ("ID=+2", 0)] {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let provider = Fixture::new("t", &trace, vec![row(1, "one"), row(2, "two")]);
        let result = run(
            &format!("SELECT ID FROM t WHERE {predicate}"),
            &[("t", &source, &provider)],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap();
        assert_eq!(result.rows, vec![vec![Scalar::Int(2)]], "{predicate}");
        assert_eq!(
            provider.lookups.borrow().len(),
            expected_lookups,
            "{predicate}"
        );
        assert_eq!(
            provider.scans(),
            usize::from(expected_lookups == 0),
            "{predicate}"
        );
    }
}

#[test]
fn cte_derived_and_recursive_delta_sources_never_inherit_root_equal_by_name() {
    for sql in [
        "WITH t AS (SELECT ID,Name FROM physical WHERE ID IN (2,1)) SELECT ID,Name FROM t WHERE ID IN (1,2)",
        "SELECT ID,Name FROM (SELECT ID,Name FROM physical WHERE ID IN (2,1)) t WHERE ID IN (1,2)",
    ] {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let shadow = schema(&["ID", "Name"]);
        let provider = Fixture::new("physical", &trace, vec![row(1, "one"), row(2, "two")]);
        let shadow_provider = Fixture::new("shadow", &trace, vec![row(99, "must not leak")]);
        let result = run(
            sql,
            &[
                ("physical", &source, &provider),
                ("t", &shadow, &shadow_provider),
            ],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap();
        assert_eq!(result.rows, vec![row(2, "two"), row(1, "one")]);
        assert_eq!(
            *provider.lookups.borrow(),
            vec![Scalar::Int(2), Scalar::Int(1)]
        );
        assert_eq!(provider.scans(), 0);
        assert!(shadow_provider.lookups.borrow().is_empty());
        assert_eq!(shadow_provider.scans(), 0);
    }
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new("physical", &trace, vec![row(1, "one")]);
    let result = run("WITH RECURSIVE r AS (SELECT ID FROM physical WHERE ID=1 UNION ALL SELECT ID+1 AS ID FROM r WHERE ID IN (1,2)) SELECT ID FROM r", &[("physical", &source, &provider)], &BTreeMap::new(), Limits::default(), None).unwrap();
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(2)],
            vec![Scalar::Int(3)]
        ]
    );
    assert_eq!(*provider.lookups.borrow(), vec![Scalar::Int(1)]);
    assert_eq!(provider.scans(), 0);
}

#[test]
fn registry_rejects_duplicate_and_foreign_source_identities_before_reads() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let foreign = schema(&["ID", "Name"]);
    let provider = Fixture::new("t", &trace, vec![row(1, "unread")]);
    let program = Program::compile("SELECT ID FROM t").unwrap();
    let sources = BTreeMap::from([("t".into(), &source)]);
    for registry in [
        vec![
            PreparedSource {
                schema: &source,
                provider: &provider
            };
            2
        ],
        vec![PreparedSource {
            schema: &foreign,
            provider: &provider,
        }],
    ] {
        let error = program
            .execute_providers_budgeted(
                &sources,
                &registry,
                &BTreeMap::new(),
                Limits::default(),
                None,
                None,
            )
            .unwrap_err();
        assert_eq!(error.code, "relational.invalid_binding");
        assert!(trace.borrow().is_empty());
    }
}

#[test]
fn schema_preflight_and_execution_share_the_original_remaining_work() {
    let trace = Trace::default();
    let source = schema(&["ID", "Name"]);
    let provider = Fixture::new("t", &trace, vec![row(1, "unread")]);
    let program = Program::compile("SELECT ID FROM t").unwrap();
    let sources = BTreeMap::from([("t".into(), &source)]);
    let registry = [PreparedSource {
        schema: &source,
        provider: &provider,
    }];
    let initial = 10000;
    let mut work = Work {
        left: initial,
        bytes: 1 << 20,
        deadline: Instant::now() + Duration::from_secs(10),
        cancel: None,
        shared: None,
        planning: true,
        selection_sources: vec![],
        providers: &registry,
    };
    program
        .execute_scoped(&sources, &BTreeMap::new(), &mut work)
        .unwrap();
    assert!(trace.borrow().is_empty());
    // The public lazy entry spends once before this exact schema pass. Give
    // it precisely enough for those phases, leaving nothing to start rows.
    let error = program
        .execute_providers_budgeted(
            &sources,
            &registry,
            &BTreeMap::new(),
            Limits {
                work: initial - work.left + 1,
                ..Limits::default()
            },
            None,
            None,
        )
        .unwrap_err();
    assert_eq!(error.code, "relational.resource_limit");
    assert!(trace.borrow().is_empty(), "execution must not reset Work");
}

#[test]
fn owned_join_right_rows_pay_the_same_full_copy_cost_as_provider_rows() {
    fn used(right: &Source) -> usize {
        let left = schema(&["ID"]);
        let sources = BTreeMap::from([("a".into(), &left), ("b".into(), right)]);
        let program = Program::compile("SELECT b.Name FROM a CROSS JOIN b").unwrap();
        let initial = 1 << 20;
        let mut work = Work {
            left: 10000,
            bytes: initial,
            deadline: Instant::now() + Duration::from_secs(10),
            cancel: None,
            shared: None,
            planning: false,
            selection_sources: vec![],
            providers: &[],
        };
        let plan = joins::prepare(
            &program.table,
            &program.alias,
            &program.joins,
            &sources,
            &BTreeMap::new(),
            &mut work,
            &program,
        )
        .unwrap();
        drop(plan);
        initial - work.bytes
    }
    let mut right = schema(&["ID", "Name"]);
    let empty = used(&right);
    right.rows.push(row(1, "copied"));
    assert_eq!(used(&right) - empty, 64 + 32 * 2 + "copied".len());
    assert_eq!(right.rows, vec![row(1, "copied")]);
}

#[test]
fn heterogeneous_join_keys_and_residuals_keep_first_candidate_type_errors() {
    for condition in ["a.ID=b.ID", "a.ID<b.ID", "a.ID=b.ID AND b.Name='x'"] {
        let trace = Trace::default();
        let sa = schema(&["ID", "Name"]);
        let sb = schema(&["ID", "Name"]);
        let a = Fixture::new("a", &trace, vec![row(1, "first"), row(2, "unread")]);
        let b = Fixture::new(
            "b",
            &trace,
            vec![
                vec![
                    Scalar::String("wrong type".into()),
                    Scalar::String("x".into()),
                ],
                row(1, "valid later"),
            ],
        );
        let error = run(
            &format!("SELECT a.ID FROM a JOIN b ON {condition}"),
            &[("a", &sa, &a), ("b", &sb, &b)],
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.code, "relational.type_mismatch", "{condition}");
        assert_eq!(
            *trace.borrow(),
            ["b.scan", "b.row", "b.row", "a.scan", "a.row"]
        );
    }
}

#[test]
fn in_original_work_or_deadline_failure_never_becomes_scan_fallback() {
    for limits in [
        Limits {
            work: 0,
            ..Limits::default()
        },
        Limits {
            deadline: Instant::now() - Duration::from_secs(1),
            ..Limits::default()
        },
    ] {
        let trace = Trace::default();
        let source = schema(&["ID", "Name"]);
        let provider = Fixture::new("t", &trace, vec![row(1, "unread")]);
        let error = visit_selection(
            &provider,
            &source,
            "ID IN (1,2)",
            limits,
            None,
            None,
            &mut |_, _| panic!("unexpected output"),
        )
        .unwrap_err();
        assert_eq!(error.code, "relational.resource_limit");
        assert!(trace.borrow().is_empty());
    }
}

#[test]
fn join_string_buffer_grows_for_later_rows_and_survives_null_and_numeric_cells() {
    let trace = Trace::default();
    let sa = schema(&["ID", "Name"]);
    let sb = schema(&["ID"]);
    let long = "larger".repeat(64);
    let a = Fixture::new(
        "a",
        &trace,
        vec![
            row(1, "x"),
            row(2, &long),
            vec![Scalar::Int(3), Scalar::Null],
            vec![Scalar::Int(4), Scalar::Int(9)],
            row(5, "reuse"),
        ],
    );
    let b = Fixture::new("b", &trace, vec![vec![Scalar::Int(1)]]);
    let result = run(
        "SELECT a.Name FROM a CROSS JOIN b",
        &[("a", &sa, &a), ("b", &sb, &b)],
        &BTreeMap::new(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::String("x".into())],
            vec![Scalar::String(long)],
            vec![Scalar::Null],
            vec![Scalar::Int(9)],
            vec![Scalar::String("reuse".into())],
        ]
    );
    assert_eq!(a.scans(), 1);
    assert_eq!(b.scans(), 1);
}

#[test]
fn selected_column_helper_uses_the_actual_alias_and_marks_ordinals() {
    let columns = vec!["ID".into(), "Name".into(), "Unused".into()];
    let mut selected = vec![false; columns.len()];
    joins::select(
        &Term::Column("actual.ID".into()),
        &columns,
        "actual",
        &mut selected,
    );
    joins::select(&Term::Ordinal(1), &columns, "actual", &mut selected);
    assert_eq!(selected, vec![true, true, false]);
}
