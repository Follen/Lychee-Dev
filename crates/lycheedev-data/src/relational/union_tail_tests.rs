//! Fixed Go 3766f9cacb52e93a803a626187e2a1d54a445468 source-derived regressions.
//! compound.go restores the enclosing executeSelect callback before final
//! executeRows; callback lexical scope, dynamic fields and row resolver differ.
//! These Rust tests are NOT_RUN. rustfmt is syntax/style checking only.
use super::*;
use crate::resource::{Budget, Cost};
use std::{cell::RefCell, rc::Rc, sync::Arc};

type Trace = Rc<RefCell<Vec<String>>>;
struct Provider<'a> {
    name: &'static str,
    source: &'a Source,
    trace: Trace,
    fail: bool,
    cancel: Option<Arc<AtomicBool>>,
}
impl Provider<'_> {
    fn scans(&self) -> usize {
        self.trace
            .borrow()
            .iter()
            .filter(|event| **event == format!("{}.scan", self.name))
            .count()
    }
}
impl RowProvider for Provider<'_> {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        self.trace.borrow_mut().push(format!("{}.scan", self.name));
        if self.fail {
            return Err(fault("fixture.tail_provider", 5, "tail provider failed"));
        }
        let mut scratch = vec![];
        for row in &self.source.rows {
            scratch.clone_from(row);
            if let Some(cancel) = &self.cancel {
                cancel.store(true, AtomicOrdering::Release);
            }
            self.trace.borrow_mut().push(format!("{}.row", self.name));
            emit(&scratch)?;
            scratch.fill(Scalar::Null);
        }
        Ok(())
    }
}
fn source(columns: &[&str], rows: Vec<Vec<Scalar>>) -> Source {
    Source {
        columns: columns.iter().map(|s| (*s).into()).collect(),
        rows,
        complete: true,
    }
}
fn ids(values: &[i64]) -> Vec<Vec<Scalar>> {
    values.iter().map(|n| vec![Scalar::Int(*n)]).collect()
}
fn fixtures() -> BTreeMap<String, Source> {
    BTreeMap::from([
        (
            "a".into(),
            source(
                &["ID", "Name"],
                vec![
                    vec![Scalar::Int(1), Scalar::String("a".into())],
                    vec![Scalar::Int(2), Scalar::String("b".into())],
                    vec![Scalar::Int(3), Scalar::String("c".into())],
                ],
            ),
        ),
        ("b".into(), source(&["ID"], ids(&[1, 3]))),
        ("c".into(), source(&["Other"], ids(&[1]))),
    ])
}
fn owned(sql: &str, sources: &BTreeMap<String, Source>) -> Result<QueryResult> {
    Program::compile(sql)?.execute(sources, &BTreeMap::new(), Limits::default(), None)
}
fn run(sql: &str) -> Result<QueryResult> {
    owned(sql, &fixtures())
}
fn provider<'a>(
    name: &'static str,
    sources: &'a BTreeMap<String, Source>,
    trace: &Trace,
) -> Provider<'a> {
    Provider {
        name,
        source: &sources[name],
        trace: trace.clone(),
        fail: false,
        cancel: None,
    }
}
fn lazy(
    sql: &str,
    sources: &BTreeMap<String, Source>,
    providers: &[&Provider<'_>],
    limits: Limits,
    cancel: Option<&AtomicBool>,
    shared: Option<&Budget>,
) -> Result<QueryResult> {
    let inputs = sources
        .iter()
        .map(|(name, source)| (name.clone(), source))
        .collect();
    let registry: Vec<_> = providers
        .iter()
        .map(|p| PreparedSource {
            schema: p.source,
            provider: *p as &dyn RowProvider,
        })
        .collect();
    Program::compile(sql)?.execute_providers_budgeted(
        &inputs,
        &registry,
        &BTreeMap::new(),
        limits,
        cancel,
        shared,
    )
}
const LEXICAL: &str = "WITH allowed AS (SELECT ID FROM a WHERE ID=1) SELECT s.ID FROM a s WHERE s.ID IN (WITH allowed AS (SELECT ID FROM a WHERE ID=2) SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT ID FROM allowed) DESC LIMIT 1)";
const CORRELATED: &str = "SELECT s.ID FROM a s WHERE s.ID IN (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT ID FROM a x WHERE x.ID=s.ID))";
const TAIL_PROVIDER: &str = "SELECT ID FROM a WHERE ID IN (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT Other FROM c) DESC LIMIT 1)";

#[test]
fn nested_tail_captures_enclosing_cte_not_shadowing_compound_cte() {
    assert_eq!(run(LEXICAL).unwrap().rows, ids(&[1]));
}
#[test]
fn callback_lexical_scope_cannot_see_compound_local_cte() {
    let sql = "SELECT ID IN (WITH hidden AS (SELECT ID FROM b) SELECT ID FROM a UNION ALL SELECT ID FROM a ORDER BY ID IN (SELECT ID FROM hidden)) FROM a";
    let sources = fixtures();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let b = provider("b", &sources, &trace);
    assert_eq!(
        lazy(sql, &sources, &[&a, &b], Limits::default(), None, None)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn restored_callback_does_not_leak_into_sibling_expression_or_execution() {
    let sql = "WITH allowed AS (SELECT ID FROM a WHERE ID=1) SELECT ID IN (WITH allowed AS (SELECT ID FROM a WHERE ID=2) SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT ID FROM allowed) DESC LIMIT 1), ID IN (SELECT ID FROM allowed) FROM a";
    let expected = vec![
        vec![Scalar::Bool(true), Scalar::Bool(true)],
        vec![Scalar::Bool(false), Scalar::Bool(false)],
        vec![Scalar::Bool(false), Scalar::Bool(false)],
    ];
    let program = Program::compile(sql).unwrap();
    for _ in 0..2 {
        assert_eq!(
            program
                .execute(&fixtures(), &BTreeMap::new(), Limits::default(), None)
                .unwrap()
                .rows,
            expected
        );
    }
}
#[test]
fn callback_presence_is_independent_of_outer_row_frame() {
    assert_eq!(run("SELECT ID FROM (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT ID FROM b) DESC LIMIT 1) d").unwrap().rows,ids(&[1]));
    for sql in [
        "SELECT ID FROM a UNION ALL SELECT ID FROM b ORDER BY ID IN (SELECT ID FROM b)",
        "WITH t AS (SELECT ID FROM a UNION ALL SELECT ID FROM b ORDER BY ID IN (SELECT ID FROM b)) SELECT ID FROM t",
    ] {
        assert_eq!(
            run(sql).unwrap_err().code,
            "relational.unsupported",
            "{sql}"
        );
    }
    assert_eq!(
        run("SELECT ID FROM a WHERE ID IN (SELECT ID FROM b UNION ALL SELECT ID FROM b)")
            .unwrap()
            .rows,
        ids(&[1, 3])
    );
}
#[test]
fn restored_fields_bind_during_preflight_but_current_ordinal_resolver_fails_at_runtime() {
    let sources = fixtures();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let explained = lazy(
        &format!("EXPLAIN {CORRELATED}"),
        &sources,
        &[&a],
        Limits::default(),
        None,
        None,
    )
    .unwrap();
    assert!(explained.plan.is_some());
    assert!(trace.borrow().is_empty());
    assert_eq!(
        lazy(CORRELATED, &sources, &[&a], Limits::default(), None, None)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert!(a.scans() > 0);
}
#[test]
fn grandparent_correlation_skips_restored_nearest_fields_and_remains_valid() {
    let sql = "SELECT z.ID FROM a z WHERE z.ID IN (SELECT x.ID FROM a x WHERE x.ID IN (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT y.ID FROM a y WHERE y.ID=z.ID) DESC LIMIT 1))";
    assert_eq!(run(sql).unwrap().rows, ids(&[1, 2]));
}
#[test]
fn empty_alias_callback_resolves_by_name_against_current_compound_row() {
    // The tail's RHS reads the current compound ID, not the enclosing SELECT
    // row. Both tail rows match themselves, so LIMIT keeps ID=1 each time.
    let sql = "SELECT ID FROM (SELECT ID FROM a) WHERE ID IN (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT ID FROM c) DESC LIMIT 1)";
    assert_eq!(run(sql).unwrap().rows, ids(&[1]));
}
#[test]
fn derived_tail_correlation_sees_empty_inherited_fields_at_runtime() {
    let sql = "SELECT ID FROM (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY ID IN (SELECT ID FROM c)) d";
    assert!(run(&format!("EXPLAIN {sql}")).unwrap().plan.is_some());
    assert_eq!(run(sql).unwrap_err().code, "relational.invalid_binding");
}
#[test]
fn group_allowed_checks_canonical_fields_not_reused_ordinals() {
    let sql = "SELECT a.ID,a.ID IN (SELECT ID FROM b UNION ALL SELECT ID FROM b ORDER BY ID IN (SELECT ID FROM c)) FROM a GROUP BY a.ID";
    let sources = fixtures();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let b = provider("b", &sources, &trace);
    let c = provider("c", &sources, &trace);
    assert_eq!(
        lazy(sql, &sources, &[&a, &b, &c], Limits::default(), None, None)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn noncorrelated_tail_preserves_group_keys_and_aggregate_input_visibility() {
    let sql = "SELECT a.ID,a.ID IN (SELECT ID FROM a WHERE ID<3 UNION ALL SELECT ID FROM a WHERE FALSE ORDER BY ID IN (SELECT ID FROM b) DESC LIMIT 1) FROM a GROUP BY a.ID ORDER BY a.ID";
    assert_eq!(
        run(sql).unwrap().rows,
        vec![
            vec![Scalar::Int(1), Scalar::Bool(true)],
            vec![Scalar::Int(2), Scalar::Bool(false)],
            vec![Scalar::Int(3), Scalar::Bool(false)]
        ]
    );
    assert_eq!(run("SELECT COUNT(CASE WHEN ID IN (SELECT ID FROM a WHERE ID<3 UNION ALL SELECT ID FROM a WHERE FALSE ORDER BY ID IN (SELECT ID FROM b) DESC LIMIT 1) THEN ID END) FROM a").unwrap().rows,ids(&[1]));
}
#[test]
fn ordinal_compound_columns_preserve_public_names_duplicates_and_aliases() {
    let result=run("SELECT ID AS chosen,Name FROM a WHERE ID=1 UNION ALL SELECT ID,Name FROM a WHERE ID=2 ORDER BY chosen DESC").unwrap();
    assert_eq!(result.columns, vec!["chosen", "Name"]);
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::Int(2), Scalar::String("b".into())],
            vec![Scalar::Int(1), Scalar::String("a".into())]
        ]
    );
    assert_eq!(
        run("SELECT ID,ID FROM a WHERE ID=1 UNION ALL SELECT ID,ID FROM a WHERE ID=2")
            .unwrap()
            .columns,
        vec!["ID", "ID"]
    );
    assert_eq!(
        run("SELECT ID,ID FROM a UNION ALL SELECT ID,ID FROM a ORDER BY ID")
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
}
#[test]
fn compound_public_names_keep_quoted_dots_without_reconstructing_encoded_fields() {
    let result = run("SELECT ID AS \"a.b\",Name AS \"c.d.e\" FROM a WHERE ID=1 UNION ALL SELECT ID,Name FROM a WHERE ID=2").unwrap();
    assert_eq!(result.columns, vec!["a.b", "c.d.e"]);
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::Int(1), Scalar::String("a".into())],
            vec![Scalar::Int(2), Scalar::String("b".into())]
        ]
    );
}
#[test]
fn join_on_callback_fields_exclude_later_aliases_that_shadow_grandparent() {
    let sql = "SELECT z.ID FROM a z WHERE z.ID IN (SELECT x.ID FROM a x JOIN b y ON x.ID IN (SELECT ID FROM b UNION ALL SELECT ID FROM b ORDER BY ID IN (SELECT Other FROM c WHERE Other=z.ID) DESC LIMIT 1) CROSS JOIN a z)";
    assert_eq!(run(sql).unwrap().rows, ids(&[1]));
}
#[test]
fn callback_scope_keeps_provider_identity_and_reexecutes_tail_rhs_per_compound_row() {
    let sources = fixtures();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let c = provider("c", &sources, &trace);
    assert_eq!(
        lazy(
            TAIL_PROVIDER,
            &sources,
            &[&a, &c],
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .rows,
        ids(&[1])
    );
    assert_eq!(a.scans(), 7); // Outer once; two branches for each of three outer rows.
    assert_eq!(c.scans(), 6); // Complete RHS for each of the two compound rows.
}
#[test]
fn tail_provider_error_and_cancel_propagate_without_fallback_or_partial_result() {
    let sources = fixtures();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let mut c = provider("c", &sources, &trace);
    c.fail = true;
    assert_eq!(
        lazy(
            TAIL_PROVIDER,
            &sources,
            &[&a, &c],
            Limits::default(),
            None,
            None
        )
        .unwrap_err()
        .code,
        "fixture.tail_provider"
    );
    assert_eq!(c.scans(), 1);
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let mut c = provider("c", &sources, &trace);
    let cancel = Arc::new(AtomicBool::new(false));
    c.cancel = Some(cancel.clone());
    assert_eq!(
        lazy(
            TAIL_PROVIDER,
            &sources,
            &[&a, &c],
            Limits::default(),
            Some(&cancel),
            None
        )
        .unwrap_err()
        .code,
        "operation.cancelled"
    );
    assert_eq!(c.scans(), 1);
}
#[test]
fn preflight_and_tail_share_work_deadline_and_safety_control() {
    let sources = fixtures();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let c = provider("c", &sources, &trace);
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
            deadline: Instant::now() - Duration::from_secs(1),
            ..Limits::default()
        },
    ] {
        assert_eq!(
            lazy(TAIL_PROVIDER, &sources, &[&a, &c], limits, None, None)
                .unwrap_err()
                .code,
            "relational.resource_limit"
        );
        assert!(trace.borrow().is_empty());
    }
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
            TAIL_PROVIDER,
            &sources,
            &[&a, &c],
            Limits::default(),
            None,
            Some(&shared)
        )
        .unwrap_err()
        .code,
        "records.query_resource_budget"
    );
    assert!(trace.borrow().is_empty());
}
#[test]
fn owned_preflight_completeness_survives_skipped_rhs_empty_outer_and_explain() {
    let mut sources = fixtures();
    sources.get_mut("b").unwrap().complete = false;
    for sql in [
        "SELECT CASE WHEN TRUE THEN ID ELSE ID IN (SELECT ID FROM b) END FROM a",
        "SELECT ID IN (SELECT ID FROM b) FROM a WHERE FALSE",
        "EXPLAIN SELECT ID IN (SELECT ID FROM b) FROM a",
        "SELECT ID IN (SELECT ID FROM b) FROM a",
    ] {
        assert!(!owned(sql, &sources).unwrap().complete, "{sql}");
    }
    sources.get_mut("a").unwrap().rows.clear();
    assert!(
        !owned("SELECT ID IN (SELECT ID FROM b) FROM a", &sources)
            .unwrap()
            .complete
    );
}
#[test]
fn provider_preflight_completeness_survives_skipped_rhs_empty_outer_and_explain() {
    let mut sources = fixtures();
    sources.get_mut("b").unwrap().complete = false;
    for (sql, scans) in [
        (
            "SELECT CASE WHEN TRUE THEN ID ELSE ID IN (SELECT ID FROM b) END FROM a",
            0,
        ),
        ("SELECT ID IN (SELECT ID FROM b) FROM a WHERE FALSE", 0),
        ("EXPLAIN SELECT ID IN (SELECT ID FROM b) FROM a", 0),
        ("SELECT ID IN (SELECT ID FROM b) FROM a", 3),
    ] {
        let trace = Trace::default();
        let a = provider("a", &sources, &trace);
        let b = provider("b", &sources, &trace);
        assert!(
            !lazy(sql, &sources, &[&a, &b], Limits::default(), None, None)
                .unwrap()
                .complete,
            "{sql}"
        );
        assert_eq!(b.scans(), scans);
    }
    sources.get_mut("a").unwrap().rows.clear();
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let b = provider("b", &sources, &trace);
    assert!(
        !lazy(
            "SELECT ID IN (SELECT ID FROM b) FROM a",
            &sources,
            &[&a, &b],
            Limits::default(),
            None,
            None
        )
        .unwrap()
        .complete
    );
    assert_eq!(b.scans(), 0);
}
#[test]
fn skipped_nested_tail_rhs_still_propagates_preflight_completeness() {
    let mut sources = fixtures();
    sources.get_mut("c").unwrap().complete = false;
    let sql = "SELECT ID IN (SELECT ID FROM a WHERE ID=1 UNION ALL SELECT ID FROM a WHERE ID=2 ORDER BY CASE WHEN TRUE THEN TRUE ELSE ID IN (SELECT Other FROM c) END) FROM a";
    let trace = Trace::default();
    let a = provider("a", &sources, &trace);
    let c = provider("c", &sources, &trace);
    assert!(
        !lazy(sql, &sources, &[&a, &c], Limits::default(), None, None)
            .unwrap()
            .complete
    );
    assert_eq!(c.scans(), 0);
}
#[test]
fn inherited_callback_schema_error_precedes_all_rows_and_runtime_provider_error() {
    let sources = fixtures();
    let trace = Trace::default();
    let mut a = provider("a", &sources, &trace);
    a.fail = true;
    let sql = "SELECT ID IN (SELECT ID FROM a UNION ALL SELECT ID FROM a ORDER BY ID IN (SELECT ID,Name FROM a)) FROM a";
    assert_eq!(
        lazy(sql, &sources, &[&a], Limits::default(), None, None)
            .unwrap_err()
            .code,
        "relational.invalid_cardinality"
    );
    assert!(trace.borrow().is_empty());
}
