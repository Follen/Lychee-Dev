//! One retrieval shell in every query position, fixed Go compile/execute order.
use super::*;
fn input() -> BTreeMap<String, Source> {
    BTreeMap::from([
        (
            "t".into(),
            Source {
                columns: vec!["ID".into()],
                rows: vec![vec![Scalar::Int(1)]],
                complete: true,
            },
        ),
        (
            "c".into(),
            Source {
                columns: vec!["ID".into()],
                rows: vec![vec![Scalar::Int(9)]],
                complete: true,
            },
        ),
    ])
}
fn run(sql: &str) -> Result<QueryResult> {
    Program::compile(sql)?.execute(&input(), &BTreeMap::new(), Limits::default(), None)
}
#[test]
fn missing_from_wins_before_malformed_where_in_every_retrieval_position() {
    for sql in [
        "SELECT 1 WHERE (",
        "WITH c AS (SELECT 1 WHERE () SELECT ID FROM t",
        "SELECT ID FROM (SELECT 1 WHERE () x",
        "SELECT (SELECT 1 WHERE () FROM t",
        "SELECT EXISTS(SELECT 1 WHERE () FROM t",
        "SELECT ID IN (SELECT 1 WHERE () FROM t",
        "SELECT ID NOT IN (SELECT 1 WHERE () FROM t",
        "SELECT ID FROM t UNION ALL SELECT 1 WHERE (",
        "SELECT CASE WHEN TRUE THEN (SELECT 1 WHERE () END FROM t",
        "SELECT COALESCE((SELECT 1 WHERE (),1) FROM t",
        "SELECT CAST((SELECT 1 WHERE () AS INT) FROM t",
    ] {
        let error = Program::compile(sql).unwrap_err();
        assert_eq!(error.code, "relational.invalid_syntax", "{sql}: {error:?}");
        assert!(error.message.contains("Expected: FROM"), "{sql}: {error:?}");
    }
}
#[test]
fn lexical_error_still_precedes_the_earlier_missing_from() {
    let error = Program::compile("SELECT 1 WHERE ( /* unclosed").unwrap_err();
    assert_eq!(error.code, "relational.invalid_syntax");
    assert!(error.message.contains("unclosed comment"));
}
#[test]
fn duplicate_cte_is_diagnosed_before_its_as_or_later_body() {
    let error =
        Program::compile("WITH c AS (SELECT ID FROM t), c WHERE ( SELECT ID FROM t").unwrap_err();
    assert!(
        error.message.contains("unique common table name"),
        "{error:?}"
    );
    let first =
        Program::compile("WITH c AS (SELECT 1 WHERE (), c WHERE ( SELECT ID FROM t").unwrap_err();
    assert!(first.message.contains("Expected: FROM"), "{first:?}");
}
#[test]
fn integer_count_rejection_precedes_later_union_body_errors() {
    for suffix in ["LIMIT -1", "LIMIT 1.0", "LIMIT (1)", "OFFSET TRUE"] {
        let sql = format!("SELECT ID FROM t {suffix} UNION ALL SELECT 1 WHERE (");
        let error = Program::compile(&sql).unwrap_err();
        assert_eq!(error.code, "relational.invalid_syntax");
        assert!(error.message.contains("integer"), "{sql}: {error:?}");
        assert!(!error.message.contains("Expected: FROM"));
    }
}
#[test]
fn all_nonfinal_union_suffixes_compile_then_fail_actual_preflight() {
    for suffix in [
        "ORDER BY ID",
        "LIMIT 1",
        "OFFSET 1",
        "ORDER BY ID LIMIT 1 OFFSET 1",
    ] {
        let sql = format!("SELECT ID FROM t {suffix} UNION ALL SELECT ID FROM t");
        let program = Program::compile(&sql).unwrap();
        assert_eq!(
            program
                .execute(&input(), &BTreeMap::new(), Limits::default(), None)
                .unwrap_err()
                .code,
            "relational.invalid_syntax",
            "{sql}"
        );
    }
}
#[test]
fn nested_nonfinal_suffixes_reach_the_same_schema_execution_check() {
    let bad = "SELECT ID FROM t LIMIT 1 UNION ALL SELECT ID FROM t";
    for sql in [
        format!("WITH c AS ({bad}) SELECT ID FROM c"),
        format!("SELECT ID FROM ({bad}) x"),
        format!("SELECT ({bad}) FROM t"),
        format!("SELECT EXISTS({bad}) FROM t"),
        format!("SELECT ID IN ({bad}) FROM t"),
        format!("SELECT ID NOT IN ({bad}) FROM t"),
        format!("SELECT ID FROM t ORDER BY ({bad})"),
        format!("SELECT COUNT(*) FROM t HAVING EXISTS({bad})"),
    ] {
        let program = Program::compile(&sql).unwrap_or_else(|e| panic!("compile {sql}: {e:?}"));
        assert_eq!(
            program
                .execute(&input(), &BTreeMap::new(), Limits::default(), None)
                .unwrap_err()
                .code,
            "relational.invalid_syntax",
            "{sql}"
        );
    }
}
#[test]
fn right_branch_cte_and_tail_suffix_keep_their_real_scope() {
    let result = run("SELECT ID FROM t UNION ALL WITH c AS (SELECT ID FROM t) SELECT ID FROM c UNION ALL SELECT ID FROM c ORDER BY ID").unwrap();
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(1)],
            vec![Scalar::Int(9)]
        ]
    );
}
#[test]
fn first_cte_is_visible_to_all_union_branches_without_reexecution() {
    let result = run("WITH c AS (SELECT ID FROM t) SELECT ID FROM c UNION ALL SELECT ID FROM c ORDER BY ID LIMIT 1").unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
}
#[test]
fn right_branch_with_is_supported_in_all_expression_query_positions() {
    let inner = "SELECT ID FROM t UNION ALL WITH c AS (SELECT ID FROM t) SELECT ID FROM c";
    for sql in [
        format!("SELECT EXISTS({inner}) FROM t"),
        format!("SELECT ID IN ({inner}) FROM t"),
        format!("SELECT ID NOT IN ({inner}) FROM t"),
    ] {
        assert!(run(&sql).is_ok(), "{sql}");
    }
    let sql = format!("SELECT ({inner}) FROM t");
    assert_eq!(
        run(&sql).unwrap_err().code,
        "relational.invalid_cardinality"
    );
}
#[test]
fn query_shell_preserves_numeric_in_and_explicit_star_alias_binding_time() {
    assert_eq!(
        run("SELECT ID FROM t WHERE ID IN (1,1.0,NULL)")
            .unwrap()
            .rows,
        vec![vec![Scalar::Int(1)]]
    );
    assert_eq!(
        run("SELECT ((t.*)) FROM t").unwrap().rows,
        vec![vec![Scalar::Int(1)]]
    );
    let program = Program::compile("SELECT * AS x FROM t").unwrap();
    assert_eq!(
        program
            .execute(&input(), &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.invalid_binding"
    );
}
#[test]
fn owned_prefix_depth_is_checked_before_a_later_grammar_error() {
    let sql = format!("SELECT {}ID{} WHERE (", "(".repeat(127), ")".repeat(127));
    assert_eq!(
        Program::compile(&sql).unwrap_err().code,
        "relational.resource_limit"
    );
    let sql = format!("SELECT {}ID{} WHERE (", "(".repeat(126), ")".repeat(126));
    assert!(
        Program::compile(&sql)
            .unwrap_err()
            .message
            .contains("Expected: FROM")
    );
}
#[test]
fn semicolon_and_query_parenthesis_rules_do_not_gain_extra_statements() {
    for sql in [
        "(SELECT ID FROM t)",
        "SELECT ID FROM t UNION ALL (SELECT ID FROM t)",
        "SELECT ID FROM t; SELECT ID FROM t",
        "SELECT ID FROM ((SELECT ID FROM t)) x",
    ] {
        assert_eq!(
            Program::compile(sql).unwrap_err().code,
            "relational.invalid_syntax",
            "{sql}"
        );
    }
    assert!(Program::compile("SELECT ID FROM t; -- final").is_ok());
}
#[test]
fn syntax_failure_releases_owned_query_depth_for_same_dialect() {
    let dialect = syntax::QueryDialect::default();
    let mut invalid = Parser::new(&dialect)
        .with_tokens_with_locations(lexical::tokens("SELECT 1 WHERE (").unwrap());
    assert!(retrieval::parse(&dialect, &mut invalid).is_err());
    let mut valid = Parser::new(&dialect)
        .with_tokens_with_locations(lexical::tokens("SELECT ID FROM t").unwrap());
    assert!(retrieval::parse(&dialect, &mut valid).is_ok());
}

#[test]
fn union_declared_sources_preserve_go_inspection_scope_separately_from_execution() {
    let program=Program::compile("SELECT ID FROM t UNION ALL WITH c AS (SELECT ID FROM t) SELECT ID FROM c UNION ALL SELECT ID FROM c").unwrap();
    // Fixed Go inspect pushes union using that query's lexical scope, while
    // execution copies each branch without inheriting preceding branch locals.
    assert_eq!(program.source_tables(), ["t"]);
    assert_eq!(
        program
            .execute(&input(), &BTreeMap::new(), Limits::default(), None)
            .unwrap()
            .rows,
        vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(1)],
            vec![Scalar::Int(9)]
        ]
    );
}
