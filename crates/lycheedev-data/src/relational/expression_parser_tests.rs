//! Finite continuation grammar and original active-depth/typed-stop contracts.
//! Run on the ordinary test thread; no enlarged stack or weaker boundary.
use super::*;

fn parse(dialect: &syntax::QueryDialect, sql: &str) -> ast_owner::Owned<Query> {
    // A production retrieval must not enter the dependency's native Pratt
    // recursion at all. One dependency frame is enough even at Go depth 128.
    let mut parser = Parser::new(dialect)
        .with_recursion_limit(1)
        .with_tokens_with_locations(lexical::tokens(sql).unwrap());
    let query = retrieval::parse(dialect, &mut parser)
        .map(ast_owner::Owned::new)
        .unwrap_or_else(|error| panic!("{sql}: {error:?}"));
    dialect.take_stop().unwrap();
    assert_eq!(parser.peek_token().token, sqlparser::tokenizer::Token::EOF);
    query
}
fn surround(value: &str, count: usize) -> String {
    format!("{}{}{}", "(".repeat(count), value, ")".repeat(count))
}

#[test]
fn all_recursive_expression_forms_use_heap_continuations_at_original_depth() {
    let dialect = syntax::QueryDialect::default();
    let mut expressions = vec![
        surround("ID", 126),
        surround("1+1", 125),
        format!("{}TRUE", "NOT ".repeat(126)),
        format!("{}1", "- ".repeat(126)),
        format!("{}ID{}", "CAST(".repeat(126), " AS INT)".repeat(126)),
        format!("{}ID{}", "COALESCE(".repeat(126), ")".repeat(126)),
        format!(
            "{}ID{}",
            "CASE WHEN TRUE THEN ".repeat(126),
            " END".repeat(126)
        ),
        format!("{}ID{}", "1 IN (".repeat(126), ")".repeat(126)),
    ];
    let mut scalar = "SELECT ID FROM t".to_owned();
    for _ in 0..63 {
        scalar = format!("SELECT ({scalar}) FROM t");
    }
    // The pre-existing full compile tests independently retain scalar63/64,
    // derived63/64, parentheses126/127 and every native-RHS125/126 assertion.
    parse(&dialect, &scalar);
    for expression in expressions.drain(..) {
        parse(&dialect, &format!("SELECT {expression} FROM t"));
    }
}

#[test]
fn is_and_empty_calls_do_not_open_an_operand_frame_at_the_boundary() {
    let dialect = syntax::QueryDialect::default();
    for value in ["1 IS NULL", "1 IS NOT NULL", "COUNT()", "COUNT(DISTINCT)"] {
        parse(&dialect, &format!("SELECT {} FROM t", surround(value, 126)));
    }
    for value in ["1+1", "COUNT(1)", "1 IN (1)", "1 BETWEEN 0 AND 2"] {
        assert_eq!(
            Program::compile(&format!("SELECT {} FROM t", surround(value, 126)))
                .unwrap_err()
                .code,
            "relational.resource_limit",
            "{value}"
        );
    }
    let null_error =
        Program::compile(&format!("SELECT {}1 IS WHERE", "(".repeat(126))).unwrap_err();
    assert_eq!(null_error.code, "relational.invalid_syntax");
    assert!(null_error.message.contains("NULL"));
    assert_eq!(
        Program::compile(&format!("SELECT {}1+WHERE", "(".repeat(126)))
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
}

#[test]
fn failed_continuations_restore_the_same_dialect_for_exact_boundary_reuse() {
    let dialect = syntax::QueryDialect::default();
    for invalid in [
        "SELECT 1 IN () FROM t",
        "SELECT 1 IN (1,) FROM t",
        "SELECT CASE WHEN 1 THEN END FROM t",
        "SELECT CASE 1 END FROM t",
        "SELECT CAST(1 AS) FROM t",
        "SELECT COALESCE(1,) FROM t",
        "SELECT 1 BETWEEN 0 AND WHERE",
    ] {
        let mut parser =
            Parser::new(&dialect).with_tokens_with_locations(lexical::tokens(invalid).unwrap());
        assert!(
            retrieval::parse(&dialect, &mut parser)
                .map(ast_owner::Owned::new)
                .is_err(),
            "{invalid}"
        );
        dialect.take_stop().unwrap();
        parse(&dialect, &format!("SELECT {} FROM t", surround("1+1", 125)));
    }
    // A rejected entry also restores the caller's depth after its typed stop
    // is consumed. There is no lingering native-RHS marker or child task.
    let sql = format!("SELECT {} FROM t", surround("ID", 127));
    let mut parser =
        Parser::new(&dialect).with_tokens_with_locations(lexical::tokens(&sql).unwrap());
    assert!(
        retrieval::parse(&dialect, &mut parser)
            .map(ast_owner::Owned::new)
            .is_err()
    );
    assert_eq!(
        dialect.take_stop().unwrap_err().code,
        "relational.resource_limit"
    );
    parse(&dialect, &format!("SELECT {} FROM t", surround("ID", 126)));
}

#[test]
fn sibling_case_function_and_in_children_do_not_accumulate_depth() {
    let wide = format!(
        "CASE {} ELSE 0 END,COALESCE({}),1 IN ({}),{}",
        "WHEN TRUE THEN 1 ".repeat(128),
        std::iter::repeat_n("1", 128).collect::<Vec<_>>().join(","),
        std::iter::repeat_n("1", 128).collect::<Vec<_>>().join(","),
        surround("1+1", 125)
    );
    parse(
        &syntax::QueryDialect::default(),
        &format!("SELECT {wide} FROM t"),
    );
}

#[test]
fn like_keeps_escape_as_an_alias_and_preserves_early_from_error() {
    for operator in ["LIKE", "NOT LIKE"] {
        let sql = format!("SELECT 'a' {operator} 'a' ESCAPE FROM t");
        let program = Program::compile(&sql).unwrap();
        assert_eq!(program.projection[0].name, "ESCAPE");
        for suffix in ["'!' FROM t", "(WHERE"] {
            let error = Program::compile(&format!("SELECT 'a' {operator} 'a' ESCAPE {suffix}"))
                .unwrap_err();
            assert_eq!(error.code, "relational.invalid_syntax");
            assert!(error.message.contains("Expected: FROM"), "{error:?}");
        }
    }
}

#[test]
fn finite_infix_continuations_keep_comparison_associativity_and_not_scope() {
    let program = Program::compile(
        "SELECT NOT 1=1 IS NULL,1 NOT IN (1) IS NULL,1 BETWEEN 0 AND 2 = TRUE FROM t",
    )
    .unwrap();
    let Some(Term::Unary(UnaryOperator::Not, operand)) = &program.projection[0].expr else {
        panic!("NOT must contain the entire comparison-level expression");
    };
    assert!(matches!(operand.as_ref(), Term::NullTest(inner, false)
        if matches!(inner.as_ref(), Term::Binary(_, BinaryOperator::Eq, _))));
    assert!(
        matches!(program.projection[1].expr.as_ref(), Some(Term::NullTest(inner, false))
        if matches!(inner.as_ref(), Term::In(_, _, true)))
    );
    assert!(
        matches!(program.projection[2].expr.as_ref(), Some(Term::Binary(left, BinaryOperator::Eq, _))
        if matches!(left.as_ref(), Term::Between(_, _, _, false)))
    );
}
