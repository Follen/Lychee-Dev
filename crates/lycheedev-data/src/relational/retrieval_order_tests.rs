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
        assert!(error.message.contains("expected FROM"), "{sql}: {error:?}");
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
    assert!(first.message.contains("expected FROM"), "{first:?}");
}
#[test]
fn integer_count_rejection_precedes_later_union_body_errors() {
    for suffix in ["LIMIT -1", "LIMIT 1.0", "LIMIT (1)", "OFFSET TRUE"] {
        let sql = format!("SELECT ID FROM t {suffix} UNION ALL SELECT 1 WHERE (");
        let error = Program::compile(&sql).unwrap_err();
        assert_eq!(error.code, "relational.invalid_syntax");
        assert!(error.message.contains("integer"), "{sql}: {error:?}");
        assert!(!error.message.contains("expected FROM"));
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
            .contains("expected FROM")
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
    assert!(
        retrieval::parse(&dialect, &mut invalid)
            .map(ast_owner::Owned::new)
            .is_err()
    );
    let mut valid = Parser::new(&dialect)
        .with_tokens_with_locations(lexical::tokens("SELECT ID FROM t").unwrap());
    assert!(
        retrieval::parse(&dialect, &mut valid)
            .map(ast_owner::Owned::new)
            .is_ok()
    );
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

#[test]
fn invalid_primary_field_extensions_stop_at_the_first_period() {
    for value in [
        "t.ID.extra",
        "t.*.extra",
        "(ID).extra",
        "TRUE.extra",
        "NULL.extra",
        "'text'.extra",
        "1 .extra",
        ":p.extra",
        "COUNT(ID).extra",
        "CAST(ID AS INT).extra",
        "CASE WHEN TRUE THEN ID END.extra",
        "(SELECT ID FROM t).extra",
        "EXISTS(SELECT ID FROM t).extra",
    ] {
        for sql in [
            format!("SELECT {value} FROM t"),
            format!("SELECT {value}+ ( WHERE ("),
            format!("SELECT COALESCE({value}, ( WHERE () FROM t"),
        ] {
            let error = Program::compile(&sql).unwrap_err();
            assert_eq!(error.code, "relational.invalid_syntax", "{sql}: {error:?}");
            assert!(
                error.message.contains("end of scalar expression"),
                "{sql}: {error:?}"
            );
        }
    }
}
#[test]
fn valid_primary_forms_retain_exact_typed_values_and_two_component_names() {
    let program = Program::compile("SELECT t.ID,((t.*)),TRUE,FALSE,NULL,'a''b',1.25,1e-7,:p,CAST(ID AS INT),CASE ID WHEN 1 THEN 7 END,COALESCE(NULL,ID) FROM t").unwrap();
    let result = program
        .execute(
            &input(),
            &BTreeMap::from([("p".into(), Scalar::UInt(u64::MAX))]),
            Limits::default(),
            None,
        )
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![
            Scalar::Int(1),
            Scalar::Int(1),
            Scalar::Bool(true),
            Scalar::Bool(false),
            Scalar::Null,
            Scalar::String("a'b".into()),
            Scalar::Float(1.25),
            Scalar::Float(1e-7),
            Scalar::UInt(u64::MAX),
            Scalar::Int(1),
            Scalar::Int(7),
            Scalar::Int(1)
        ]]
    );
}

fn parenthesized(value: &str, count: usize) -> String {
    format!("{}{}{}", "(".repeat(count), value, ")".repeat(count))
}
#[test]
fn native_infix_rhs_observation_preserves_go_128_and_129_boundaries() {
    for value in [
        "1+1",
        "1-1",
        "1*1",
        "1/1",
        "1%1",
        "'a'||'b'",
        "1=1",
        "1<>1",
        "1<1",
        "1<=1",
        "1>1",
        "1>=1",
        "TRUE AND TRUE",
        "FALSE OR TRUE",
        "'a' LIKE 'a'",
        "'a' NOT LIKE 'a'",
        "1 BETWEEN 0 AND 2",
        "1 NOT BETWEEN 0 AND 2",
    ] {
        // retrieve + outer expression +125 nested expressions +one native RHS.
        let valid = format!("SELECT {} FROM t", parenthesized(value, 125));
        Program::compile(&valid).unwrap_or_else(|error| panic!("{value}: {error:?}"));
        let too_deep = format!("SELECT {} FROM t", parenthesized(value, 126));
        assert_eq!(
            Program::compile(&too_deep).unwrap_err().code,
            "relational.resource_limit",
            "{value}"
        );
    }
}
#[test]
fn native_rhs_depth_stops_before_later_malformed_primary_for_binary_and_between() {
    for value in [
        "1+(WHERE",
        "1 BETWEEN (WHERE",
        "1 BETWEEN 0 AND (WHERE",
        "'a' LIKE (WHERE",
    ] {
        let sql = format!("SELECT {}{value}", "(".repeat(125));
        assert_eq!(
            Program::compile(&sql).unwrap_err().code,
            "relational.resource_limit",
            "{sql}"
        );
        let within_limit = format!("SELECT {}{value}", "(".repeat(124));
        assert_eq!(
            Program::compile(&within_limit).unwrap_err().code,
            "relational.invalid_syntax",
            "{within_limit}"
        );
    }
}
#[test]
fn native_rhs_depth_combines_with_every_query_entry_without_extra_wrapper_frames() {
    for template in [
        "WITH c AS (SELECT VALUE FROM t) SELECT ID FROM t",
        "SELECT ID FROM (SELECT VALUE FROM t) c",
        "SELECT (SELECT VALUE FROM t) FROM t",
        "SELECT EXISTS(SELECT VALUE FROM t) FROM t",
        "SELECT ID IN (SELECT VALUE FROM t) FROM t",
    ] {
        // CTE/derived have one extra retrieve frame. Expression subqueries also
        // retain their outer expression frame while the nested query is parsed.
        let extra = if template.starts_with("WITH") || template.starts_with("SELECT ID FROM (") {
            1
        } else {
            2
        };
        let valid = template.replace("VALUE", &parenthesized("1+1", 125 - extra));
        Program::compile(&valid).unwrap_or_else(|error| panic!("{valid}: {error:?}"));
        let invalid = template.replace("VALUE", &parenthesized("1+1", 126 - extra));
        assert_eq!(
            Program::compile(&invalid).unwrap_err().code,
            "relational.resource_limit",
            "{invalid}"
        );
    }
}
#[test]
fn native_rhs_markers_release_after_errors_and_do_not_accumulate_left_associative_chains() {
    let dialect = syntax::QueryDialect::default();
    for sql in [
        "SELECT 1+ FROM t",
        "SELECT 1 BETWEEN 0 WHERE (",
        "SELECT 1 BETWEEN 0 AND WHERE (",
    ] {
        let mut parser =
            Parser::new(&dialect).with_tokens_with_locations(lexical::tokens(sql).unwrap());
        assert!(
            retrieval::parse(&dialect, &mut parser)
                .map(ast_owner::Owned::new)
                .is_err(),
            "{sql}"
        );
        dialect.take_stop().unwrap();
        let valid = format!("SELECT {} FROM t", parenthesized("1+1", 125));
        let mut parser = Parser::new(&dialect)
            .with_recursion_limit(256)
            .with_tokens_with_locations(lexical::tokens(&valid).unwrap());
        retrieval::parse(&dialect, &mut parser)
            .map(ast_owner::Owned::new)
            .unwrap_or_else(|error| panic!("after {sql}: {error:?}"));
        dialect.take_stop().unwrap();
    }
    // Each next '+' closes the preceding RHS at the same precedence, so a
    // hundred operators use two active expression frames, not a hundred.
    let sql = format!(
        "SELECT {} FROM t",
        std::iter::repeat_n("1", 100).collect::<Vec<_>>().join("+")
    );
    assert_eq!(run(&sql).unwrap().rows, vec![vec![Scalar::Int(100)]]);
}
#[test]
fn between_rhs_phase_does_not_consume_outer_infix_or_owned_prefix_depth() {
    for sql in [
        "SELECT 1+2*3 FROM t",
        "SELECT 1 BETWEEN 0+0 AND 1+2 AND TRUE FROM t",
        "SELECT 1 BETWEEN (CASE WHEN TRUE THEN 0 END) AND (SELECT ID+1 FROM t) FROM t",
        "SELECT 1 + CASE WHEN TRUE THEN 1 BETWEEN 0 AND 2 ELSE FALSE END FROM t",
    ] {
        // The final case is a runtime type error, but its grammar must compile.
        Program::compile(sql).unwrap_or_else(|error| panic!("{sql}: {error:?}"));
    }
    assert_eq!(
        run("SELECT 1+2*3,1 BETWEEN 0+0 AND 1+2 AND TRUE FROM t")
            .unwrap()
            .rows,
        vec![vec![Scalar::Int(7), Scalar::Bool(true)]]
    );
}
