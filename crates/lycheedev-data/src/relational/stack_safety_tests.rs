//! Ordinary-thread regression coverage for the original Go depth/node limits.
//! No test creates a larger stack, changes the product budget, or treats a
//! subprocess stack abort as a successful resource-limit result.
use super::*;
use std::{cell::Cell, io::Write};
thread_local! {
    static ENABLED: Cell<bool> = const {Cell::new(false)};
    static ACTIVE: Cell<usize> = const {Cell::new(0)};
    static PEAK: Cell<usize> = const {Cell::new(0)};
    static LIVE_FUTURE_BYTES: Cell<usize> = const {Cell::new(0)};
    static PEAK_FUTURE_BYTES: Cell<usize> = const {Cell::new(0)};
    static POLL_ACTIVE: Cell<usize> = const {Cell::new(0)};
    static POLL_PEAK: Cell<usize> = const {Cell::new(0)};
    static POLL_BASE: Cell<usize> = const {Cell::new(0)};
    static POLL_SPAN: Cell<usize> = const {Cell::new(0)};
}
pub(super) struct QueryEntry {
    enabled: bool,
    heap_bytes: usize,
}
pub(super) fn enter_query(_marker: &u8, heap_bytes: usize) -> QueryEntry {
    let enabled = ENABLED.get();
    if enabled {
        let active = ACTIVE.get() + 1;
        // This counts owned query states, including suspended heap futures.
        // Their marker addresses are not evidence of native stack frames.
        ACTIVE.set(active);
        PEAK.set(PEAK.get().max(active));
        let live_bytes = LIVE_FUTURE_BYTES.get() + heap_bytes;
        LIVE_FUTURE_BYTES.set(live_bytes);
        PEAK_FUTURE_BYTES.set(PEAK_FUTURE_BYTES.get().max(live_bytes));
    }
    QueryEntry {
        enabled,
        heap_bytes,
    }
}
impl Drop for QueryEntry {
    fn drop(&mut self) {
        if self.enabled {
            ACTIVE.set(ACTIVE.get() - 1);
            LIVE_FUTURE_BYTES.set(LIVE_FUTURE_BYTES.get() - self.heap_bytes);
        }
    }
}
pub(super) struct PollEntry(bool);
pub(super) fn enter_poll(marker: &u8) -> PollEntry {
    let enabled = ENABLED.get();
    if enabled {
        let active = POLL_ACTIVE.get() + 1;
        // The caller owns this marker in its synchronous poll helper. Only
        // simultaneous native poll calls contribute to the observed span.
        let address = std::ptr::from_ref(marker) as usize;
        if active == 1 {
            POLL_BASE.set(address);
        }
        POLL_SPAN.set(POLL_SPAN.get().max(address.abs_diff(POLL_BASE.get())));
        POLL_ACTIVE.set(active);
        POLL_PEAK.set(POLL_PEAK.get().max(active));
    }
    PollEntry(enabled)
}
impl Drop for PollEntry {
    fn drop(&mut self) {
        if self.0 {
            POLL_ACTIVE.set(POLL_ACTIVE.get() - 1);
        }
    }
}
struct Trace;
impl Trace {
    fn new(label: &str) -> Self {
        assert_eq!(ACTIVE.get(), 0);
        PEAK.set(0);
        assert_eq!(LIVE_FUTURE_BYTES.get(), 0);
        PEAK_FUTURE_BYTES.set(0);
        assert_eq!(POLL_ACTIVE.get(), 0);
        POLL_PEAK.set(0);
        POLL_SPAN.set(0);
        ENABLED.set(true);
        marker(&format!(
            "{label}: begin; admitted future bytes measured at runtime; synchronous poll markers alone observe native nesting"
        ));
        Self
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        marker(&format!(
            "completed: peak live query states={}; remaining query states={}; peak live future bytes={}; remaining future bytes={}; peak native poll nesting={}; native poll marker span bytes={}; remaining polls={}",
            PEAK.get(),
            ACTIVE.get(),
            PEAK_FUTURE_BYTES.get(),
            LIVE_FUTURE_BYTES.get(),
            POLL_PEAK.get(),
            POLL_SPAN.get(),
            POLL_ACTIVE.get()
        ));
        ENABLED.set(false);
        assert_eq!(ACTIVE.get(), 0);
        assert_eq!(LIVE_FUTURE_BYTES.get(), 0);
        assert!(POLL_PEAK.get() <= 1, "driver poll must not recurse");
        assert_eq!(POLL_ACTIVE.get(), 0);
    }
}
fn marker(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "SQL heap-stack {message}");
}
fn sources() -> BTreeMap<String, Source> {
    BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["ID".into()],
            rows: vec![vec![Scalar::Int(1)]],
            complete: true,
        },
    )])
}
fn scalar_chain() -> String {
    let mut query = "SELECT root.ID AS ID FROM t leaf".to_owned();
    for index in 0..62 {
        query = format!("SELECT ({query}) AS ID FROM t s{index}");
    }
    format!("SELECT ({query}) AS ID FROM t root")
}
fn mixed_chain(base: &str, extra_ctes: usize) -> String {
    let mut query = base.to_owned();
    // Each cycle adds 1+2+2+3+3=11 logical levels. Eleven cycles,
    // three final CTEs, and the outer correlated scalar reach exactly 128.
    for index in 0..11 {
        query = format!("WITH c{index} AS ({query}) SELECT ID FROM c{index}");
        query = format!("SELECT ID FROM ({query}) d{index}");
        query = format!("SELECT ({query}) AS ID FROM t s{index}");
        query = format!("SELECT CASE WHEN TRUE THEN ({query}) ELSE 0 END AS ID FROM t k{index}");
        query = format!(
            "SELECT l{index}.ID AS ID FROM t l{index} JOIN t r{index} ON r{index}.ID=({query})"
        );
    }
    for index in 0..extra_ctes {
        query = format!("WITH z{index} AS ({query}) SELECT ID FROM z{index}");
    }
    format!("SELECT ({query}) AS ID FROM t root")
}
#[test]
fn original_scalar_63_default_stack_executes_correlations_clones_and_drops() {
    let _trace = Trace::new("scalar63");
    let sql = scalar_chain();
    marker("scalar63: parse/lower");
    let program = Program::compile(&sql).unwrap();
    let copied = program.clone();
    assert!(
        program == copied,
        "iterative query copy must preserve structure"
    );
    assert_eq!(program.source_tables(), ["t"]);
    marker("scalar63: bind/execute");
    let result = program
        .execute(&sources(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert!(result.complete);
    assert_eq!(PEAK.get(), 64);
    marker("scalar63: explicit drop");
    drop(copied);
    drop(program);
    assert_eq!(
        Program::compile(&format!("SELECT ({sql}) FROM t"))
            .err()
            .expect("expected failure without formatting a deep success tree")
            .code,
        "relational.resource_limit"
    );
}
#[test]
fn scalar_63_with_union_wrappers_exercises_the_largest_native_callback_chain() {
    let _trace = Trace::new("scalar63-union127entries");
    let mut sql = "SELECT root.ID AS ID FROM t leaf".to_owned();
    for index in 0..63 {
        let alias = if index == 62 {
            "root".to_owned()
        } else {
            format!("s{index}")
        };
        sql = format!("SELECT ({sql}) AS ID FROM t {alias} UNION ALL SELECT ID FROM t WHERE FALSE");
    }
    marker("scalar63-union: parse/lower");
    let program = Program::compile(&sql).unwrap();
    marker("scalar63-union: bind/execute");
    let result = program
        .execute(&sources(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert_eq!(
        PEAK.get(),
        127,
        "each UNION wrapper and first branch stays active around the nested scalar"
    );
    drop(program);
    assert_eq!(
        Program::compile(&format!("SELECT ({sql}) FROM t"))
            .err()
            .expect("64 scalar layers must reject")
            .code,
        "relational.resource_limit"
    );
}
#[test]
fn mixed_cte_derived_case_join_scalar_default_stack_preserves_128_129() {
    let _trace = Trace::new("mixed128");
    let sql = mixed_chain("SELECT root.ID AS ID FROM t leaf", 3);
    marker("mixed128: parse/lower");
    let program = Program::compile(&sql).unwrap();
    marker("mixed128: bind/execute");
    let result = program
        .execute(&sources(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert!(result.complete);
    assert!(
        PEAK.get() >= 60,
        "every nested query remains owned while its child is suspended"
    );
    marker("mixed128: explain");
    let explanation = Program::compile(&format!("EXPLAIN {sql}"))
        .unwrap()
        .execute(&sources(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert!(explanation.plan.unwrap().steps.len() > 100);
    drop(program);
    let error = Program::compile(&mixed_chain("SELECT root.ID AS ID FROM t leaf", 4))
        .err()
        .expect("expected failure without formatting a deep success tree");
    assert_eq!(error.code, "relational.resource_limit");
}
#[test]
fn maximum_cte_query_entries_and_derived_63_use_the_default_stack() {
    for derived in [false, true] {
        let _trace = Trace::new(if derived { "derived63" } else { "cte126" });
        let mut sql = "SELECT ID FROM t".to_owned();
        let levels = if derived { 63 } else { 126 };
        for index in 0..levels {
            sql = if derived {
                format!("SELECT ID FROM ({sql}) d{index}")
            } else {
                format!("WITH c{index} AS ({sql}) SELECT ID FROM c{index}")
            };
        }
        let program = Program::compile(&sql).unwrap();
        let result = program
            .execute(&sources(), &BTreeMap::new(), Limits::default(), None)
            .unwrap();
        assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
        assert_eq!(PEAK.get(), levels + 1);
        drop(program);
        let rejected = if derived {
            format!("SELECT ID FROM ({sql}) rejected")
        } else {
            format!("WITH rejected AS ({sql}) SELECT ID FROM rejected")
        };
        assert_eq!(
            Program::compile(&rejected)
                .err()
                .expect("expected failure without formatting a deep success tree")
                .code,
            "relational.resource_limit"
        );
    }
}
#[test]
fn query_lowering_result_slots_keep_sibling_and_nested_parameter_scopes() {
    let program=Program::compile("SELECT (SELECT :lhs FROM t) IN (WITH c AS (SELECT :cte AS ID FROM t) SELECT :rhs FROM c UNION ALL SELECT :tail FROM t LIMIT :take),(SELECT :sibling FROM t) FROM (SELECT :derived AS ID FROM t) d LIMIT :outer").unwrap();
    assert_eq!(
        program
            .parameters
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "cte", "derived", "lhs", "outer", "rhs", "sibling", "tail", "take"
        ]
    );
    let Some(Term::InSubquery(input, right, _)) = &program.projection[0].expr else {
        panic!("IN shape");
    };
    let Term::ScalarSubquery(left) = &**input else {
        panic!("left scalar");
    };
    assert_eq!(
        left.parameters
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["lhs"]
    );
    assert_eq!(
        right
            .parameters
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["cte", "rhs", "tail", "take"]
    );
    assert_eq!(
        program
            .derived
            .as_ref()
            .unwrap()
            .parameters
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["derived"]
    );
}
#[test]
fn near_token_limit_left_chains_reject_and_destroy_without_native_recursion() {
    let _trace = Trace::new("long-left-cleanup");
    let chain = std::iter::repeat_n("1", 16380)
        .collect::<Vec<_>>()
        .join("+");
    let valid = format!("SELECT {chain} FROM t");
    assert!(lexical::tokens(&valid).unwrap().len() < 32768);
    assert_eq!(
        Program::compile(&valid)
            .err()
            .expect("expected failure without formatting a deep success tree")
            .code,
        "relational.resource_limit"
    );
    assert_eq!(
        Program::compile(&format!("SELECT {chain}+ FROM t"))
            .err()
            .expect("expected failure without formatting a deep success tree")
            .code,
        "relational.invalid_syntax"
    );
    let dialect = syntax::QueryDialect::default();
    let mut parser =
        Parser::new(&dialect).with_tokens_with_locations(lexical::tokens(&valid).unwrap());
    let mut query = ast_owner::Owned::new(retrieval::parse(&dialect, &mut parser).unwrap());
    let checks = Cell::new(0);
    let error = Program::compile_query(&mut query, 0, &|| {
        checks.set(checks.get() + 1);
        if checks.get() == 100 {
            Err(fault(
                "fixture.lowering_cancelled",
                7,
                "original typed stop",
            ))
        } else {
            Ok(())
        }
    })
    .err()
    .expect("expected failure without formatting a deep success tree");
    assert_eq!(error.code, "fixture.lowering_cancelled");
    assert_eq!(checks.get(), 100);
    drop(query);
}
#[test]
fn deep_unadmitted_terms_and_last_shared_query_owner_have_iterative_teardown() {
    let _trace = Trace::new("owned-cleanup");
    let mut term = Term::Value(Scalar::Int(1));
    for _ in 0..32768 {
        term = Term::Unary(UnaryOperator::Plus, Box::new(term));
    }
    let copied = term.clone();
    assert!(
        term == copied,
        "iterative scalar copy must preserve structure"
    );
    drop(copied);
    drop(term);
    let mut query = Program::compile("SELECT ID FROM t").unwrap();
    for level in 0..16384 {
        let mut parent = Program::compile("SELECT ID FROM t").unwrap();
        match level % 4 {
            0 => parent.derived = Some(Box::new(query)),
            1 => parent.bindings.push(("c".into(), query)),
            2 => parent.branches.push(query),
            _ => parent.joins.push(joins::Link {
                table: RelationName::internal("t"),
                alias: "d".into(),
                derived: Some(Box::new(query)),
                left: false,
                condition: None,
            }),
        }
        query = parent;
    }
    let copied = query.clone();
    assert!(
        query == copied,
        "iterative owned-query copy must preserve structure"
    );
    drop(copied);
    let first = std::sync::Arc::new(query);
    let second = first.clone();
    drop(Term::Exists(first));
    drop(Term::ScalarSubquery(second));
}
#[test]
fn maximum_flat_join_width_uses_heap_dfs_and_keeps_left_join_match_state() {
    let _trace = Trace::new("flat4095");
    let mut sql = "SELECT a0.ID FROM t a0".to_owned();
    for index in 1..4096 {
        sql.push_str(&format!(" CROSS JOIN t a{index}"));
    }
    let program = Program::compile(&sql).unwrap();
    let result = program
        .execute(&sources(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    let mut data = sources();
    data.insert(
        "empty".into(),
        Source {
            columns: vec!["ID".into()],
            rows: vec![],
            complete: true,
        },
    );
    // An ON success stays matched even when a later CROSS JOIN emits no row.
    let result =
        Program::compile("SELECT a.ID,b.ID FROM t a LEFT JOIN t b ON TRUE CROSS JOIN empty e")
            .unwrap()
            .execute(&data, &BTreeMap::new(), Limits::default(), None)
            .unwrap();
    assert!(result.rows.is_empty());
    let result = Program::compile(
        "SELECT a.ID,b.ID,c.ID FROM t a LEFT JOIN empty b ON TRUE LEFT JOIN t c ON b.ID=c.ID",
    )
    .unwrap()
    .execute(&data, &BTreeMap::new(), Limits::default(), None)
    .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Scalar::Int(1), Scalar::Null, Scalar::Null]]
    );
}
struct LateProvider<'a> {
    calls: Cell<usize>,
    cancel: Option<&'a AtomicBool>,
    cancel_on: usize,
    live_cursors: Cell<usize>,
    live_rows: Cell<usize>,
    peak_rows: Cell<usize>,
    drops: Cell<usize>,
}
struct LateCursor<'a, 'b> {
    provider: &'a LateProvider<'b>,
    yielded: bool,
    current: bool,
    row: [Scalar; 1],
}
impl RowCursor for LateCursor<'_, '_> {
    fn advance(&mut self) -> Result<bool> {
        if !self.yielded {
            self.yielded = true;
            self.current = true;
            let live = self.provider.live_rows.get() + 1;
            self.provider.live_rows.set(live);
            self.provider
                .peak_rows
                .set(self.provider.peak_rows.get().max(live));
            return Ok(true);
        }
        if self.current {
            self.provider
                .live_rows
                .set(self.provider.live_rows.get() - 1);
            self.current = false;
        }
        self.row.fill(Scalar::Null);
        Err(fault(
            "fixture.late_scan",
            5,
            "provider failed after its row",
        ))
    }
    fn row(&self) -> &[Scalar] {
        if self.yielded { &self.row } else { &[] }
    }
}
impl Drop for LateCursor<'_, '_> {
    fn drop(&mut self) {
        self.provider
            .live_cursors
            .set(self.provider.live_cursors.get() - 1);
        self.provider.drops.set(self.provider.drops.get() + 1);
        if self.current {
            self.provider
                .live_rows
                .set(self.provider.live_rows.get() - 1);
        }
    }
}
impl RowProvider for LateProvider<'_> {
    fn cursor(&self, _: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if call == self.cancel_on
            && let Some(cancel) = self.cancel
        {
            cancel.store(true, AtomicOrdering::Release);
        }
        self.live_cursors.set(self.live_cursors.get() + 1);
        Ok(Box::new(LateCursor {
            provider: self,
            yielded: false,
            current: false,
            row: [Scalar::Int(1)],
        }))
    }
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        let mut cursor = self.cursor(None)?;
        while cursor.advance()? {
            emit(cursor.row())?;
        }
        Ok(())
    }
}

#[test]
fn deepest_provider_late_error_and_cancellation_release_all_active_query_frames() {
    for cancelled in [false, true] {
        let _trace = Trace::new(if cancelled {
            "scalar63-cancel"
        } else {
            "scalar63-late-error"
        });
        let flag = AtomicBool::new(false);
        let provider = LateProvider {
            calls: Cell::new(0),
            cancel: cancelled.then_some(&flag),
            cancel_on: 64,
            live_cursors: Cell::new(0),
            live_rows: Cell::new(0),
            peak_rows: Cell::new(0),
            drops: Cell::new(0),
        };
        let schema = Source {
            columns: vec!["ID".into()],
            rows: vec![],
            complete: true,
        };
        let inputs = BTreeMap::from([("t".into(), &schema)]);
        let prepared = [PreparedSource {
            schema: &schema,
            provider: &provider,
        }];
        let program = Program::compile(&scalar_chain()).unwrap();
        let error = program
            .execute_providers_budgeted(
                &inputs,
                &prepared,
                &BTreeMap::new(),
                Limits::default(),
                Some(&flag),
                None,
            )
            .err()
            .expect("expected failure without formatting a deep success tree");
        assert_eq!(
            error.code,
            if cancelled {
                "operation.cancelled"
            } else {
                "fixture.late_scan"
            }
        );
        assert_eq!(provider.calls.get(), 64);
        assert_eq!(provider.peak_rows.get(), 64);
        assert_eq!(provider.drops.get(), 64);
        assert_eq!(provider.live_cursors.get(), 0);
        assert_eq!(provider.live_rows.get(), 0);
        assert_eq!(PEAK.get(), 64);
        assert_eq!(ACTIVE.get(), 0);
        drop(program);
    }
}

#[test]
fn mixed_128_provider_failure_and_cancel_unwind_the_full_owned_chain() {
    for cancelled in [false, true] {
        let _trace = Trace::new(if cancelled {
            "mixed128-cancel"
        } else {
            "mixed128-late-error"
        });
        let flag = AtomicBool::new(false);
        let provider = LateProvider {
            calls: Cell::new(0),
            cancel: cancelled.then_some(&flag),
            cancel_on: 1,
            live_cursors: Cell::new(0),
            live_rows: Cell::new(0),
            peak_rows: Cell::new(0),
            drops: Cell::new(0),
        };
        let physical = Source {
            columns: vec!["ID".into()],
            rows: vec![vec![Scalar::Int(1)]],
            complete: true,
        };
        let failing = Source {
            columns: vec!["ID".into()],
            rows: vec![],
            complete: true,
        };
        let inputs = BTreeMap::from([("t".into(), &physical), ("bad".into(), &failing)]);
        let prepared = [PreparedSource {
            schema: &failing,
            provider: &provider,
        }];
        let program =
            Program::compile(&mixed_chain("SELECT root.ID AS ID FROM bad leaf", 3)).unwrap();
        let error = program
            .execute_providers_budgeted(
                &inputs,
                &prepared,
                &BTreeMap::new(),
                Limits::default(),
                Some(&flag),
                None,
            )
            .err()
            .expect("expected failure without formatting a deep success tree");
        assert_eq!(
            error.code,
            if cancelled {
                "operation.cancelled"
            } else {
                "fixture.late_scan"
            }
        );
        assert_eq!(provider.calls.get(), 1);
        assert_eq!(provider.peak_rows.get(), 1);
        assert_eq!(provider.drops.get(), 1);
        assert_eq!(provider.live_cursors.get(), 0);
        assert_eq!(provider.live_rows.get(), 0);
        assert!(PEAK.get() >= 60);
        assert_eq!(ACTIVE.get(), 0);
        drop(program);
    }
}

#[test]
fn last_strong_query_owner_with_live_weak_handles_drops_iteratively() {
    let _trace = Trace::new("weak-query-cleanup");
    let leaf = Program::compile("SELECT ID FROM t").unwrap();
    let mut query = std::sync::Arc::new(leaf.clone());
    let mut weak = Vec::new();
    for _ in 0..16384 {
        weak.push(std::sync::Arc::downgrade(&query));
        let mut parent = leaf.clone();
        parent.projection[0].expr = Some(Term::ScalarSubquery(query));
        query = std::sync::Arc::new(parent);
    }
    weak.push(std::sync::Arc::downgrade(&query));
    drop(query);
    assert!(weak.iter().all(|query| query.upgrade().is_none()));
}

#[test]
fn concurrent_final_query_owners_with_weak_handles_release_without_recursive_drop() {
    let leaf = Program::compile("SELECT ID FROM t").unwrap();
    let mut query = std::sync::Arc::new(leaf.clone());
    let mut weak = Vec::new();
    let mut other_owners = Vec::new();
    for _ in 0..8192 {
        weak.push(std::sync::Arc::downgrade(&query));
        other_owners.push(query.clone());
        let mut parent = leaf.clone();
        parent.projection[0].expr = Some(Term::ScalarSubquery(query));
        query = std::sync::Arc::new(parent);
    }
    let barrier = std::sync::Barrier::new(2);
    // Both threads keep their ordinary default stack. Only test-owned query
    // objects cross this boundary; production execution stays same-thread.
    std::thread::scope(|scope| {
        let barrier = &barrier;
        let peer = scope.spawn(move || {
            barrier.wait();
            drop(other_owners);
        });
        barrier.wait();
        drop(query);
        peer.join().unwrap();
    });
    assert!(weak.iter().all(|query| query.upgrade().is_none()));
}
