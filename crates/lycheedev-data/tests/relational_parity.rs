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
fn retained_run(
    program: &Program,
    input: &BTreeMap<String, Source>,
    params: &BTreeMap<String, S>,
    cap: u64,
) -> (Result<Vec<Vec<S>>, String>, lycheedev_data::resource::Cost) {
    use lycheedev_data::resource::{Budget, Cost};
    use std::time::{Duration, Instant};
    let budget = Budget::new(
        Cost {
            retained_bytes: cap,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(5),
        None,
    );
    let result = program
        .execute_budgeted(input, params, Limits::default(), None, Some(&budget))
        .map(|result| result.rows)
        .map_err(|error| error.code);
    (result, budget.snapshot().unwrap())
}
fn empty_string_success_boundary(
    case: &str,
    program: &Program,
    input: &BTreeMap<String, Source>,
    params: &BTreeMap<String, S>,
) -> (u64, lycheedev_data::resource::Cost) {
    // Only determine the current implementation's fixed execution overhead.
    // The search is finite (at most 29 halvings of the existing 512 MiB cap),
    // has no shared state between runs, and never compiles a different program.
    let mut rejected = 0;
    let mut accepted = lycheedev_data::resource::Cost::defaults().retained_bytes;
    let (result, mut usage) = retained_run(program, input, params, accepted);
    assert_eq!(
        result,
        Ok(vec![vec![S::Int(0)]]),
        "{case}: empty upper={accepted}, usage={usage:?}"
    );
    while accepted - rejected > 1 {
        let cap = rejected + (accepted - rejected) / 2;
        let (result, actual) = retained_run(program, input, params, cap);
        match result {
            Ok(rows) => {
                assert_eq!(
                    rows,
                    vec![vec![S::Int(0)]],
                    "{case}: cap={cap}, usage={actual:?}"
                );
                accepted = cap;
                usage = actual;
            }
            Err(code) => {
                assert_eq!(
                    code, "records.query_resource_budget",
                    "{case}: cap={cap}, usage={actual:?}"
                );
                rejected = cap;
            }
        }
    }
    (accepted, usage)
}
#[test]
fn query_future_admission_rejects_before_scalar_evaluation() {
    // The current query future is 4152 bytes on the accepted Windows target.
    // Preserve the original small limits as an entry-admission regression;
    // their zero retained remainder is correct because first poll never ran.
    let text = "界".repeat(4096);
    let input = source(vec![vec![S::Int(1), S::String(text.clone())]]);
    for (case, sql, params, cap) in [
        (
            "column",
            "SELECT LENGTH(Name) FROM T".to_owned(),
            BTreeMap::new(),
            4096,
        ),
        (
            "parameter",
            "SELECT LENGTH(:text) FROM T".to_owned(),
            BTreeMap::from([("text".into(), S::String(text.clone()))]),
            4096,
        ),
        (
            "literal",
            format!("SELECT LENGTH('{text}') FROM T"),
            BTreeMap::new(),
            4096,
        ),
        (
            "concat",
            "SELECT Name||Name FROM T".to_owned(),
            BTreeMap::new(),
            2200,
        ),
    ] {
        let program = Program::compile(&sql).unwrap();
        let (result, actual) = retained_run(&program, &input, &params, cap);
        assert_eq!(
            result,
            Err("records.query_resource_budget".into()),
            "{case}: cap={cap}, actual={actual:?}"
        );
        assert_eq!(
            actual.retained_bytes, 0,
            "{case}: cap={cap}, actual={actual:?}"
        );
        assert_eq!(
            actual.decode_work, 1,
            "{case}: only the original query-entry charge may precede admission; cap={cap}, actual={actual:?}"
        );
        eprintln!("SQL early admission: case={case}, cap={cap}, actual={actual:?}");
    }
}
#[test]
fn literal_parameter_column_and_concat_allocations_require_shared_admission() {
    let empty_input = source(vec![vec![S::Int(1), S::String(String::new())]]);
    for (case, empty_sql, empty_params) in [
        ("column", "SELECT LENGTH(Name) FROM T", BTreeMap::new()),
        (
            "parameter",
            "SELECT LENGTH(:text) FROM T",
            BTreeMap::from([("text".into(), S::String(String::new()))]),
        ),
        ("literal", "SELECT LENGTH('') FROM T", BTreeMap::new()),
    ] {
        let empty = Program::compile(empty_sql).unwrap();
        let (cap, empty_usage) =
            empty_string_success_boundary(case, &empty, &empty_input, &empty_params);
        let repetitions = 4096.max(usize::try_from(cap / 3 + 1).unwrap());
        let scalar_bytes = repetitions.checked_mul("界".len()).unwrap();
        assert!(
            scalar_bytes + "SELECT LENGTH('') FROM T".len() <= 256 << 10,
            "{case}: calibration cap={cap} must fit the existing SQL bound before allocating fixture text"
        );
        let text = "界".repeat(repetitions);
        assert!(
            text.len() as u64 > cap,
            "{case}: clone alone must exceed cap={cap}"
        );
        let input = source(vec![vec![S::Int(1), S::String(text.clone())]]);
        let params = if case == "parameter" {
            BTreeMap::from([("text".into(), S::String(text.clone()))])
        } else {
            BTreeMap::new()
        };
        let sql = if case == "literal" {
            format!("SELECT LENGTH('{text}') FROM T")
        } else {
            empty_sql.to_owned()
        };
        assert!(
            sql.len() <= 256 << 10,
            "{case}: retain the original SQL input bound"
        );
        let program = Program::compile(&sql).unwrap();
        let (result, actual) = retained_run(&program, &input, &params, cap);
        assert_eq!(
            result,
            Err("records.query_resource_budget".into()),
            "{case}: cap={cap}, scalar_bytes={}, actual={actual:?}",
            text.len()
        );
        // Empty and large inputs have the same pre-clone metadata and Eval
        // nodes. Only results.finish's one-row spend is absent on rejection;
        // this cannot pass when the driver or argument stack rejects earlier.
        assert_eq!(
            actual.decode_work + 1,
            empty_usage.decode_work,
            "{case}: cap={cap}, actual={actual:?}, empty={empty_usage:?}"
        );
        // A scalar Int output costs 96 at projection and 96 at finish. These
        // cumulative Go charges are absent, while temporary ownership refunds.
        assert_eq!(
            actual.retained_bytes + 192,
            empty_usage.retained_bytes,
            "{case}: cap={cap}, actual={actual:?}, empty={empty_usage:?}"
        );
        let (success, admitted) = retained_run(&program, &input, &params, cap + text.len() as u64);
        assert_eq!(
            success,
            Ok(vec![vec![S::Int(repetitions as i64)]]),
            "{case}: cap={cap}, admitted={admitted:?}"
        );
        assert_eq!(
            admitted.decode_work,
            actual.decode_work + text.len() as u64 + 1,
            "{case}: actual={actual:?}, admitted={admitted:?}"
        );
        assert_eq!(
            admitted.retained_bytes, empty_usage.retained_bytes,
            "{case}: temporary scalar bytes must refund; admitted={admitted:?}, empty={empty_usage:?}"
        );
        eprintln!(
            "SQL scalar admission: case={case}, cap={cap}, scalar_bytes={scalar_bytes}, rejected={actual:?}, admitted={admitted:?}"
        );
    }
    let concat = Program::compile("SELECT LENGTH(Name||Name) FROM T").unwrap();
    let (overhead, _) =
        empty_string_success_boundary("concat", &concat, &empty_input, &BTreeMap::new());
    let operand = 1024.max(usize::try_from(overhead / 2 + 1).unwrap());
    assert!(
        2 * operand <= 1 << 20,
        "keep the original concatenation length bound"
    );
    let input = source(vec![vec![S::Int(1), S::String("a".repeat(operand))]]);
    let cap = overhead + (2 * operand) as u64;
    assert!(cap < (4 * operand) as u64);
    // Same AST/task shape and both operand clones, but equality creates no
    // string destination. LENGTH's type error proves both clones completed.
    let equality = Program::compile("SELECT LENGTH(Name=Name) FROM T").unwrap();
    let (witness, equality_usage) = retained_run(&equality, &input, &BTreeMap::new(), cap);
    assert_eq!(
        witness,
        Err("relational.type_mismatch".into()),
        "concat operands: cap={cap}, operand={operand}, actual={equality_usage:?}"
    );
    let (result, rejected) = retained_run(&concat, &input, &BTreeMap::new(), cap);
    assert_eq!(
        result,
        Err("records.query_resource_budget".into()),
        "concat destination: cap={cap}, operand={operand}, actual={rejected:?}"
    );
    assert_eq!(
        rejected, equality_usage,
        "concat must fail at destination admission after the same two operand evaluations; cap={cap}, operand={operand}"
    );
    let (result, admitted) = retained_run(
        &concat,
        &input,
        &BTreeMap::new(),
        overhead + (4 * operand) as u64,
    );
    assert_eq!(
        result,
        Ok(vec![vec![S::Int((2 * operand) as i64)]]),
        "concat success: overhead={overhead}, operand={operand}, actual={admitted:?}"
    );
    assert_eq!(
        admitted.decode_work,
        rejected.decode_work + (2 * operand) as u64 + 1,
        "concat: admitted={admitted:?}, rejected={rejected:?}"
    );
    assert_eq!(
        admitted.retained_bytes,
        rejected.retained_bytes + 192,
        "concat: only cumulative Int output charges remain; admitted={admitted:?}, rejected={rejected:?}"
    );
    eprintln!(
        "SQL concat admission: overhead={overhead}, cap={cap}, operand_bytes={operand}, rejected={rejected:?}, admitted={admitted:?}"
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
