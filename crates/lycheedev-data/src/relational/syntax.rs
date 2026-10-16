//! Keep Go's function argument grammar without patching sqlparser or rewriting
//! source text. Function semantic checks remain in the real schema binder.
use super::*;
use sqlparser::{
    ast::helpers::attached_token::AttachedToken,
    dialect::{Dialect, Precedence},
    keywords::Keyword,
    parser::ParserError,
    tokenizer::Token,
};

pub(super) fn reserved(value: &str) -> bool {
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

pub(super) fn identifier(name: &Ident) -> Result<&str> {
    if name.value.is_empty()
        || name.value.contains('\0')
        || !matches!(name.quote_style, None | Some('"' | '`' | '['))
        || name.quote_style.is_none() && reserved(&name.value)
    {
        return Err(problem("relational.invalid_syntax", "Expected identifier"));
    }
    Ok(&name.value)
}

#[derive(Debug)]
pub(super) struct QueryDialect;

// Delegate the pinned GenericDialect's feature overrides. This adapter changes
// expression prefixes and identifier boundaries. Tokens retain the fixed Go
// lexical contract before entering the existing parser.
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
        matches!(ch, '"' | '`' | '[')
    }
    fn is_identifier_start(&self, ch: char) -> bool {
        ch == '_' || identifier_categories::letter(ch)
    }
    fn is_identifier_part(&self, ch: char) -> bool {
        self.is_identifier_start(ch) || identifier_categories::digit(ch) || ch == '$'
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

    fn is_select_item_alias(&self, _explicit: bool, kw: &Keyword, _parser: &mut Parser) -> bool {
        *kw == Keyword::NoKeyword
    }
    fn is_table_factor_alias(&self, _explicit: bool, kw: &Keyword, _parser: &mut Parser) -> bool {
        *kw == Keyword::NoKeyword
    }
    fn prec_value(&self, precedence: Precedence) -> u8 {
        match precedence {
            Precedence::Or => 10,
            Precedence::And => 20,
            Precedence::UnaryNot => 29,
            Precedence::Is | Precedence::Between | Precedence::Like | Precedence::Eq => 30,
            Precedence::PlusMinus => 50,
            Precedence::MulDivModOp => 60,
            _ => GenericDialect {}.prec_value(precedence),
        }
    }
    fn parse_infix(
        &self,
        parser: &mut Parser,
        expr: &Expr,
        _precedence: u8,
    ) -> Option<std::result::Result<Expr, ParserError>> {
        if !matches!(parser.peek_token().token, Token::Word(ref w) if w.keyword == Keyword::IS) {
            return None;
        }
        Some((|| {
            parser.next_token();
            let negated = parser.parse_keyword(Keyword::NOT);
            parser.expect_keyword(Keyword::NULL)?;
            let expr = Box::new(expr.clone());
            Ok(if negated {
                Expr::IsNotNull(expr)
            } else {
                Expr::IsNull(expr)
            })
        })())
    }
    fn get_next_precedence(&self, parser: &Parser) -> Option<std::result::Result<u8, ParserError>> {
        Some(Ok(match parser.peek_token().token {
            Token::Word(ref word) => match word.keyword {
                Keyword::OR => 10,
                Keyword::AND => 20,
                Keyword::IS | Keyword::IN | Keyword::BETWEEN | Keyword::LIKE => 30,
                Keyword::NOT
                    if matches!(parser.peek_nth_token(1).token, Token::Word(ref next)
                    if matches!(next.keyword, Keyword::IN | Keyword::BETWEEN | Keyword::LIKE)) =>
                {
                    30
                }
                _ => 0,
            },
            Token::Eq | Token::Neq | Token::Lt | Token::LtEq | Token::Gt | Token::GtEq => 30,
            Token::StringConcat => 40,
            Token::Plus | Token::Minus => 50,
            Token::Mul | Token::Div | Token::Mod => 60,
            _ => 0,
        }))
    }
    fn parse_prefix(&self, parser: &mut Parser) -> Option<std::result::Result<Expr, ParserError>> {
        if matches!(parser.peek_token().token, Token::Word(ref word)
            if word.quote_style.is_none() && equal_fold(&word.value, "NOT"))
        {
            parser.next_token();
            // Generic's NOT EXISTS shortcut negates before following comparisons.
            // Go NOT always calls expression(3), including the EXISTS case.
            return Some(parser.parse_subexpr(29).map(|expr| Expr::UnaryOp {
                op: UnaryOperator::Not,
                expr: Box::new(expr),
            }));
        }
        if matches!(parser.peek_token().token, Token::Plus | Token::Minus) {
            let op = if parser.next_token().token == Token::Plus {
                UnaryOperator::Plus
            } else {
                UnaryOperator::Minus
            };
            return Some(parser.parse_subexpr(60).map(|expr| Expr::UnaryOp {
                op,
                expr: Box::new(expr),
            }));
        }
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
        if word.quote_style.is_none() && equal_fold(&word.value, "CAST") {
            return Some((|| {
                parser.next_token();
                parser.expect_token(&Token::LParen)?;
                let expr = Box::new(parser.parse_expr()?);
                parser.expect_keyword(Keyword::AS)?;
                let name = parser.parse_identifier()?;
                identifier(&name).map_err(|e| ParserError::ParserError(e.message))?;
                parser.expect_token(&Token::RParen)?;
                Ok(Expr::Cast {
                    kind: CastKind::Cast,
                    expr,
                    data_type: DataType::Custom(ObjectName::from(vec![name]), Vec::new()),
                    format: None,
                })
            })());
        }
        // The remaining Go primary productions stay with the existing parser.
        if word.quote_style.is_none()
            && ["EXISTS", "CASE", "NULL", "TRUE", "FALSE"]
                .iter()
                .any(|keyword| equal_fold(&word.value, keyword))
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
            // CURRENT_DATE, INTERVAL and other sqlparser-specific words are
            // ordinary Go names, rather than special expressions or functions.
            return Some((|| {
                let first = parser.parse_identifier()?;
                identifier(&first).map_err(|e| ParserError::ParserError(e.message))?;
                if parser.consume_token(&Token::Period) {
                    let second = parser.parse_identifier()?;
                    identifier(&second).map_err(|e| ParserError::ParserError(e.message))?;
                    Ok(Expr::CompoundIdentifier(vec![first, second]))
                } else {
                    Ok(Expr::Identifier(first))
                }
            })());
        }
        Some((|| {
            let name = ObjectName::from(vec![parser.parse_identifier()?]);
            parser.expect_token(&Token::LParen)?;
            let distinct = matches!(parser.peek_token().token, Token::Word(ref word)
                if word.quote_style.is_none() && equal_fold(&word.value, "DISTINCT"));
            if distinct {
                parser.next_token();
            }
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
