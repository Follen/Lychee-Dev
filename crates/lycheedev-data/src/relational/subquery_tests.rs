//! Executable source regressions for fixed Go 3.2.3 commit 3766f9cacb52e93a803a626187e2a1d54a445468.
//! Mapping: subquery_test.go/evaluate.go (IN, NULL, full RHS); correlation_test.go/
//! correlation.go (lexical frames); group_subquery_test.go/aggregate_subquery_test.go
//! (group visibility); dependencies.go/recursive.go/explain.go (query traversal).
//! Rust execution is NOT_RUN; rustfmt only checks source syntax and formatting.
use super::*;
use crate::resource::{Budget, Cost};
use std::{cell::RefCell, rc::Rc, sync::Arc};

type Trace = Rc<RefCell<Vec<String>>>;
struct Rows {
    name: &'static str,
    rows: Vec<Vec<Scalar>>,
    trace: Trace,
    equal: bool,
    fail_after: Option<usize>,
    cancel: Option<Arc<AtomicBool>>,
}
impl Rows {
    fn new(name: &'static str, rows: Vec<Vec<Scalar>>, trace: &Trace) -> Self {
        Self {
            name,
            rows,
            trace: trace.clone(),
            equal: false,
            fail_after: None,
            cancel: None,
        }
    }
    fn event(&self, what: &str) {
        self.trace
            .borrow_mut()
            .push(format!("{}.{what}", self.name));
    }
    fn scans(&self) -> usize {
        self.trace
            .borrow()
            .iter()
            .filter(|event| **event == format!("{}.scan", self.name))
            .count()
    }
}
impl RowProvider for Rows {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        self.event("scan");
        let mut scratch = vec![];
        for (index, row) in self.rows.iter().enumerate() {
            if self.fail_after == Some(index) {
                return Err(fault("fixture.rhs_error", 5, "RHS failed"));
            }
            if let Some(cancel) = &self.cancel {
                cancel.store(true, AtomicOrdering::Release);
            }
            scratch.clone_from(row);
            self.event("row");
            emit(&scratch)?;
            scratch.fill(Scalar::Null);
        }
        if self.fail_after == Some(self.rows.len()) {
            return Err(fault("fixture.rhs_error", 5, "RHS failed"));
        }
        Ok(())
    }
    fn scan_selected(&self, _: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
        self.event("selected");
        self.scan(emit)
    }
    fn has_equal(&self) -> bool {
        self.equal
    }
    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        self.event(&format!("equal:{value:?}"));
        if !equal_fold(column, "ID") {
            return Ok(EqualityOutcome::Declined);
        }
        for row in &self.rows {
            if value != &Scalar::Null
                && row[0] != Scalar::Null
                && compare(&row[0], value)? == Ordering::Equal
            {
                emit(row)?;
            }
        }
        Ok(EqualityOutcome::Handled)
    }
}
fn source(columns: &[&str], rows: Vec<Vec<Scalar>>) -> Source {
    Source {
        columns: columns.iter().map(|s| (*s).into()).collect(),
        rows,
        complete: true,
    }
}
fn pair(id: i64, name: &str) -> Vec<Scalar> {
    vec![Scalar::Int(id), Scalar::String(name.into())]
}
fn fixtures() -> BTreeMap<String, Source> {
    BTreeMap::from([
        (
            "a".into(),
            source(
                &["ID", "Name"],
                vec![pair(1, "a"), pair(2, "b"), pair(3, "c")],
            ),
        ),
        (
            "b".into(),
            source(
                &["ID", "Name"],
                vec![pair(1, "x"), pair(1, "y"), pair(3, "z")],
            ),
        ),
    ])
}
fn owned(sql: &str, sources: &BTreeMap<String, Source>) -> Result<QueryResult> {
    Program::compile(sql)?.execute(sources, &BTreeMap::new(), Limits::default(), None)
}
fn run(sql: &str) -> Result<QueryResult> {
    owned(sql, &fixtures())
}
fn ids(values: &[i64]) -> Vec<Vec<Scalar>> {
    values.iter().map(|n| vec![Scalar::Int(*n)]).collect()
}
fn lazy(
    sql: &str,
    a: &Rows,
    b: &Rows,
    limits: Limits,
    cancel: Option<&AtomicBool>,
    shared: Option<&Budget>,
) -> Result<QueryResult> {
    let a_schema = source(&["ID", "Name"], vec![]);
    let b_schema = source(&["ID", "Name"], vec![]);
    let sources = BTreeMap::from([("a".into(), &a_schema), ("b".into(), &b_schema)]);
    let providers = [
        PreparedSource {
            schema: &a_schema,
            provider: a,
        },
        PreparedSource {
            schema: &b_schema,
            provider: b,
        },
    ];
    Program::compile(sql)?.execute_providers_budgeted(
        &sources,
        &providers,
        &BTreeMap::new(),
        limits,
        cancel,
        shared,
    )
}
fn providers() -> (Trace, Rows, Rows) {
    let trace = Trace::default();
    let a = Rows::new("a", vec![pair(1, "a"), pair(2, "b"), pair(3, "c")], &trace);
    let b = Rows::new("b", vec![pair(1, "x"), pair(1, "y"), pair(3, "z")], &trace);
    (trace, a, b)
}

#[test]
fn in_and_not_in_preserve_outer_order_duplicates_nulls_and_empty_rhs() {
    assert_eq!(
        run("SELECT ID FROM a WHERE ID IN (SELECT ID FROM b)")
            .unwrap()
            .rows,
        ids(&[1, 3])
    );
    assert_eq!(
        run("SELECT ID FROM a WHERE ID NOT IN (SELECT ID FROM b)")
            .unwrap()
            .rows,
        ids(&[2])
    );
    assert_eq!(run("SELECT ID IN (SELECT NULL FROM b), ID NOT IN (SELECT NULL FROM b), NULL IN (SELECT ID FROM b WHERE FALSE), NULL NOT IN (SELECT ID FROM b WHERE FALSE) FROM a LIMIT 1").unwrap().rows,
        vec![vec![Scalar::Null,Scalar::Null,Scalar::Bool(false),Scalar::Bool(true)]]);
    assert_eq!(
        run("SELECT ID IN (SELECT ID FROM b UNION ALL SELECT NULL FROM b) FROM a")
            .unwrap()
            .rows,
        vec![
            vec![Scalar::Bool(true)],
            vec![Scalar::Null],
            vec![Scalar::Bool(true)]
        ]
    );
}
#[test]
fn correlated_in_and_nested_frames_resolve_nearest_then_grandparent() {
    for sql in [
        "SELECT a.ID FROM a WHERE a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID)",
        "SELECT a.ID FROM a WHERE a.ID IN (SELECT b.ID FROM b WHERE b.ID IN (SELECT c.ID FROM b c WHERE c.ID=a.ID AND c.ID=b.ID))",
    ] {
        assert_eq!(run(sql).unwrap().rows, ids(&[1, 3]), "{sql}");
    }
    assert_eq!(
        run("SELECT a.ID, a.ID IN (SELECT ID FROM b a WHERE a.ID=3) FROM a")
            .unwrap()
            .rows,
        vec![
            vec![Scalar::Int(1), Scalar::Bool(false)],
            vec![Scalar::Int(2), Scalar::Bool(false)],
            vec![Scalar::Int(3), Scalar::Bool(true)]
        ]
    );
}
#[test]
fn unqualified_outer_fallback_works_but_local_qualifier_and_ambiguity_do_not_fall_through() {
    let mut sources = fixtures();
    sources.insert(
        "only_name".into(),
        source(&["Name"], vec![vec![Scalar::String("x".into())]]),
    );
    assert_eq!(
        owned(
            "SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM only_name)",
            &sources
        )
        .unwrap()
        .rows,
        ids(&[1, 2, 3])
    );
    for sql in [
        "SELECT x.ID FROM a x WHERE x.ID IN (SELECT x.ID FROM only_name x)",
        "SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM b x JOIN b y ON x.ID=y.ID)",
    ] {
        assert_eq!(
            owned(sql, &sources).unwrap_err().code,
            "relational.invalid_binding",
            "{sql}"
        );
    }
}
#[test]
fn schema_preflight_visits_subqueries_in_all_positions_before_any_provider() {
    for sql in [
        "SELECT a.ID IN (SELECT Missing FROM b) FROM a WHERE FALSE",
        "SELECT CASE WHEN TRUE THEN TRUE ELSE a.ID IN (SELECT ID,Name FROM b) END FROM a",
        "SELECT a.ID FROM a WHERE a.ID IN (SELECT ID,Name FROM b)",
        "SELECT a.ID FROM a ORDER BY a.ID IN (SELECT ID,Name FROM b)",
        "SELECT COUNT(*) FROM a GROUP BY a.ID IN (SELECT ID,Name FROM b)",
        "SELECT a.ID FROM a JOIN b ON a.ID IN (SELECT ID,Name FROM b)",
        "SELECT ID FROM a UNION ALL SELECT ID FROM a WHERE ID IN (SELECT ID,Name FROM b)",
        "WITH c AS (SELECT a.ID IN (SELECT ID,Name FROM b) FROM a) SELECT ID FROM a",
        "SELECT * FROM (SELECT a.ID IN (SELECT ID,Name FROM b) FROM a) d",
    ] {
        let (trace, mut a, mut b) = providers();
        a.fail_after = Some(0);
        b.fail_after = Some(0);
        let error = lazy(sql, &a, &b, Limits::default(), None, None).unwrap_err();
        assert_eq!(
            error.code,
            if sql.contains("Missing") {
                "relational.invalid_binding"
            } else {
                "relational.invalid_cardinality"
            },
            "{sql}"
        );
        assert!(trace.borrow().is_empty(), "{sql}: {:?}", trace.borrow());
    }
}
#[test]
fn cardinality_and_group_scope_are_rejected_even_with_empty_owned_input() {
    let mut sources = fixtures();
    sources.get_mut("a").unwrap().rows.clear();
    for sql in [
        "SELECT ID IN (SELECT ID,Name FROM b) FROM a",
        "SELECT CASE WHEN TRUE THEN TRUE ELSE ID IN (SELECT ID,Name FROM b) END FROM a",
    ] {
        assert_eq!(
            owned(sql, &sources).unwrap_err().code,
            "relational.invalid_cardinality"
        );
    }
    assert_eq!(
        owned("SELECT COUNT(*),1 IN (SELECT a.ID FROM b) FROM a", &sources)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
}
#[test]
fn rhs_schema_is_bound_before_the_in_left_operand() {
    assert_eq!(
        run("SELECT Missing IN (SELECT ID,Name FROM b) FROM a")
            .unwrap_err()
            .code,
        "relational.invalid_cardinality"
    );
}
#[test]
fn complete_rhs_is_reexecuted_per_outer_row_without_global_cache() {
    let (trace, a, b) = providers();
    assert_eq!(
        lazy(
            "SELECT ID FROM a WHERE ID IN (SELECT ID FROM b)",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .rows,
        ids(&[1, 3])
    );
    assert_eq!(a.scans(), 1);
    assert_eq!(b.scans(), 3);
    assert_eq!(
        &*trace.borrow(),
        &[
            "a.scan",
            "a.row",
            "b.selected",
            "b.scan",
            "b.row",
            "b.row",
            "b.row",
            "a.row",
            "b.selected",
            "b.scan",
            "b.row",
            "b.row",
            "b.row",
            "a.row",
            "b.selected",
            "b.scan",
            "b.row",
            "b.row",
            "b.row",
        ]
    );
}
#[test]
fn rhs_provider_failure_precedes_match_comparison_and_never_falls_back() {
    let (_, a, mut b) = providers();
    b.rows = vec![vec![Scalar::String("wrong type".into()), Scalar::Null]];
    b.fail_after = Some(1);
    assert_eq!(
        lazy(
            "SELECT ID IN (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap_err()
        .code,
        "fixture.rhs_error"
    );
    assert_eq!(a.scans(), 1);
    assert_eq!(b.scans(), 1);
}
#[test]
fn later_incompatible_rhs_value_is_not_hidden_by_an_earlier_match() {
    let (_, a, mut b) = providers();
    b.rows = vec![
        pair(1, "match"),
        vec![Scalar::String("bad".into()), Scalar::Null],
    ];
    assert_eq!(
        lazy(
            "SELECT ID IN (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap_err()
        .code,
        "relational.type_mismatch"
    );
    assert_eq!(b.scans(), 1);
}
#[test]
fn case_runtime_laziness_does_not_skip_schema_validation() {
    let (_, a, mut b) = providers();
    b.fail_after = Some(0);
    assert_eq!(
        lazy(
            "SELECT CASE WHEN TRUE THEN ID ELSE ID IN (SELECT ID FROM b) END FROM a",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .rows,
        ids(&[1, 2, 3])
    );
    assert_eq!(b.scans(), 0);
}
#[test]
fn original_numeric_list_in_inside_rhs_keeps_distinct_candidate_callbacks() {
    let (trace, a, mut b) = providers();
    b.equal = true;
    assert_eq!(
        lazy(
            "SELECT ID FROM a WHERE ID IN (SELECT ID FROM b WHERE ID IN (3,1,3,NULL))",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .rows,
        ids(&[1, 3])
    );
    assert_eq!(a.scans(), 1);
    assert_eq!(b.scans(), 0);
    let lookups: Vec<_> = trace
        .borrow()
        .iter()
        .filter(|s| s.contains(".equal:"))
        .cloned()
        .collect();
    assert_eq!(
        lookups,
        vec![
            "b.equal:Int(3)",
            "b.equal:Int(1)",
            "b.equal:Int(3)",
            "b.equal:Int(1)",
            "b.equal:Int(3)",
            "b.equal:Int(1)"
        ]
    );
}
#[test]
fn outer_correlated_in_list_is_a_scan_predicate_not_an_inner_index_binding() {
    let (_, a, mut b) = providers();
    b.equal = true;
    assert_eq!(
        lazy(
            "SELECT a.ID FROM a WHERE a.ID IN (SELECT b.ID FROM b WHERE a.ID IN (1,3))",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .rows,
        ids(&[1, 3])
    );
    assert_eq!(b.scans(), 3);
}
#[test]
fn cte_derived_and_union_rhs_keep_lexical_sources_and_correlation() {
    for sql in [
        "WITH c AS (SELECT ID FROM b) SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM c)",
        "SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM (SELECT ID FROM b WHERE b.ID=a.ID) d)",
        "SELECT a.ID FROM a WHERE a.ID IN (WITH c AS (SELECT ID FROM b WHERE b.ID=a.ID) SELECT ID FROM c)",
        "SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM b WHERE ID=1 UNION ALL SELECT ID FROM b WHERE ID=3)",
    ] {
        assert_eq!(run(sql).unwrap().rows, ids(&[1, 3]), "{sql}");
    }
}
#[test]
fn nested_parameters_and_physical_dependencies_reach_preparation() {
    let program=Program::compile("WITH c AS (SELECT ID FROM b) SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM c WHERE ID=:wanted UNION ALL SELECT ID FROM other WHERE ID=:other)").unwrap();
    assert_eq!(
        program.source_tables().into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(["a", "b", "other"])
    );
    assert_eq!(
        program.parameters,
        BTreeSet::from(["wanted".into(), "other".into()])
    );
    assert_eq!(
        program
            .validate_parameters(&BTreeMap::from([("wanted".into(), Scalar::Int(1))]))
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    let mut sources = fixtures();
    sources.insert("other".into(), source(&["ID"], ids(&[3])));
    assert_eq!(
        program
            .execute(
                &sources,
                &BTreeMap::from([
                    ("wanted".into(), Scalar::Int(1)),
                    ("other".into(), Scalar::Int(3))
                ]),
                Limits::default(),
                None
            )
            .unwrap()
            .rows,
        ids(&[1, 3])
    );
    let sibling=Program::compile("SELECT ID IN (WITH hidden AS (SELECT ID FROM b) SELECT ID FROM hidden), ID IN (SELECT ID FROM hidden) FROM a").unwrap();
    assert_eq!(
        sibling.source_tables().into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from(["a", "b", "hidden"])
    );
}
#[test]
fn recursive_reference_inside_rhs_reads_only_previous_delta() {
    let sql = "WITH RECURSIVE r AS (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT a.ID FROM a WHERE a.ID-1 IN (SELECT ID FROM r)) SELECT ID FROM r";
    assert_eq!(run(sql).unwrap().rows, ids(&[1, 2, 3]));
    assert_eq!(Program::compile(sql).unwrap().source_tables(), vec!["a"]);
}
#[test]
fn recursive_rhs_scope_does_not_reset_work_or_hide_nontermination() {
    let sources = fixtures();
    let query=Program::compile("WITH RECURSIVE r AS (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM r)) SELECT ID FROM r").unwrap();
    assert_eq!(
        query
            .execute(
                &sources,
                &BTreeMap::new(),
                Limits {
                    work: 4000,
                    ..Limits::default()
                },
                None
            )
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
}
#[test]
fn group_outputs_having_and_order_correlate_only_to_group_keys() {
    assert_eq!(run("SELECT a.ID, a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID),COUNT(*) FROM a GROUP BY a.ID ORDER BY a.ID").unwrap().rows,
        vec![vec![Scalar::Int(1),Scalar::Bool(true),Scalar::Int(1)],vec![Scalar::Int(2),Scalar::Bool(false),Scalar::Int(1)],vec![Scalar::Int(3),Scalar::Bool(true),Scalar::Int(1)]]);
    assert_eq!(run("SELECT a.ID FROM a GROUP BY a.ID HAVING a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID) ORDER BY a.ID").unwrap().rows,ids(&[1,3]));
    assert_eq!(run("SELECT a.ID FROM a GROUP BY a.ID ORDER BY a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID) DESC,a.ID").unwrap().rows,ids(&[1,3,2]));
}
#[test]
fn group_scope_never_exposes_a_representative_non_key_row() {
    for sql in [
        "SELECT COUNT(*),'a' IN (SELECT a.Name FROM b) FROM a",
        "SELECT a.ID,'a' IN (SELECT a.Name FROM b) FROM a GROUP BY a.ID",
        "SELECT a.ID FROM a GROUP BY a.ID HAVING 'a' IN (SELECT a.Name FROM b)",
        "SELECT a.ID FROM a GROUP BY a.ID ORDER BY 'a' IN (SELECT a.Name FROM b)",
        "SELECT a.ID+1,1 IN (SELECT a.ID FROM b) FROM a GROUP BY a.ID+1",
    ] {
        let (trace, a, b) = providers();
        assert_eq!(
            lazy(sql, &a, &b, Limits::default(), None, None)
                .unwrap_err()
                .code,
            "relational.invalid_binding",
            "{sql}"
        );
        assert!(trace.borrow().is_empty());
    }
}
#[test]
fn aggregate_input_subqueries_use_raw_rows_and_inner_aggregates_stay_inner() {
    assert_eq!(run("SELECT COUNT(CASE WHEN a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID) THEN a.ID END) FROM a").unwrap().rows,ids(&[2]));
    assert_eq!(
        run("SELECT a.ID FROM a WHERE a.ID IN (SELECT MAX(b.ID) FROM b WHERE b.ID=a.ID)")
            .unwrap()
            .rows,
        ids(&[1, 3])
    );
    assert_eq!(
        run("SELECT COUNT(*),3 IN (SELECT COUNT(*) FROM b) FROM a WHERE FALSE")
            .unwrap()
            .rows,
        vec![vec![Scalar::Int(0), Scalar::Bool(true)]]
    );
    assert_eq!(
        run("SELECT a.ID FROM a WHERE a.ID IN (SELECT a.ID FROM b GROUP BY b.ID)")
            .unwrap()
            .rows,
        ids(&[1, 2, 3])
    );
}
#[test]
fn group_key_subquery_is_reused_without_extra_output_evaluation() {
    let (_, a, b) = providers();
    let rows = lazy(
        "SELECT ID IN (SELECT ID FROM b),COUNT(*) FROM a GROUP BY ID IN (SELECT ID FROM b)",
        &a,
        &b,
        Limits::default(),
        None,
        None,
    )
    .unwrap()
    .rows;
    assert_eq!(
        rows,
        vec![
            vec![Scalar::Bool(true), Scalar::Int(2)],
            vec![Scalar::Bool(false), Scalar::Int(1)]
        ]
    );
    assert_eq!(b.scans(), 3);
}
#[test]
fn join_on_subquery_sees_bound_prefix_and_rights_copy_before_left() {
    let (trace, a, b) = providers();
    assert_eq!(lazy("SELECT a.ID,b.Name FROM a JOIN b ON a.ID=b.ID AND a.ID IN (SELECT x.ID FROM b x WHERE x.ID=a.ID)",&a,&b,Limits::default(),None,None).unwrap().rows,
        vec![pair(1,"x"),pair(1,"y"),pair(3,"z")]);
    let events = trace.borrow();
    let left = events.iter().position(|s| s == "a.scan").unwrap();
    assert!(events[..left].iter().any(|s| s == "b.scan"));
    assert_eq!(a.scans(), 1);
}
#[test]
fn explain_in_nodes_bind_all_nested_schemas_without_provider_calls() {
    let (trace, a, b) = providers();
    let result=lazy("EXPLAIN SELECT a.ID IN (SELECT ID FROM b) FROM a WHERE a.ID IN (SELECT b.ID FROM b WHERE b.ID=a.ID)",&a,&b,Limits::default(),None,None).unwrap();
    let plan = result.plan.unwrap();
    assert_eq!(
        plan.steps
            .iter()
            .filter(|s| s.operation == "expression-subquery" && s.detail == "operator")
            .count(),
        2
    );
    assert!(plan.steps.iter().any(|s| s.operation == "filter"));
    assert!(trace.borrow().is_empty());
    assert_eq!(
        lazy(
            "EXPLAIN SELECT ID IN (SELECT ID,Name FROM b) FROM a",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap_err()
        .code,
        "relational.invalid_cardinality"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn subquery_cancel_and_deadline_propagate_without_restart_or_partial_result() {
    let (_, a, mut b) = providers();
    let cancel = Arc::new(AtomicBool::new(false));
    b.cancel = Some(cancel.clone());
    assert_eq!(
        lazy(
            "SELECT ID IN (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits::default(),
            Some(&cancel),
            None
        )
        .unwrap_err()
        .code,
        "operation.cancelled"
    );
    assert_eq!(a.scans(), 1);
    assert_eq!(b.scans(), 1);
    let (trace, a, b) = providers();
    assert_eq!(
        lazy(
            "SELECT ID IN (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits {
                deadline: Instant::now() - Duration::from_secs(1),
                ..Limits::default()
            },
            None,
            None
        )
        .unwrap_err()
        .code,
        "relational.resource_limit"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn subquery_uses_one_work_and_memory_allowance_including_preflight() {
    let (_, a, b) = providers();
    let sources = BTreeMap::from([
        ("a".into(), source(&["ID", "Name"], vec![])),
        ("b".into(), source(&["ID", "Name"], vec![])),
    ]);
    let inputs = sources
        .iter()
        .map(|(name, source)| (name.clone(), source))
        .collect();
    let providers = [
        PreparedSource {
            schema: &sources["a"],
            provider: &a,
        },
        PreparedSource {
            schema: &sources["b"],
            provider: &b,
        },
    ];
    let program = Program::compile("SELECT ID IN (SELECT ID FROM b GROUP BY ID) FROM a").unwrap();
    let limits = Limits::default();
    let mut work = Work {
        left: limits.work,
        bytes: limits.memory_bytes,
        deadline: limits.deadline,
        cancel: None,
        shared: None,
        planning: true,
        selection_sources: vec![],
        providers: &providers,
        resolver: None,
        roots: BTreeMap::new(),
        measure: false,
        scans: Vec::new(),
    };
    program
        .execute_scoped(&inputs, &BTreeMap::new(), &mut work)
        .unwrap();
    let (left, bytes) = (work.left, work.bytes);
    assert!(left < limits.work);
    assert!(bytes < limits.memory_bytes);
    work.planning = false;
    program
        .execute_scoped(&inputs, &BTreeMap::new(), &mut work)
        .unwrap();
    assert!(work.left < left);
    assert!(work.bytes < bytes);
    for limits in [
        Limits {
            work: 1,
            ..Limits::default()
        },
        Limits {
            memory_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            lazy(
                "SELECT ID IN (SELECT ID FROM b) FROM a",
                &a,
                &b,
                limits,
                None,
                None
            )
            .unwrap_err()
            .code,
            "relational.resource_limit"
        );
    }
}
#[test]
fn shared_safety_budget_failure_is_not_reclassified_as_optional_scan_fallback() {
    let (_, a, b) = providers();
    let shared = Budget::new(
        Cost {
            decode_work: 1,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    );
    assert_eq!(
        lazy(
            "SELECT ID IN (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits::default(),
            None,
            Some(&shared)
        )
        .unwrap_err()
        .code,
        "records.query_resource_budget"
    );
    assert_eq!(a.scans(), 0);
    assert_eq!(b.scans(), 0);
}
#[test]
fn unicode_table_cte_alias_and_column_binding_share_go_identifier_rules() {
    let sources = BTreeMap::from([
        (
            "K".into(),
            source(&["Σ", "中文"], vec![vec![Scalar::Int(7), Scalar::Int(9)]]),
        ),
        (
            "physical".into(),
            source(&["Σ", "中文"], vec![vec![Scalar::Int(7), Scalar::Int(9)]]),
        ),
    ]);
    for sql in [
        "SELECT σ,中文 FROM K",
        "SELECT ſ.ς,ſ.中文 FROM K S",
        "WITH K AS (SELECT σ,中文 FROM physical) SELECT ς,中文 FROM K",
    ] {
        assert_eq!(
            owned(sql, &sources).unwrap().rows,
            vec![vec![Scalar::Int(7), Scalar::Int(9)]],
            "{sql}"
        );
    }
    assert_eq!(
        owned("WITH K AS (SELECT σ FROM K) SELECT σ FROM K", &sources)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
}
#[test]
fn unicode_lower_key_duplicates_and_equal_fold_ambiguity_are_distinct() {
    // Go duplicate-schema keys use ToLower, not EqualFold's canonical orbit.
    for columns in [["K", "K"], ["I", "İ"], ["Σ", "σ"]] {
        let sources = BTreeMap::from([("t".into(), source(&columns, vec![]))]);
        assert_eq!(
            owned("SELECT * FROM t", &sources).unwrap_err().code,
            "relational.invalid_binding"
        );
    }
    for columns in [["S", "ſ"], ["Σ", "ς"]] {
        let sources = BTreeMap::from([("t".into(), source(&columns, vec![]))]);
        // Schema keys differ, but selecting either spelling is ambiguous.
        assert_eq!(
            owned(&format!("SELECT {} FROM t", columns[0]), &sources)
                .unwrap_err()
                .code,
            "relational.invalid_binding"
        );
        assert_eq!(owned("SELECT 1 FROM t", &sources).unwrap().rows, ids(&[]));
    }
    for sql in [
        "WITH K AS (SELECT ID FROM a), K AS (SELECT ID FROM b) SELECT ID FROM K",
        "WITH I AS (SELECT ID FROM a), İ AS (SELECT ID FROM b) SELECT ID FROM I",
        "SELECT I.ID FROM a I JOIN b İ ON I.ID=İ.ID",
        // Different alias keys are allowed; EqualFold makes a selected ID ambiguous.
        "SELECT S.ID FROM a S JOIN b ſ ON S.ID=ſ.ID",
    ] {
        assert_eq!(
            run(sql).unwrap_err().code,
            if sql.starts_with("WITH") {
                "relational.invalid_syntax"
            } else {
                "relational.invalid_binding"
            },
            "{sql}"
        );
    }
    assert_eq!(
        run("WITH S AS (SELECT ID FROM a), ſ AS (SELECT ID FROM b) SELECT ID FROM S")
            .unwrap()
            .rows,
        ids(&[1, 2, 3])
    );
    let sources = BTreeMap::from([
        ("K".into(), source(&["ID"], ids(&[1]))),
        ("K".into(), source(&["ID"], ids(&[2]))),
    ]);
    assert_eq!(
        owned("SELECT ID FROM K", &sources).unwrap_err().code,
        "relational.invalid_binding"
    );
}
#[test]
fn dotted_dotless_i_field_comparison_differs_from_lower_map_identity() {
    for (field, matching, wrong) in [("I", "i", "İ"), ("ı", "ı", "i"), ("İ", "İ", "I")] {
        let sources = BTreeMap::from([(
            "中文".into(),
            source(&[field, "列"], vec![vec![Scalar::Int(1), Scalar::Int(4)]]),
        )]);
        assert_eq!(
            owned(&format!("SELECT {matching},列 FROM 中文"), &sources)
                .unwrap()
                .rows,
            vec![vec![Scalar::Int(1), Scalar::Int(4)]]
        );
        assert_eq!(
            owned(&format!("SELECT {wrong} FROM 中文"), &sources)
                .unwrap_err()
                .code,
            "relational.invalid_binding"
        );
    }
    let sources = BTreeMap::from([
        ("I".into(), source(&["ID"], ids(&[1]))),
        ("ı".into(), source(&["ID"], ids(&[2]))),
    ]);
    assert_eq!(
        owned(
            "SELECT ID FROM i UNION ALL SELECT ID FROM ı UNION ALL SELECT ID FROM İ",
            &sources
        )
        .unwrap()
        .rows,
        ids(&[1, 2, 1])
    );
}
#[test]
fn unicode_correlation_uses_the_same_fold_as_local_binding() {
    let sources = BTreeMap::from([
        ("K".into(), source(&["Σ"], ids(&[1, 2]))),
        ("b".into(), source(&["ID"], ids(&[1]))),
    ]);
    assert_eq!(
        owned(
            "SELECT S.σ FROM K S WHERE S.ς IN (SELECT ID FROM b WHERE ID=ſ.Σ)",
            &sources
        )
        .unwrap()
        .rows,
        ids(&[1])
    );
}
#[test]
fn star_and_unresolved_output_alias_disable_projected_provider_scan() {
    for sql in [
        "SELECT * FROM a",
        "SELECT ID AS renamed FROM a ORDER BY renamed",
        "SELECT a.* FROM a JOIN b ON a.ID=b.ID",
    ] {
        let (trace, a, b) = providers();
        lazy(sql, &a, &b, Limits::default(), None, None).unwrap();
        assert!(
            !trace
                .borrow()
                .iter()
                .any(|event| event.ends_with(".selected")),
            "{sql}"
        );
    }
}

#[test]
fn grouping_compares_bound_column_identity_and_unicode_spelling() {
    let sources = BTreeMap::from([
        ("K".into(), source(&["Σ"], ids(&[1, 2]))),
        ("b".into(), source(&["ID"], ids(&[1]))),
    ]);
    assert_eq!(
        owned(
            "SELECT ς, σ IN (SELECT ID FROM b WHERE ID=S.Σ) FROM K S GROUP BY S.σ ORDER BY Σ",
            &sources
        )
        .unwrap()
        .rows,
        vec![
            vec![Scalar::Int(1), Scalar::Bool(true)],
            vec![Scalar::Int(2), Scalar::Bool(false)]
        ]
    );
    assert_eq!(
        run("SELECT ID+1 FROM a GROUP BY a.ID+1 ORDER BY ID+1")
            .unwrap()
            .rows,
        ids(&[2, 3, 4])
    );
}
#[test]
fn top_level_union_tail_subquery_preserves_fixed_go_unsupported_boundary() {
    let (trace, a, b) = providers();
    assert_eq!(
        lazy(
            "SELECT ID FROM a UNION ALL SELECT ID FROM b ORDER BY ID IN (SELECT ID FROM b)",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap_err()
        .code,
        "relational.unsupported"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn where_preflight_error_precedes_having_and_group_subquery_errors() {
    assert_eq!(
        run("SELECT COUNT(*) FROM a WHERE Missing=1 GROUP BY ID IN (SELECT ID,Name FROM b)")
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert_eq!(
        run("SELECT COUNT(*) FROM a WHERE ID IN (SELECT ID,Name FROM b) GROUP BY Missing")
            .unwrap_err()
            .code,
        "relational.invalid_cardinality"
    );
}

#[test]
fn output_names_use_canonical_local_fields_and_outer_expression_names() {
    let sources = BTreeMap::from([(
        "K".into(),
        source(&["Σ", "中文"], vec![vec![Scalar::Int(7), Scalar::Int(9)]]),
    )]);
    assert_eq!(
        owned("SELECT σ,中文 FROM K", &sources).unwrap().columns,
        vec!["Σ", "中文"]
    );
    assert_eq!(
        run("SELECT a.ID FROM a WHERE a.ID IN (SELECT column_1 FROM (SELECT a.ID FROM b) d)")
            .unwrap()
            .rows,
        ids(&[1, 2, 3])
    );
}

#[test]
fn bare_and_static_names_share_one_physical_dependency_and_provider_identity() {
    let program =
        Program::compile("SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM static.a)").unwrap();
    assert_eq!(program.source_tables().len(), 1);
    let (_, a, b) = providers();
    assert_eq!(
        lazy(
            "SELECT a.ID FROM a WHERE a.ID IN (SELECT ID FROM static.a)",
            &a,
            &b,
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .rows,
        ids(&[1, 2, 3])
    );
    assert_eq!(a.scans(), 4);
    assert_eq!(b.scans(), 0);
    let sources = BTreeMap::from([("static.a".into(), source(&["ID"], ids(&[7])))]);
    assert_eq!(
        owned(
            "SELECT ID FROM a UNION ALL SELECT ID FROM STATIC.a",
            &sources
        )
        .unwrap()
        .rows,
        ids(&[7, 7])
    );
}
#[test]
fn explicit_static_root_bypasses_cte_but_forward_reserved_names_never_fall_through() {
    assert_eq!(run("WITH a AS (SELECT ID FROM b WHERE ID=3) SELECT ID FROM static.a UNION ALL SELECT ID FROM a").unwrap().rows,ids(&[1,2,3,3]));
    assert_eq!(
        run("WITH a AS (SELECT ID FROM static.a) SELECT ID FROM a")
            .unwrap()
            .rows,
        ids(&[1, 2, 3])
    );
    let query = Program::compile(
        "WITH a AS (SELECT ID FROM b) SELECT ID FROM static.a UNION ALL SELECT ID FROM a",
    )
    .unwrap();
    assert_eq!(
        query
            .source_tables()
            .into_iter()
            .map(|name| compound::physical_key(name).unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["static.a".into(), "static.b".into()])
    );
    let (trace, a, b) = providers();
    for sql in [
        "WITH x AS (SELECT ID FROM a), a AS (SELECT ID FROM b) SELECT ID FROM x",
        "WITH a AS (SELECT ID FROM a) SELECT ID FROM a",
    ] {
        assert_eq!(
            lazy(sql, &a, &b, Limits::default(), None, None)
                .unwrap_err()
                .code,
            "relational.invalid_binding"
        );
        assert!(trace.borrow().is_empty());
    }
}
#[test]
fn duplicate_canonical_roots_are_rejected_before_provider_reads() {
    let sources = BTreeMap::from([
        ("a".into(), source(&["ID"], ids(&[1]))),
        ("static.a".into(), source(&["ID"], ids(&[2]))),
    ]);
    assert_eq!(
        owned("SELECT ID FROM a", &sources).unwrap_err().code,
        "relational.invalid_binding"
    );
    let (trace, a, b) = providers();
    let sa = source(&["ID", "Name"], vec![]);
    let sb = source(&["ID", "Name"], vec![]);
    let inputs = BTreeMap::from([("a".into(), &sa), ("static.a".into(), &sb)]);
    let providers = [
        PreparedSource {
            schema: &sa,
            provider: &a,
        },
        PreparedSource {
            schema: &sb,
            provider: &b,
        },
    ];
    assert_eq!(
        Program::compile("SELECT ID FROM a")
            .unwrap()
            .execute_providers_budgeted(
                &inputs,
                &providers,
                &BTreeMap::new(),
                Limits::default(),
                None,
                None
            )
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn provider_schema_accepts_4096_fields_and_rejects_4097_before_scan() {
    for width in [4096usize, 4097] {
        let trace = Trace::default();
        let schema = Source {
            columns: (0..width).map(|n| format!("c{n}")).collect(),
            rows: vec![],
            complete: true,
        };
        let provider = Rows::new("wide", vec![vec![Scalar::Int(1); width]], &trace);
        let sources = BTreeMap::from([("wide".into(), &schema)]);
        let providers = [PreparedSource {
            schema: &schema,
            provider: &provider,
        }];
        let result = Program::compile("SELECT c4095 FROM wide")
            .unwrap()
            .execute_providers_budgeted(
                &sources,
                &providers,
                &BTreeMap::new(),
                Limits::default(),
                None,
                None,
            );
        if width == 4096 {
            assert_eq!(result.unwrap().rows, ids(&[1]));
            assert_eq!(provider.scans(), 1);
        } else {
            assert_eq!(result.unwrap_err().code, "relational.invalid_binding");
            assert!(trace.borrow().is_empty());
        }
    }
}
#[test]
fn joined_schema_total_is_4096_and_projection_group_counts_are_not_1024() {
    for right_width in [2048usize, 2049] {
        let trace = Trace::default();
        let a_schema = Source {
            columns: (0..2048).map(|n| format!("c{n}")).collect(),
            rows: vec![],
            complete: true,
        };
        let b_schema = Source {
            columns: (0..right_width).map(|n| format!("d{n}")).collect(),
            rows: vec![],
            complete: true,
        };
        let a = Rows::new("a", vec![vec![Scalar::Int(1); 2048]], &trace);
        let b = Rows::new("b", vec![vec![Scalar::Int(2); right_width]], &trace);
        let sources = BTreeMap::from([("a".into(), &a_schema), ("b".into(), &b_schema)]);
        let providers = [
            PreparedSource {
                schema: &a_schema,
                provider: &a,
            },
            PreparedSource {
                schema: &b_schema,
                provider: &b,
            },
        ];
        let result = Program::compile("SELECT a.c2047,b.d2047 FROM a CROSS JOIN b")
            .unwrap()
            .execute_providers_budgeted(
                &sources,
                &providers,
                &BTreeMap::new(),
                Limits::default(),
                None,
                None,
            );
        if right_width == 2048 {
            assert_eq!(
                result.unwrap().rows,
                vec![vec![Scalar::Int(1), Scalar::Int(2)]]
            );
        } else {
            assert_eq!(result.unwrap_err().code, "relational.invalid_binding");
            assert!(trace.borrow().is_empty());
        }
    }
    let names = (0..1025).map(|n| format!("c{n}")).collect::<Vec<_>>();
    let sources = BTreeMap::from([(
        "wide".into(),
        Source {
            columns: names.clone(),
            rows: vec![],
            complete: true,
        },
    )]);
    let fields = names.join(",");
    let result = owned(
        &format!("SELECT {fields} FROM wide GROUP BY {fields}"),
        &sources,
    )
    .unwrap();
    assert_eq!(result.columns.len(), 1025);
    assert!(result.rows.is_empty());
    let too_many = vec!["1"; 32769].join(",");
    assert_eq!(
        Program::compile(&format!("SELECT {too_many} FROM wide"))
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
}

#[test]
fn incomplete_nested_source_marks_outer_result_incomplete() {
    let mut sources = fixtures();
    sources.get_mut("b").unwrap().complete = false;
    let result = owned("SELECT ID FROM a WHERE ID IN (SELECT ID FROM b)", &sources).unwrap();
    assert_eq!(result.rows, ids(&[1, 3]));
    assert!(!result.complete);
}

#[test]
fn scalar_and_exists_match_fixed_go_expression_cases() {
    for (sql, expected) in [
        (
            "SELECT (SELECT Name FROM b WHERE ID=3) FROM a WHERE ID=1",
            vec![vec![Scalar::String("z".into())]],
        ),
        (
            "SELECT (SELECT Name FROM b WHERE ID=9) FROM a WHERE ID=1",
            vec![vec![Scalar::Null]],
        ),
        (
            "SELECT ID FROM a WHERE EXISTS (SELECT ID,Name FROM b WHERE ID=3) ORDER BY ID",
            ids(&[1, 2, 3]),
        ),
        (
            "SELECT ID FROM a WHERE NOT EXISTS (SELECT ID FROM b WHERE b.ID=a.ID) ORDER BY ID",
            ids(&[2]),
        ),
        (
            "SELECT EXISTS (SELECT NULL FROM b), NOT EXISTS (SELECT NULL FROM b WHERE FALSE) FROM a WHERE ID=1",
            vec![vec![Scalar::Bool(true), Scalar::Bool(true)]],
        ),
    ] {
        assert_eq!(run(sql).unwrap().rows, expected, "{sql}");
    }
}
#[test]
fn scalar_cardinality_and_empty_input_schema_are_not_deferred() {
    for sql in [
        "SELECT (SELECT ID FROM b) FROM a",
        "SELECT (SELECT ID,Name FROM b WHERE ID=9) FROM a",
        "SELECT (SELECT ID,Name FROM b) FROM a WHERE FALSE",
        "SELECT CASE WHEN FALSE THEN (SELECT ID,Name FROM b) ELSE ID END FROM a",
    ] {
        assert_eq!(
            run(sql).unwrap_err().code,
            "relational.invalid_cardinality",
            "{sql}"
        );
    }
    // More than one row is only a runtime violation; plain EXPLAIN remains schema-only.
    assert!(run("EXPLAIN SELECT (SELECT ID FROM b) FROM a").is_ok());
}
#[test]
fn scalar_and_exists_preserve_group_visibility_and_aggregate_input_scope() {
    for (sql, expected) in [
        (
            "SELECT a.ID,COUNT(*),(SELECT COUNT(*) FROM b WHERE b.ID=a.ID) FROM a GROUP BY a.ID ORDER BY a.ID",
            vec![vec![1, 1, 2], vec![2, 1, 0], vec![3, 1, 1]],
        ),
        (
            "SELECT a.ID,COUNT(*) FROM a GROUP BY a.ID HAVING EXISTS (SELECT ID FROM b WHERE b.ID=a.ID) ORDER BY a.ID",
            vec![vec![1, 1], vec![3, 1]],
        ),
        (
            "SELECT COUNT(*),(SELECT COUNT(*) FROM b) FROM a WHERE ID=9",
            vec![vec![0, 3]],
        ),
        (
            "SELECT a.ID,COUNT(*) FROM a GROUP BY a.ID ORDER BY (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) DESC",
            vec![vec![1, 1], vec![3, 1], vec![2, 1]],
        ),
        (
            "SELECT SUM((SELECT COUNT(*) FROM b WHERE b.ID=a.ID)) FROM a",
            vec![vec![3]],
        ),
        (
            "SELECT (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) AS n,COUNT(*) FROM a GROUP BY (SELECT COUNT(*) FROM b WHERE b.ID=a.ID) ORDER BY n",
            vec![vec![0, 1], vec![1, 1], vec![2, 1]],
        ),
    ] {
        assert_eq!(
            run(sql).unwrap().rows,
            expected
                .into_iter()
                .map(|row| row.into_iter().map(Scalar::Int).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            "{sql}"
        );
    }
    for sql in [
        "SELECT COUNT(*),(SELECT a.Name FROM b WHERE ID=3) FROM a",
        "SELECT a.ID,(SELECT a.Name FROM b WHERE ID=3) FROM a GROUP BY a.ID",
        "SELECT COUNT(*) FROM a HAVING EXISTS (SELECT ID FROM b WHERE b.Name=a.Name)",
    ] {
        assert_eq!(
            run(sql).unwrap_err().code,
            "relational.invalid_binding",
            "{sql}"
        );
    }
}
#[test]
fn exists_and_scalar_finish_provider_before_returning_boolean_or_cardinality() {
    for sql in [
        "SELECT EXISTS (SELECT ID FROM b) FROM a WHERE ID=1",
        "SELECT (SELECT ID FROM b) FROM a WHERE ID=1",
    ] {
        let (_, a, mut b) = providers();
        b.fail_after = Some(2);
        assert_eq!(
            lazy(sql, &a, &b, Limits::default(), None, None)
                .unwrap_err()
                .code,
            "fixture.rhs_error"
        );
        assert_eq!(b.scans(), 1);
    }
}
#[test]
fn scalar_and_exists_bind_unreachable_nested_sources_without_scan() {
    for sql in [
        "SELECT CASE WHEN FALSE THEN EXISTS (SELECT Missing FROM b) ELSE TRUE END FROM a",
        "SELECT (SELECT Missing FROM b) FROM a WHERE FALSE",
    ] {
        let (trace, a, b) = providers();
        assert_eq!(
            lazy(sql, &a, &b, Limits::default(), None, None)
                .unwrap_err()
                .code,
            "relational.invalid_binding"
        );
        assert!(trace.borrow().is_empty());
    }
}
#[test]
fn scalar_exists_dependencies_parameters_and_explain_nodes_are_complete() {
    let sql = "EXPLAIN SELECT (SELECT :scalar FROM b WHERE b.ID=a.ID),EXISTS (SELECT ID,Name FROM b WHERE ID=:exists) FROM a";
    let program = Program::compile(sql).unwrap();
    assert_eq!(
        program.parameters,
        BTreeSet::from(["exists".into(), "scalar".into()])
    );
    assert_eq!(program.source_tables(), vec!["a", "b"]);
    let (trace, a, b) = providers();
    let result = lazy(
        "EXPLAIN SELECT (SELECT Name FROM b WHERE ID=3),EXISTS (SELECT ID,Name FROM b) FROM a",
        &a,
        &b,
        Limits::default(),
        None,
        None,
    )
    .unwrap();
    let details = result
        .plan
        .unwrap()
        .steps
        .into_iter()
        .filter(|s| s.operation == "expression-subquery")
        .map(|s| s.detail)
        .collect::<Vec<_>>();
    assert_eq!(details, vec!["subquery", "exists"]);
    assert!(trace.borrow().is_empty());
}
#[test]
fn exists_respects_nested_cancel_and_independent_work_limit() {
    let (_, a, mut b) = providers();
    let cancel = Arc::new(AtomicBool::new(false));
    b.cancel = Some(cancel.clone());
    assert_eq!(
        lazy(
            "SELECT EXISTS (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits::default(),
            Some(&cancel),
            None
        )
        .unwrap_err()
        .code,
        "operation.cancelled"
    );
    assert_eq!(b.scans(), 1);
    let (_, a, b) = providers();
    assert_eq!(
        lazy(
            "SELECT EXISTS (SELECT ID FROM b) FROM a",
            &a,
            &b,
            Limits {
                work: 1,
                ..Limits::default()
            },
            None,
            None
        )
        .unwrap_err()
        .code,
        "relational.resource_limit"
    );
}
#[test]
fn analyze_correlated_queries_measure_actual_complete_callback_counts() {
    let (_, a, b) = providers();
    let result=lazy("EXPLAIN ANALYZE SELECT a.ID,EXISTS (SELECT ID FROM b WHERE b.ID=a.ID),(SELECT COUNT(*) FROM b WHERE b.ID=a.ID) FROM a ORDER BY a.ID",&a,&b,Limits::default(),None,None).unwrap();
    assert!(result.columns.is_empty() && result.rows.is_empty());
    let measured = result.plan.unwrap().measured.unwrap();
    assert_eq!(measured.output_rows, 3);
    assert_eq!(
        measured
            .scans
            .iter()
            .map(|s| (s.table.name.as_str(), s.calls, s.rows, s.lookups))
            .collect::<Vec<_>>(),
        vec![("a", 1, 3, 0), ("b", 6, 18, 0)]
    );
    assert!(measured.work_units > 0 && measured.charged_bytes > 0);
    assert_eq!(a.scans(), 1);
    assert_eq!(b.scans(), 6);
}
