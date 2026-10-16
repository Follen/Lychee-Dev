//! Arithmetic counts below are derived from fixed Go 323 execute.go, rows.go,
//! group.go, results.go and topk.go. They are asserted at the real Program
//! consumer, not a second implementation used in place of execution.
use super::*;
fn source(column: &str, values: Vec<Scalar>) -> Source {
    Source {
        columns: vec![column.into()],
        rows: values.into_iter().map(|value| vec![value]).collect(),
        complete: true,
    }
}
fn run(sql: &str, source: Source, work: u64, bytes: usize) -> Result<QueryResult> {
    Program::compile(sql)?.execute(
        &BTreeMap::from([("t".into(), source)]),
        &BTreeMap::new(),
        Limits {
            work,
            memory_bytes: bytes,
            ..Limits::default()
        },
        None,
    )
}
#[test]
fn fixed_go_work_and_memory_boundaries_drive_real_program() {
    let cases = [
        ("SELECT ID FROM t", vec![Scalar::Int(1)], 10, 354),
        (
            "SELECT ID FROM t ORDER BY ID",
            vec![Scalar::Int(2), Scalar::Int(1)],
            21,
            610,
        ),
        (
            "SELECT ID FROM t LIMIT 1",
            vec![Scalar::Int(2), Scalar::Int(1)],
            17,
            418,
        ),
        (
            "SELECT COUNT(*) FROM t",
            vec![Scalar::Int(2), Scalar::Int(1)],
            19,
            994,
        ),
        (
            "SELECT DISTINCT ID FROM t",
            vec![Scalar::Int(1), Scalar::Float(1.0)],
            20,
            614,
        ),
    ];
    for (sql, values, work, bytes) in cases {
        assert!(
            run(sql, source("ID", values.clone()), work, bytes).is_ok(),
            "{sql}: exact boundary"
        );
        assert_eq!(
            run(sql, source("ID", values.clone()), work - 1, bytes)
                .unwrap_err()
                .code,
            "relational.resource_limit",
            "{sql}: one work unit short"
        );
        assert_eq!(
            run(sql, source("ID", values), work, bytes - 1)
                .unwrap_err()
                .code,
            "relational.resource_limit",
            "{sql}: one retained byte short"
        );
    }
}
#[test]
fn analyze_reports_the_same_real_work_and_logical_bytes() {
    let result = run(
        "EXPLAIN ANALYZE SELECT ID FROM t",
        source("ID", vec![Scalar::Int(1)]),
        13,
        494,
    )
    .unwrap();
    let measurement = result.plan.unwrap().measured.unwrap();
    assert_eq!(
        (
            measurement.work_units,
            measurement.charged_bytes,
            measurement.output_rows
        ),
        (13, 494, 1)
    );
    assert_eq!(
        (measurement.scans[0].calls, measurement.scans[0].rows),
        (1, 1)
    );
}
#[test]
fn min_retains_only_the_current_extreme_string_delta() {
    // Schema 162; one state 640; current MIN 'a' 1; output 97; finish 96.
    let values = vec![Scalar::String("bbb".into()), Scalar::String("a".into())];
    let result = run(
        "SELECT MIN(ID) FROM t",
        source("ID", values.clone()),
        24,
        996,
    )
    .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::String("a".into())]]);
    assert_eq!(
        run("SELECT MIN(ID) FROM t", source("ID", values), 24, 995)
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
}
#[test]
fn max_charges_growth_and_never_refunds_distinct_keys_or_work() {
    let values = vec![Scalar::String("a".into()), Scalar::String("zzz".into())];
    // Schema 162; state 640; current extreme 3; output 99; finish 96.
    assert_eq!(
        run(
            "SELECT MAX(ID) FROM t",
            source("ID", values.clone()),
            24,
            1000
        )
        .unwrap()
        .rows,
        vec![vec![Scalar::String("zzz".into())]]
    );
    assert_eq!(
        run("SELECT MAX(ID) FROM t", source("ID", values), 23, 1000)
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
}
#[test]
fn distinct_cannot_hide_a_later_duplicate_order_expression_error() {
    let inputs = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["ID".into(), "Text".into()],
            rows: vec![
                vec![Scalar::Int(1), Scalar::String("1".into())],
                vec![Scalar::Int(1), Scalar::String("bad".into())],
            ],
            complete: true,
        },
    )]);
    let error = Program::compile("SELECT DISTINCT ID FROM t ORDER BY CAST(Text AS INT)")
        .unwrap()
        .execute(&inputs, &BTreeMap::new(), Limits::default(), None)
        .unwrap_err();
    assert_eq!(error.code, "relational.type_mismatch");
}
#[test]
fn groups_finish_having_projection_and_order_before_the_next_group() {
    let inputs = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["K".into(), "V".into()],
            rows: vec![
                vec![Scalar::String("bad".into()), Scalar::Int(1)],
                vec![Scalar::String("2".into()), Scalar::UInt(u64::MAX)],
                vec![Scalar::String("2".into()), Scalar::Int(1)],
            ],
            complete: true,
        },
    )]);
    let error = Program::compile("SELECT CAST(K AS INT), SUM(V) FROM t GROUP BY K")
        .unwrap()
        .execute(&inputs, &BTreeMap::new(), Limits::default(), None)
        .unwrap_err();
    assert_eq!(
        error.code, "relational.type_mismatch",
        "first group's projection precedes later SUM overflow"
    );
}
#[test]
fn bare_and_explicit_static_map_entries_share_one_resolved_identity() {
    let inputs = BTreeMap::from([
        ("t".into(), source("ID", vec![Scalar::Int(1)])),
        ("static.t".into(), source("ID", vec![Scalar::Int(2)])),
        (
            "unused".into(),
            Source {
                columns: vec![],
                rows: vec![],
                complete: true,
            },
        ),
    ]);
    let result = Program::compile("SELECT ID FROM t UNION ALL SELECT ID FROM static.t")
        .unwrap()
        .execute(&inputs, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![Scalar::Int(1)], vec![Scalar::Int(1)]]
    );
}
#[test]
fn generic_text_values_do_not_inherit_the_key_function_one_mib_limit() {
    let text = "x".repeat((1 << 20) + 1);
    let result = run(
        "SELECT ID FROM t",
        source("ID", vec![Scalar::String(text.clone())]),
        10,
        2 << 20,
    )
    .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::String(text)]]);
}
#[test]
fn numeric_canonicalization_still_rejects_projected_nonfinite_provider_values() {
    assert_eq!(
        run(
            "SELECT ID FROM t",
            source("ID", vec![Scalar::Float(f64::INFINITY)]),
            100,
            1 << 20
        )
        .unwrap_err()
        .code,
        "relational.numeric_range"
    );
    assert!(
        run(
            "SELECT 1 FROM t",
            source("ID", vec![Scalar::Float(f64::INFINITY)]),
            100,
            1 << 20
        )
        .is_ok(),
        "plain Source's unused first-side scalar is not read by Go"
    );
}
#[test]
fn implicit_case_else_null_pays_the_same_bind_and_evaluation_nodes() {
    let first = Program::compile("EXPLAIN ANALYZE SELECT CASE WHEN FALSE THEN 1 END FROM t")
        .unwrap()
        .execute(
            &BTreeMap::from([("t".into(), source("ID", vec![Scalar::Int(1)]))]),
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap();
    let second =
        Program::compile("EXPLAIN ANALYZE SELECT CASE WHEN FALSE THEN 1 ELSE NULL END FROM t")
            .unwrap()
            .execute(
                &BTreeMap::from([("t".into(), source("ID", vec![Scalar::Int(1)]))]),
                &BTreeMap::new(),
                Limits::default(),
                None,
            )
            .unwrap();
    assert_eq!(
        serde_json::to_value(first.plan).unwrap(),
        serde_json::to_value(second.plan).unwrap()
    );
}
#[test]
fn expression_temporary_safety_is_released_without_refunding_logical_retention() {
    let shared = crate::resource::Budget::new(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let mut work = Work {
        left: 100,
        bytes: 1024,
        deadline: shared.deadline,
        cancel: None,
        shared: Some(&shared),
        planning: false,
        temporaries: Vec::new(),
        selection_sources: vec![],
        providers: &[],
        resolver: None,
        roots: BTreeMap::new(),
        measure: false,
        scans: vec![],
    };
    work.temporary(0).unwrap();
    work.temporaries.clear();
    let before = shared.snapshot().unwrap().retained_bytes;
    work.temporary_scope(|work| {
        work.clone_scalar(&Scalar::String("temporary".into()))?;
        work.retain(17)?;
        work.spend()
    })
    .unwrap();
    assert_eq!(work.bytes, 1007);
    assert_eq!(work.left, 99);
    assert_eq!(shared.snapshot().unwrap().retained_bytes, before + 17);
}

#[test]
fn cancellation_precedes_invalid_initial_executor_limits() {
    let cancel = AtomicBool::new(true);
    let error = Program::compile("SELECT ID FROM t")
        .unwrap()
        .execute(
            &BTreeMap::new(),
            &BTreeMap::new(),
            Limits {
                work: 0,
                memory_bytes: 0,
                ..Limits::default()
            },
            Some(&cancel),
        )
        .unwrap_err();
    assert_eq!(error.code, "operation.cancelled");
}
#[test]
fn full_binary_arguments_evaluate_before_boolean_type_validation() {
    let error = run(
        "SELECT 'bad' AND 18446744073709551616 FROM t",
        source("ID", vec![Scalar::Int(1)]),
        1000,
        1 << 20,
    )
    .unwrap_err();
    assert_eq!(error.code, "relational.numeric_range");
}
