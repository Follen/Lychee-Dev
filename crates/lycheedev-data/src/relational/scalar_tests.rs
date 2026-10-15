//! Fixed Go 3.2.3 evaluate_test.go, functions_test.go, values.go and evaluate.go.
//! Native execution status is NOT_RUN.
use super::*;
use crate::resource::{Budget, Cost};
fn evaluate(expr: &str) -> Result<Scalar> {
    let source = Source {
        columns: vec!["ID".into()],
        rows: vec![vec![Scalar::Int(1)]],
        complete: true,
    };
    let result = Program::compile(&format!("SELECT {expr} FROM t"))?.execute(
        &BTreeMap::from([("t".into(), source)]),
        &BTreeMap::new(),
        Limits::default(),
        None,
    )?;
    Ok(result
        .rows
        .into_iter()
        .next()
        .unwrap()
        .into_iter()
        .next()
        .unwrap())
}
#[test]
fn cast_preserves_fixed_go_exact_numbers_truncation_null_and_bool() {
    for (expr, want) in [
        (
            "CAST(18446744073709551615 AS TEXT)",
            Scalar::String("18446744073709551615".into()),
        ),
        ("CAST('-2.75' AS INTEGER)", Scalar::Int(-2)),
        ("CAST(3 AS REAL)", Scalar::Float(3.0)),
        ("CAST('true' AS BOOLEAN)", Scalar::Bool(true)),
        ("CAST(NULL AS INTEGER)", Scalar::Null),
        (
            "CAST('9223372036854775807' AS BIGINT)",
            Scalar::Int(i64::MAX),
        ),
        ("CAST('-9223372036854775808' AS INT)", Scalar::Int(i64::MIN)),
        ("CAST(1.0 AS BOOL)", Scalar::Bool(true)),
        ("CAST(-0.0 AS BOOLEAN)", Scalar::Bool(false)),
        ("CAST(1e-5 AS VARCHAR)", Scalar::String("1e-05".into())),
        ("CAST(1e6 AS STRING)", Scalar::String("1e+06".into())),
        ("CAST(-0.0 AS TEXT)", Scalar::String("-0".into())),
    ] {
        assert_eq!(evaluate(expr).unwrap(), want, "{expr}");
    }
}
#[test]
fn cast_rejects_invalid_types_range_and_non_decimal_text() {
    for (expr, code) in [
        ("CAST(NULL AS MISSING)", "relational.unsupported"),
        ("CAST('abc' AS REAL)", "relational.type_mismatch"),
        (
            "CAST(18446744073709551615 AS BIGINT)",
            "relational.numeric_range",
        ),
        (
            "CAST(-9223372036854775809 AS BIGINT)",
            "relational.numeric_range",
        ),
        ("CAST('1e999' AS REAL)", "relational.numeric_range"),
        ("CAST(2 AS BOOL)", "relational.type_mismatch"),
        ("CAST(TRUE AS INTEGER)", "relational.type_mismatch"),
        ("CAST('+1' AS INTEGER)", "relational.type_mismatch"),
        ("CAST(' 1' AS FLOAT)", "relational.type_mismatch"),
        ("CAST('0x10' AS BIGINT)", "relational.type_mismatch"),
        ("CAST('1_0' AS REAL)", "relational.type_mismatch"),
        ("CAST('1.' AS BOOL)", "relational.type_mismatch"),
    ] {
        assert_eq!(evaluate(expr).unwrap_err().code, code, "{expr}");
    }
}
#[test]
fn cast_boolean_text_follows_parse_bool_spellings_without_casefold() {
    for text in ["1", "t", "T", "TRUE", "true", "True"] {
        assert_eq!(
            evaluate(&format!("CAST('{text}' AS BOOLEAN)")).unwrap(),
            Scalar::Bool(true)
        );
    }
    for text in ["0", "f", "F", "FALSE", "false", "False"] {
        assert_eq!(
            evaluate(&format!("CAST('{text}' AS BOOLEAN)")).unwrap(),
            Scalar::Bool(false)
        );
    }
    for text in ["TrUe", "yes", " 1", "false ", "01"] {
        assert_eq!(
            evaluate(&format!("CAST('{text}' AS BOOLEAN)"))
                .unwrap_err()
                .code,
            "relational.type_mismatch"
        );
    }
}
#[test]
fn like_uses_case_sensitive_unicode_cells_and_literal_backslash() {
    for (expr, want) in [
        ("'Warrior' LIKE 'W%r'", true),
        ("'Warrior' NOT LIKE 'w%'", true),
        ("'猫' LIKE '_'", true),
        ("'' LIKE '%%'", true),
        ("'' LIKE '_'", false),
        ("'aab' LIKE '%a_b'", true),
        ("'猫犬' LIKE '_犬'", true),
        ("'a%z' LIKE 'a%z'", true),
        ("'abc' LIKE 'a\\%'", false),
        ("'\\abc' LIKE '\\%'", true),
        ("'Σ' LIKE 'σ'", false),
    ] {
        assert_eq!(evaluate(expr).unwrap(), Scalar::Bool(want), "{expr}");
    }
    assert_eq!(evaluate("NULL NOT LIKE '%'").unwrap(), Scalar::Null);
    assert_eq!(evaluate("1 LIKE NULL").unwrap(), Scalar::Null);
    assert_eq!(evaluate("NULL LIKE 1").unwrap(), Scalar::Null);
    assert_eq!(
        evaluate("1 LIKE '%'").unwrap_err().code,
        "relational.type_mismatch"
    );
}
fn work(shared: Option<&Budget>) -> Work<'_> {
    Work {
        left: 10000,
        bytes: 1 << 20,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: None,
        shared,
        planning: false,
        selection_sources: vec![],
        providers: &[],
        resolver: None,
        roots: BTreeMap::new(),
        measure: false,
        scans: vec![],
    }
}
#[test]
fn like_charges_exact_go_cells_and_releases_only_actual_temporary_ownership() {
    let mut w = work(None);
    let initial = w.bytes;
    assert_eq!(
        like(
            &Scalar::String("猫犬".into()),
            &Scalar::String("_%".into()),
            false,
            &mut w
        )
        .unwrap(),
        Scalar::Bool(true)
    );
    // 2 initialization cells + 2 text iterations + 2*2 inner cells.
    assert_eq!(w.left, 10000 - 8);
    assert_eq!(w.bytes, initial);
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        None,
    );
    let before = budget.snapshot().unwrap();
    let mut w = work(Some(&budget));
    like(
        &Scalar::String("猫犬".into()),
        &Scalar::String("_%".into()),
        false,
        &mut w,
    )
    .unwrap();
    let after = budget.snapshot().unwrap();
    assert_eq!(after.retained_bytes, before.retained_bytes);
    assert_eq!(after.decode_work - before.decode_work, 8);
}
#[test]
fn like_budget_and_cancel_failures_are_fatal_without_partial_boolean() {
    let mut w = work(None);
    w.left = 7;
    assert_eq!(
        like(
            &Scalar::String("猫犬".into()),
            &Scalar::String("_%".into()),
            false,
            &mut w
        )
        .unwrap_err()
        .code,
        "relational.resource_limit"
    );
    let cancelled = AtomicBool::new(true);
    let mut w = work(None);
    w.cancel = Some(&cancelled);
    assert_eq!(
        like(
            &Scalar::String("x".into()),
            &Scalar::String("%".into()),
            false,
            &mut w
        )
        .unwrap_err()
        .code,
        "operation.cancelled"
    );
}
#[test]
fn cast_and_like_traverse_nested_queries_group_keys_and_schema_only_explain() {
    let source = Source {
        columns: vec!["ID".into()],
        rows: vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]],
        complete: true,
    };
    let sources = BTreeMap::from([("t".into(), source)]);
    for sql in [
        "SELECT CAST((SELECT ID FROM t b WHERE b.ID=a.ID) AS TEXT) FROM t a ORDER BY a.ID",
        "SELECT CAST(ID AS TEXT),COUNT(*) FROM t GROUP BY CAST(ID AS TEXT) ORDER BY CAST(ID AS TEXT)",
        "SELECT ID FROM t a WHERE CAST(a.ID AS TEXT) LIKE (SELECT CAST(ID AS TEXT) FROM t b WHERE b.ID=a.ID)",
        "EXPLAIN SELECT CAST((SELECT ID FROM t b) AS TEXT) LIKE '%' FROM t",
    ] {
        assert!(
            Program::compile(sql)
                .unwrap()
                .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                .is_ok(),
            "{sql}"
        );
    }
    assert_eq!(
        Program::compile("SELECT CASE WHEN FALSE THEN CAST(ID AS MISSING) ELSE ID END FROM t")
            .unwrap()
            .execute(&sources, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.unsupported"
    );
}

#[test]
fn quoted_scalar_function_and_cast_type_keep_identifier_values_and_go_unicode_upper() {
    for expr in [r#"CAST(1 AS "TEXT")"#, r#"CAST(1 AS "ſtring")"#] {
        assert_eq!(evaluate(expr).unwrap(), Scalar::String("1".into()));
    }
    assert_eq!(
        evaluate(r#""lower"('ABC')"#).unwrap(),
        Scalar::String("abc".into())
    );
    assert_eq!(evaluate(r#""ſum"(ID)"#).unwrap(), Scalar::Int(1));
}

#[test]
fn bit_functions_preserve_fixed_go_left_to_right_null_and_type_error_order() {
    for name in ["HAS_FLAG", "BIT_AND"] {
        assert_eq!(
            evaluate(&format!("{name}(NULL,CAST('bad' AS INTEGER))")).unwrap(),
            Scalar::Null
        );
        assert_eq!(
            evaluate(&format!("{name}(1.5,NULL)")).unwrap_err().code,
            "relational.type_mismatch"
        );
        assert_eq!(
            evaluate(&format!("{name}('bad',CAST('1e999' AS FLOAT))"))
                .unwrap_err()
                .code,
            "relational.type_mismatch"
        );
    }
}
#[test]
fn scalar_string_copies_and_unicode_case_are_safety_only_not_extra_go_retention() {
    let mut w = work(None);
    let initial = w.bytes;
    assert_eq!(
        w.clone_scalar(&Scalar::String("猫".into())).unwrap(),
        Scalar::String("猫".into())
    );
    assert_eq!(case_text("Σſ猫", true, &mut w).unwrap(), "ΣS猫");
    assert_eq!(w.bytes, initial);
    assert_eq!(w.left, 10000 - "Σſ猫".len() as u64);
}
