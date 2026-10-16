//! Go323 scan/compile/expressions/dependencies contracts through Program.
use super::*;
use sqlparser::tokenizer::Token;

fn sources(columns: &[&str]) -> BTreeMap<String, Source> {
    BTreeMap::from([(
        "t".into(),
        Source {
            columns: columns.iter().map(|s| (*s).into()).collect(),
            rows: vec![
                (0..columns.len())
                    .map(|i| Scalar::Int(i as i64 + 1))
                    .collect(),
            ],
            complete: true,
        },
    )])
}
fn syntax_error(sql: &str) {
    assert_eq!(
        Program::compile(sql).unwrap_err().code,
        "relational.invalid_syntax",
        "{sql}"
    );
}
fn budget_error(sql: &str) {
    assert_eq!(
        Program::compile(sql).unwrap_err().code,
        "relational.resource_limit",
        "len={}",
        sql.len()
    );
}
#[test]
fn ordinary_sqlparser_keywords_are_real_go_columns() {
    let names = [
        "CURRENT_DATE",
        "CURRENT_TIME",
        "CURRENT_TIMESTAMP",
        "LOCALTIME",
        "LOCALTIMESTAMP",
        "INTERVAL",
        "TOP",
        "QUALIFY",
        "FETCH",
        "TABLE",
        "LATERAL",
        "UNNEST",
        "COLLATE",
    ];
    let sql = format!("SELECT {} FROM t", names.join(","));
    let result = Program::compile(&sql)
        .unwrap()
        .execute(&sources(&names), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(
        result.rows[0],
        (1..=names.len())
            .map(|i| Scalar::Int(i as i64))
            .collect::<Vec<_>>()
    );
}
#[test]
fn ordinary_table_names_and_aliases_do_not_trigger_foreign_grammar() {
    for name in ["TABLE", "LATERAL", "UNNEST", "QUALIFY", "FETCH", "TOP"] {
        let mut data = sources(&["ID"]);
        let source = data.remove("t").unwrap();
        data.insert(name.into(), source);
        let sql = format!("SELECT {name}.ID AS CURRENT_DATE FROM {name} AS {name}");
        let result = Program::compile(&sql)
            .unwrap()
            .execute(&data, &BTreeMap::new(), Limits::default(), None)
            .unwrap();
        assert_eq!(result.rows, vec![vec![Scalar::Int(1)]], "{sql}");
        assert_eq!(result.columns, ["CURRENT_DATE"]);
    }
}
#[test]
fn bracket_identifiers_decode_doubled_closers_through_execution() {
    let data = BTreeMap::from([(
        "t]able".into(),
        Source {
            columns: vec!["a]b".into()],
            rows: vec![vec![Scalar::Int(9)]],
            complete: true,
        },
    )]);
    // Quote the public map key because it contains a SQL delimiter.
    let data = data
        .into_iter()
        .map(|(key, value): (String, Source)| (format!("\"{key}\""), value))
        .collect();
    let result = Program::compile("SELECT [x.y].[a]]b] AS [SELECT] FROM [t]]able] AS [x.y]")
        .unwrap()
        .execute(&data, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.columns, ["SELECT"]);
    assert_eq!(result.rows, vec![vec![Scalar::Int(9)]]);
}
#[test]
fn unicode_letters_and_decimal_digits_use_fixed_go_categories() {
    let names = ["中文", "a٠", "_9$", "\u{10400}", "\u{1c89}"];
    let result = Program::compile(&format!("SELECT {} FROM t", names.join(",")))
        .unwrap()
        .execute(&sources(&names), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows[0].len(), names.len());
    for name in ["@a", "#a", "Ⅰ", "a\u{0345}", "٠a", "$a"] {
        syntax_error(&format!("SELECT {name} FROM t"));
    }
    assert!(Program::compile("SELECT \"Ⅰ\", [a\u{0345}] FROM t").is_ok());
}
#[test]
fn fixed_category_ranges_cover_non_casing_letters_and_only_nd_digits() {
    assert!(identifier_categories::letter('中'));
    assert!(identifier_categories::letter('\u{2ebf0}'));
    assert!(!identifier_categories::letter('Ⅰ'));
    assert!(!identifier_categories::letter('\u{0345}'));
    assert!(identifier_categories::digit('٠'));
    assert!(!identifier_categories::digit('²'));
}
#[test]
fn reserved_and_single_quoted_identifiers_fail_at_every_boundary() {
    for sql in [
        "SELECT ID AS FROM FROM t",
        "SELECT ID FROM t AS WHERE",
        "SELECT ID FROM SELECT",
        "WITH SELECT AS (SELECT ID FROM t) SELECT ID FROM t",
        "SELECT t.SELECT FROM t",
        "SELECT ID FROM static.SELECT",
        "SELECT ID FROM SELECT.t",
        "SELECT ID AS 'alias' FROM t",
        "SELECT ID FROM t AS 'alias'",
        "SELECT ID FROM 't'",
        "WITH 'x' AS (SELECT ID FROM t) SELECT ID FROM x",
        "SELECT CAST(ID AS 'INT') FROM t",
        "SELECT \"\" FROM t",
        "SELECT [] FROM t",
    ] {
        syntax_error(sql);
    }
}
#[test]
fn quoted_reserved_names_remain_legal_through_ctes_and_casts() {
    let program = Program::compile(
        "WITH [SELECT] AS (SELECT ID AS [FROM] FROM t) SELECT CAST([FROM] AS [INT]) FROM [SELECT]",
    )
    .unwrap();
    assert_eq!(
        program
            .execute(&sources(&["ID"]), &BTreeMap::new(), Limits::default(), None)
            .unwrap()
            .rows,
        vec![vec![Scalar::Int(1)]]
    );
}
#[test]
fn keyword_equal_fold_and_infix_simple_upper_are_distinct() {
    let program = Program::compile("ſELECT 1 ıN (1) FROM t").unwrap();
    assert_eq!(
        program
            .execute(&sources(&["ID"]), &BTreeMap::new(), Limits::default(), None)
            .unwrap()
            .rows,
        vec![vec![Scalar::Bool(true)]]
    );
    syntax_error("SELECT ıN FROM t");
    syntax_error("SELECT ID FROM t ıNNER JOIN t ON TRUE");
}
#[test]
fn long_identifiers_and_many_ctes_have_no_invented_small_limit() {
    let name = "a".repeat(513);
    assert!(
        Program::compile(&format!(
            "WITH {name} AS (SELECT ID FROM t) SELECT ID FROM {name}"
        ))
        .is_ok()
    );
    let bindings = (0..129)
        .map(|i| format!("c{i} AS (SELECT ID FROM t)"))
        .collect::<Vec<_>>()
        .join(",");
    assert!(Program::compile(&format!("WITH {bindings} SELECT ID FROM c128")).is_ok());
}
#[test]
fn repeated_joins_are_not_counted_as_distinct_physical_sources() {
    let joins = (1..=33)
        .map(|i| format!(" CROSS JOIN t a{i}"))
        .collect::<String>();
    let program = Program::compile(&format!("SELECT a0.ID FROM t a0{joins}")).unwrap();
    assert_eq!(program.joins.len(), 33);
    assert_eq!(program.source_tables(), ["t"]);
    let empty = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["ID".into()],
            rows: vec![],
            complete: true,
        },
    )]);
    assert!(
        program
            .execute(&empty, &BTreeMap::new(), Limits::default(), None)
            .unwrap()
            .rows
            .is_empty()
    );
}
#[test]
fn cast_has_one_plain_or_quoted_type_name_without_foreign_modifiers() {
    for ty in ["INT", "[INT]", "\"INT\"", "`INT`", "Unsupported"] {
        assert!(
            Program::compile(&format!("SELECT CAST(ID AS {ty}) FROM t")).is_ok(),
            "{ty}"
        );
    }
    for ty in [
        "INT(10)",
        "DOUBLE PRECISION",
        "static.INT",
        "INT[]",
        "'INT'",
    ] {
        syntax_error(&format!("SELECT CAST(ID AS {ty}) FROM t"));
    }
}
#[test]
fn source_byte_limit_includes_explain_and_comments_and_accepts_empty_as_syntax() {
    syntax_error("");
    let prefix = "EXPLAIN ANALYZE SELECT ID FROM t";
    let sql = format!("{prefix}{}", " ".repeat((256 << 10) - prefix.len()));
    assert!(Program::compile(&sql).is_ok());
    budget_error(&(sql + " "));
    let oversized_with_nul = format!("\0{}", " ".repeat(256 << 10));
    budget_error(&oversized_with_nul);
}
#[test]
fn lexer_counts_named_parameters_as_one_and_excludes_eof() {
    let sql = (0..32768).map(|_| ":ALL").collect::<Vec<_>>().join(" ");
    let tokens = lexical::tokens(&sql).unwrap();
    assert_eq!(tokens.len(), 32769);
    assert!(matches!(&tokens[0].token, Token::Placeholder(value) if value == ":ALL"));
    assert_eq!(
        lexical::tokens(&(sql.clone() + " ")).unwrap_err().code,
        "relational.resource_limit"
    );
    assert_eq!(
        lexical::tokens(&(sql + "/*comment*/")).unwrap_err().code,
        "relational.resource_limit"
    );
}
#[test]
fn lexer_parameter_adjacency_and_unicode_names_are_exact() {
    assert!(Program::compile("SELECT :ALL, :UNION, :a٠, :中文 FROM t").is_ok());
    for sql in [
        "SELECT : ALL FROM t",
        "SELECT :/*x*/name FROM t",
        "SELECT :1 FROM t",
        "SELECT :[a] FROM t",
    ] {
        syntax_error(sql);
    }
}
#[test]
fn lexical_comment_depth_and_first_error_precede_normal_parser() {
    let comments = |n| format!("{}x{} SELECT ID FROM t", "/*".repeat(n), "*/".repeat(n));
    assert!(Program::compile(&comments(16)).is_ok());
    budget_error(&comments(17));
    budget_error(&format!("{}", "/*".repeat(17)));
    syntax_error(&format!("@{}", "/*".repeat(17)));
    let error = Program::compile("SELECT FROM t /* unclosed").unwrap_err();
    assert!(error.message.contains("unclosed comment"), "{error:?}");
}
#[test]
fn lexer_preserves_original_unicode_locations_and_quoted_comment_text() {
    let error = Program::compile("SELECT 中文 FROM t\n  @").unwrap_err();
    assert_eq!(
        error.details["location"],
        serde_json::json!({"offset":23,"line":2,"column":3})
    );
    assert!(Program::compile("SELECT '/* /*', [--], [a]]b] FROM t").is_ok());
    syntax_error("SELECT ID FROM t\u{feff}");
    assert!(Program::compile("SELECT\u{0085}ID\u{3000}FROM t").is_ok());
}
#[test]
fn lexer_does_not_merge_foreign_numeric_or_string_prefixes() {
    let program = Program::compile("SELECT 1L,0x10 FROM t").unwrap();
    assert_eq!(program.projection[0].name, "L");
    assert_eq!(program.projection[1].name, "x10");
    assert_eq!(
        program
            .execute(&sources(&["ID"]), &BTreeMap::new(), Limits::default(), None)
            .unwrap()
            .rows,
        vec![vec![Scalar::Int(1), Scalar::Int(0)]]
    );
    for sql in [
        "SELECT 1e FROM t",
        "SELECT E'x' FROM t",
        "SELECT X'00' FROM t",
        "SELECT $$x$$ FROM t",
    ] {
        syntax_error(sql);
    }
}
#[test]
fn exactly_one_optional_final_semicolon_is_allowed() {
    assert!(Program::compile("SELECT ID FROM t; /* after */").is_ok());
    for sql in [
        ";SELECT ID FROM t",
        "SELECT ID FROM t;;",
        "SELECT ID FROM t; SELECT ID FROM t",
    ] {
        syntax_error(sql);
    }
}
#[test]
fn infix_and_unary_precedence_follow_go_instead_of_generic_sql() {
    let program = Program::compile("SELECT 'x'||1+2, -ID*2, 1=1 IS NULL FROM t").unwrap();
    assert!(
        matches!(program.projection[0].expr.as_ref(), Some(Term::Binary(_, BinaryOperator::StringConcat, right)) if matches!(right.as_ref(), Term::Binary(_, BinaryOperator::Plus, _)))
    );
    assert!(
        matches!(program.projection[1].expr.as_ref(), Some(Term::Binary(left, BinaryOperator::Multiply, _)) if matches!(left.as_ref(), Term::Unary(UnaryOperator::Minus, _)))
    );
    assert!(
        matches!(program.projection[2].expr.as_ref(), Some(Term::NullTest(value, false)) if matches!(value.as_ref(), Term::Binary(_, BinaryOperator::Eq, _)))
    );
    for sql in [
        "SELECT ID IS TRUE FROM t",
        "SELECT ID IS NOT FALSE FROM t",
        "SELECT ID IS DISTINCT FROM ID FROM t",
    ] {
        syntax_error(sql);
    }
}
#[test]
fn original_expression_error_is_not_hidden_by_later_union_keyword() {
    let error = Program::compile("SELECT FROM t UNION SELECT ID FROM u").unwrap_err();
    assert_eq!(error.code, "relational.invalid_syntax");
    assert!(!error.message.contains("expected ALL"), "{error:?}");
    let error = Program::compile("SELECT ID FROM t UNION SELECT ID FROM u").unwrap_err();
    assert!(error.message.contains("expected ALL"), "{error:?}");
}
#[test]
fn right_linked_union_depth_includes_each_projection_and_relation() {
    let chain = |n| vec!["SELECT ID FROM t"; n].join(" UNION ALL ");
    assert!(Program::compile(&chain(127)).is_ok());
    budget_error(&chain(128));
}
#[test]
fn parentheses_and_infix_left_chains_have_separate_parser_and_node_depth() {
    let nested = |n| format!("SELECT {}ID{} FROM t", "(".repeat(n), ")".repeat(n));
    assert!(Program::compile(&nested(126)).is_ok());
    budget_error(&nested(127));
    let left = format!("SELECT (ID){} FROM t", "+1".repeat(126));
    assert!(Program::compile(&left).is_ok());
    budget_error(&format!("SELECT ID{} FROM t", "+1".repeat(127)));
}
#[test]
fn nested_scalar_and_derived_queries_are_admitted_at_go_depth() {
    let scalar = |n| {
        let mut sql = "SELECT ID FROM t".to_string();
        for _ in 0..n {
            sql = format!("SELECT ({sql}) FROM t");
        }
        sql
    };
    assert!(Program::compile(&scalar(63)).is_ok());
    budget_error(&scalar(64));
    let derived = |n| {
        let mut sql = "SELECT ID FROM t".to_string();
        for _ in 0..n {
            sql = format!("SELECT ID FROM ({sql}) x");
        }
        sql
    };
    assert!(Program::compile(&derived(63)).is_ok());
    budget_error(&derived(64));
}

#[test]
fn not_exists_uses_the_same_comparison_operand_as_every_not_prefix() {
    let result = Program::compile("SELECT NOT EXISTS(SELECT ID FROM t) IS NULL FROM t")
        .unwrap()
        .execute(&sources(&["ID"]), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Bool(true)]]);
}

#[test]
fn projection_wildcard_fast_path_retains_its_parenthesis_budget_frame() {
    let mut plain = "SELECT * FROM t".to_owned();
    let mut nested = "SELECT (*) FROM t".to_owned();
    for _ in 0..63 {
        plain = format!("SELECT ({plain}) FROM t");
        nested = format!("SELECT ({nested}) FROM t");
    }
    assert!(Program::compile(&plain).is_ok());
    budget_error(&nested);
}
