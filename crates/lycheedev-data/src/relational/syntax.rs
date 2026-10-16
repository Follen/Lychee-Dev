//! Keep Go's function argument grammar without patching sqlparser or rewriting
//! source text. Function semantic checks remain in the real schema binder.
use super::*;
use sqlparser::{
    ast::helpers::attached_token::AttachedToken, dialect::Dialect, keywords::Keyword,
    parser::ParserError, tokenizer::Token,
};

fn reserved(value: &str) -> bool {
    matches!(
        scalar::upper(value).as_str(),
        "SELECT"
            | "WITH"
            | "RECURSIVE"
            | "FROM"
            | "WHERE"
            | "GROUP"
            | "BY"
            | "HAVING"
            | "ORDER"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "ALL"
            | "AS"
            | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "FULL"
            | "CROSS"
            | "OUTER"
            | "ON"
            | "DISTINCT"
            | "ASC"
            | "DESC"
            | "NULLS"
            | "FIRST"
            | "LAST"
            | "AND"
            | "OR"
            | "NOT"
            | "IS"
            | "NULL"
            | "IN"
            | "BETWEEN"
            | "LIKE"
            | "EXISTS"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
            | "CAST"
            | "TRUE"
            | "FALSE"
            | "EXPLAIN"
            | "ANALYZE"
    )
}

#[derive(Debug)]
pub(super) struct QueryDialect;

// Delegate the pinned GenericDialect's feature overrides. This adapter changes
// only expression prefixes; unrelated accepted/rejected ASTs stay unchanged.
macro_rules! generic_flags {
    ($($name:ident),* $(,)?) => {$(
        fn $name(&self) -> bool { GenericDialect {}.$name() }
    )*};
}
impl Dialect for QueryDialect {
    fn dialect(&self) -> std::any::TypeId {
        std::any::TypeId::of::<GenericDialect>()
    }
    fn is_delimited_identifier_start(&self, ch: char) -> bool {
        GenericDialect {}.is_delimited_identifier_start(ch)
    }
    fn is_identifier_start(&self, ch: char) -> bool {
        GenericDialect {}.is_identifier_start(ch)
    }
    fn is_identifier_part(&self, ch: char) -> bool {
        GenericDialect {}.is_identifier_part(ch)
    }
    generic_flags!(
        supports_unicode_string_literal,
        supports_partition_by_after_order_by,
        supports_array_join_syntax,
        supports_group_by_expr,
        supports_group_by_with_modifier,
        supports_left_associative_joins_without_parens,
        supports_connect_by,
        supports_match_recognize,
        supports_pipe_operator,
        supports_start_transaction_modifier,
        supports_window_function_null_treatment_arg,
        supports_dictionary_syntax,
        supports_window_clause_named_window_reference,
        supports_parenthesized_set_variables,
        supports_select_wildcard_except,
        support_map_literal_syntax,
        allow_extract_custom,
        allow_extract_single_quotes,
        supports_extract_comma_syntax,
        supports_create_view_comment_syntax,
        supports_parens_around_table_factor,
        supports_values_as_table_factor,
        supports_create_index_with_clause,
        supports_explain_with_utility_options,
        supports_exclude_constraint,
        supports_limit_comma,
        supports_update_order_by,
        supports_from_first_select,
        supports_projection_trailing_commas,
        supports_asc_desc_in_column_definition,
        supports_try_convert,
        supports_bitwise_shift_operators,
        supports_comment_on,
        supports_load_extension,
        supports_named_fn_args_with_assignment_operator,
        supports_struct_literal,
        supports_empty_projections,
        supports_nested_comments,
        supports_multiline_comment_hints,
        supports_user_host_grantee,
        supports_string_escape_constant,
        supports_array_typedef_with_brackets,
        supports_match_against,
        supports_set_names,
        supports_comma_separated_set_assignments,
        supports_filter_during_aggregation,
        supports_select_wildcard_exclude,
        supports_data_type_signed_suffix,
        supports_interval_options,
        supports_quote_delimited_string,
        supports_select_wildcard_replace,
        supports_select_wildcard_ilike,
        supports_select_wildcard_rename,
        supports_optimize_table,
        supports_install,
        supports_detach,
        supports_prewhere,
        supports_with_fill,
        supports_limit_by,
        supports_interpolate,
        supports_settings,
        supports_select_format,
        supports_comment_optimizer_hint,
        supports_constraint_keyword_without_name,
        supports_key_column_option,
        supports_comma_separated_trim,
        supports_cte_without_as,
        supports_select_item_multi_column_alias,
        supports_xml_expressions,
        supports_aliased_function_args,
    );

    fn parse_prefix(&self, parser: &mut Parser) -> Option<std::result::Result<Expr, ParserError>> {
        if parser.peek_token().token == Token::Colon
            && !matches!(parser.peek_nth_token(1).token, Token::Word(ref word) if word.quote_style.is_none())
        {
            return Some(Err(ParserError::ParserError(
                "Expected named parameter".into(),
            )));
        }
        if parser.peek_token().token == Token::Mul {
            return Some(Ok(Expr::Wildcard(AttachedToken(parser.next_token()))));
        }
        let Token::Word(word) = parser.peek_token().token else {
            return None;
        };
        // These are primary productions in Go, not ordinary function names.
        if word.quote_style.is_none()
            && matches!(
                word.keyword,
                Keyword::NOT
                    | Keyword::EXISTS
                    | Keyword::CAST
                    | Keyword::CASE
                    | Keyword::NULL
                    | Keyword::TRUE
                    | Keyword::FALSE
            )
        {
            return None;
        }
        if word.quote_style.is_none() && reserved(&word.value) {
            return Some(Err(ParserError::ParserError("Expected identifier".into())));
        }
        if parser.peek_nth_token(1).token == Token::Period
            && parser.peek_nth_token(2).token == Token::Mul
        {
            return Some((|| {
                let name = parser.parse_identifier()?;
                parser.expect_token(&Token::Period)?;
                Ok(Expr::QualifiedWildcard(
                    ObjectName::from(vec![name]),
                    AttachedToken(parser.next_token()),
                ))
            })());
        }
        if parser.peek_nth_token(1).token != Token::LParen {
            return None;
        }
        Some((|| {
            let name = ObjectName::from(vec![parser.parse_identifier()?]);
            parser.expect_token(&Token::LParen)?;
            let distinct = parser.parse_keyword(Keyword::DISTINCT);
            let mut args = Vec::new();
            if !parser.consume_token(&Token::RParen) {
                loop {
                    // parse_expr retains nested stars and argument order. The
                    // generic argument helper otherwise consumes '*' early or
                    // rejects DISTINCT with no argument before source binding.
                    args.push(FunctionArg::Unnamed(parser.parse_expr()?.into()));
                    if !parser.consume_token(&Token::Comma) {
                        break;
                    }
                }
                parser.expect_token(&Token::RParen)?;
            }
            Ok(Expr::Function(Function {
                name,
                uses_odbc_syntax: false,
                parameters: FunctionArguments::None,
                args: FunctionArguments::List(FunctionArgumentList {
                    duplicate_treatment: distinct.then_some(DuplicateTreatment::Distinct),
                    args,
                    clauses: Vec::new(),
                }),
                within_group: Vec::new(),
                filter: None,
                null_treatment: None,
                over: None,
            }))
        })())
    }
}
