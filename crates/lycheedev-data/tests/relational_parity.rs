use lycheedev_data::relational::{Limits, Program, Scalar as S, Source};
use std::{collections::BTreeMap, sync::atomic::AtomicBool};
fn source(rows: Vec<Vec<S>>) -> BTreeMap<String, Source> {
    BTreeMap::from([(
        "T".into(),
        Source {
            columns: vec!["ID".into(), "Name".into()],
            rows,
            complete: true,
        },
    )])
}
fn query(sql: &str) -> Vec<Vec<S>> {
    Program::compile(sql)
        .unwrap()
        .execute(
            &source(vec![vec![S::UInt(u64::MAX), S::String("冰雪".into())]]),
            &BTreeMap::new(),
            Limits::default(),
            None,
        )
        .unwrap()
        .rows
}
#[test]
fn exact_u64_math_flags_unicode_and_null_truth() {
    assert_eq!(
        query(
            "SELECT ID, ID=18446744073709551615, ID>18446744073709549568.0, BIT_AND(ID,9223372036854775808), HAS_FLAG(ID,9223372036854775808), LENGTH(Name) FROM T"
        ),
        vec![vec![
            S::UInt(u64::MAX),
            S::Bool(true),
            S::Bool(true),
            S::UInt(1 << 63),
            S::Bool(true),
            S::Int(2)
        ]]
    );
    assert_eq!(
        query(
            "SELECT 2 BETWEEN 3 AND NULL, 2 NOT BETWEEN 3 AND NULL, NULL AND FALSE, NULL OR TRUE, 1/0, 1%0 FROM T"
        ),
        vec![vec![
            S::Bool(false),
            S::Bool(true),
            S::Bool(false),
            S::Bool(true),
            S::Null,
            S::Null
        ]]
    );
    assert_eq!(
        query(
            "SELECT 18446744073709551615/18446744073709551615, -7%3, COALESCE(NULL,Name), NULLIF(ID,18446744073709551615) FROM T"
        ),
        vec![vec![
            S::Float(1.),
            S::Int(-1),
            S::String("冰雪".into()),
            S::Null
        ]]
    );
}
#[test]
fn simple_case_mapping_keeps_german_turkish_greek_and_contextual_sigma() {
    assert_eq!(
        query("SELECT UPPER('ß'), LOWER('İ'), UPPER('ᾀ'), LOWER('ΟΣ'), UPPER('ﬃ') FROM T"),
        vec![vec![
            S::String("ß".into()),
            S::String("i".into()),
            S::String("ᾈ".into()),
            S::String("οσ".into()),
            S::String("ﬃ".into())
        ]]
    );
}
#[test]
fn literal_parameter_column_and_concat_allocations_require_shared_admission() {
    use lycheedev_data::resource::{Budget, Cost};
    use std::time::{Duration, Instant};
    let text = "界".repeat(4096);
    let input = source(vec![vec![S::Int(1), S::String(text.clone())]]);
    for (sql, params) in [
        ("SELECT LENGTH(Name) FROM T".to_owned(), BTreeMap::new()),
        (
            "SELECT LENGTH(:text) FROM T".to_owned(),
            BTreeMap::from([("text".into(), S::String(text.clone()))]),
        ),
        (format!("SELECT LENGTH('{text}') FROM T"), BTreeMap::new()),
    ] {
        let budget = Budget::new(
            Cost {
                retained_bytes: 4096,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(5),
            None,
        );
        let error = Program::compile(&sql)
            .unwrap()
            .execute_budgeted(&input, &params, Limits::default(), None, Some(&budget))
            .unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget");
        let admitted = budget.snapshot().unwrap().retained_bytes;
        assert!(
            admitted >= std::mem::size_of::<S>() as u64 && admitted < text.len() as u64,
            "source-map metadata/argument container may be admitted, but the large scalar clone must not be"
        );
    }
    let input = source(vec![vec![S::Int(1), S::String("a".repeat(1024))]]);
    let budget = Budget::new(
        Cost {
            retained_bytes: 2200,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(5),
        None,
    );
    assert_eq!(
        Program::compile("SELECT Name||Name FROM T")
            .unwrap()
            .execute_budgeted(
                &input,
                &BTreeMap::new(),
                Limits::default(),
                None,
                Some(&budget)
            )
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    let admitted = budget.snapshot().unwrap().retained_bytes;
    assert!(
        admitted > 0 && admitted < 2048,
        "temporary operands are released on failure; only schema and allocation metadata remain"
    );
}
#[test]
fn all_boolean_and_in_operands_preserve_type_errors() {
    for sql in [
        "SELECT FALSE AND 1 FROM T",
        "SELECT TRUE OR 'true' FROM T",
        "SELECT 1 IN (1,'1') FROM T",
    ] {
        assert_eq!(
            Program::compile(sql)
                .unwrap()
                .execute(
                    &source(vec![vec![S::Int(1), S::Null]]),
                    &BTreeMap::new(),
                    Limits::default(),
                    None
                )
                .unwrap_err()
                .code,
            "relational.type_mismatch",
            "{sql}"
        );
    }
}
#[test]
fn supported_functions_check_arity_before_null_and_abs_stays_unsupported() {
    for sql in [
        "SELECT LOWER(NULL,1) FROM T",
        "SELECT UPPER(NULL,1) FROM T",
        "SELECT LENGTH(NULL,1) FROM T",
        "SELECT HAS_FLAG(NULL,1,2) FROM T",
        "SELECT BIT_AND(NULL,1,2) FROM T",
        "SELECT NULLIF(NULL,1,2) FROM T",
        "SELECT COALESCE() FROM T",
    ] {
        let error = Program::compile(sql)
            .unwrap()
            .execute(
                &source(vec![vec![S::Int(1), S::Null]]),
                &BTreeMap::new(),
                Limits::default(),
                None,
            )
            .unwrap_err();
        assert_eq!(error.code, "relational.type_mismatch", "{sql}");
    }
    for rows in [vec![], vec![vec![S::Int(1), S::Null]]] {
        let error = Program::compile("SELECT CASE WHEN TRUE THEN 1 ELSE LOWER(NULL,1) END FROM T")
            .unwrap()
            .execute(&source(rows), &BTreeMap::new(), Limits::default(), None)
            .unwrap_err();
        assert_eq!(
            error.code, "relational.type_mismatch",
            "unreachable/empty-source arity is still bound"
        );
    }
    for sql in [
        "SELECT ABS(NULL,1) FROM T",
        "SELECT ABS(-9223372036854775808) FROM T",
    ] {
        let program = Program::compile(sql).unwrap();
        for rows in [vec![], vec![vec![S::Int(1), S::Null]]] {
            assert_eq!(
                program
                    .execute(&source(rows), &BTreeMap::new(), Limits::default(), None)
                    .unwrap_err()
                    .code,
                "relational.unsupported",
                "{sql}: Go rejects unsupported functions during schema binding"
            );
        }
    }
    assert_eq!(
        query(
            "SELECT LOWER(NULL), UPPER(NULL), LENGTH(NULL), HAS_FLAG(NULL,1), BIT_AND(NULL,1), NULLIF(NULL,1) FROM T"
        ),
        vec![vec![S::Null; 6]]
    );
}
#[test]
fn preflight_binds_unreachable_branches_empty_sources_and_parameter_identity() {
    let empty = source(vec![]);
    let p = Program::compile("SELECT CASE WHEN TRUE THEN 1 ELSE Missing END FROM T").unwrap();
    assert_eq!(
        p.execute(&empty, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
    let p = Program::compile("SELECT ID FROM T WHERE ID=:id").unwrap();
    let params = BTreeMap::from([("id".into(), S::UInt(u64::MAX))]);
    assert_eq!(
        p.execute(
            &source(vec![vec![S::UInt(u64::MAX), S::Null]]),
            &params,
            Limits::default(),
            None
        )
        .unwrap()
        .rows,
        vec![vec![S::UInt(u64::MAX)]]
    );
    assert!(Program::compile("DELETE FROM T").is_err());
    assert!(Program::compile("SELECT ~ID FROM T").is_err());
}
#[test]
fn stable_order_alias_distinct_and_legacy_column_names() {
    let rows = vec![
        vec![S::Int(2), S::String("a".into())],
        vec![S::Int(1), S::String("b".into())],
        vec![S::Int(2), S::String("a".into())],
    ];
    let p =
        Program::compile("SELECT DISTINCT ID, ID+1 AS N FROM T ORDER BY N DESC LIMIT 2 OFFSET 0")
            .unwrap();
    let result = p
        .execute(&source(rows), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.columns, vec!["ID", "N"]);
    assert_eq!(
        result.rows,
        vec![vec![S::Int(2), S::Int(3)], vec![S::Int(1), S::Int(2)]]
    );
    let r = Program::compile("SELECT ID+1, T.Name FROM T")
        .unwrap()
        .execute(&source(vec![]), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(r.columns, vec!["column_1", "Name"]);
}
#[test]
fn bounded_nonfinite_overflow_string_work_and_cancel() {
    for sql in [
        "SELECT 18446744073709551615+1 FROM T",
        "SELECT 1e999 FROM T",
        "SELECT 1.7976931348623157e308*2.0 FROM T",
    ] {
        let result = Program::compile(sql).and_then(|p| {
            p.execute(
                &source(vec![vec![S::Int(1), S::Null]]),
                &BTreeMap::new(),
                Limits::default(),
                None,
            )
        });
        assert_eq!(result.unwrap_err().code, "relational.numeric_range");
    }
    let p = Program::compile("SELECT Name||Name FROM T").unwrap();
    let input = source(vec![vec![S::Int(1), S::String("x".repeat(600_000))]]);
    assert_eq!(
        p.execute(&input, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
    let p = Program::compile("SELECT ID FROM T").unwrap();
    assert_eq!(
        p.execute(
            &input,
            &BTreeMap::new(),
            Limits {
                work: 1,
                ..Limits::default()
            },
            None
        )
        .unwrap_err()
        .code,
        "relational.resource_limit"
    );
    assert_eq!(
        p.execute(
            &input,
            &BTreeMap::new(),
            Limits::default(),
            Some(&AtomicBool::new(true))
        )
        .unwrap_err()
        .exit_code,
        7
    );
}
