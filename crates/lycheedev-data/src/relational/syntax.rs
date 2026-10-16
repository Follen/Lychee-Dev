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

/// Owns views of the original controls because sqlparser's Dialect is Any.
/// Cloning a control and taking a safety view preserves their identity, deadline
/// and cancellation provenance; no token or replacement deadline is created.
#[derive(Default)]
pub(super) struct QueryDialect {
    controls: Option<(
        lycheedev_core::control::OperationControl,
        crate::resource::Budget,
    )>,
    stop: std::cell::RefCell<Option<lycheedev_core::ToolkitError>>,
    depth: std::cell::Cell<usize>,
    rhs: std::cell::RefCell<Vec<RhsFrame>>,
}
impl std::fmt::Debug for QueryDialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryDialect")
            .field("controlled", &self.controls.is_some())
            .finish()
    }
}
impl QueryDialect {
    pub(super) fn controlled(
        control: &lycheedev_core::control::OperationControl,
        budget: &crate::resource::Budget,
    ) -> Self {
        Self {
            controls: Some((control.clone(), budget.safety_only())),
            stop: Default::default(),
            depth: Default::default(),
            rhs: Default::default(),
        }
    }
    pub(super) fn check(&self) -> Result<()> {
        if let Some((control, budget)) = &self.controls {
            control.check()?;
            budget.check()?;
        }
        Ok(())
    }
    // sqlparser has a closed error enum. Preserve the actual typed error in
    // this one compilation, rather than inferring cancellation from a string.
    pub(super) fn parser_check(&self) -> std::result::Result<(), ParserError> {
        let mut stop = self.stop.borrow_mut();
        if stop.is_none() {
            *stop = self.check().err();
        }
        if stop.is_some() {
            Err(ParserError::ParserError("SQL compilation stopped".into()))
        } else {
            Ok(())
        }
    }
    fn enter(&self) -> std::result::Result<(), ParserError> {
        self.parser_check()?;
        let depth = self.depth.get() + 1;
        if depth > 128 {
            *self.stop.borrow_mut() = Some(budget());
            return Err(ParserError::RecursionLimitExceeded);
        }
        self.depth.set(depth);
        Ok(())
    }
    pub(super) fn query_guard(&self) -> std::result::Result<FrameGuard<'_>, ParserError> {
        let depth = self.depth.get();
        let rhs = self.rhs.borrow().len();
        self.enter()?;
        Ok(FrameGuard {
            dialect: self,
            depth,
            rhs,
        })
    }
    // Observe the pinned public Pratt loop, without taking or copying its AST.
    // Each fallback infix calls parse_subexpr(precedence). Its final precedence
    // hook sees next <= minimum just before returning to the caller. An owned
    // prefix/query inside that RHS has a greater depth and cannot close it.
    fn begin_rhs(&self, precedence: u8, between: bool) -> std::result::Result<(), ParserError> {
        self.enter()?;
        self.rhs.borrow_mut().push(RhsFrame {
            depth: self.depth.get(),
            precedence,
            between,
            active: true,
        });
        Ok(())
    }
    fn end_rhs(&self, next: u8) {
        let mut frames = self.rhs.borrow_mut();
        let Some(frame) = frames.last_mut() else {
            return;
        };
        if !frame.active || frame.depth != self.depth.get() || next > frame.precedence {
            return;
        }
        self.depth.set(frame.depth - 1);
        if frame.between {
            // parse_between consumes AND, then enters its second RHS. If AND
            // is missing, the enclosing RAII frame clears this pending marker.
            frame.between = false;
            frame.active = false;
        } else {
            frames.pop();
        }
    }
    fn resume_rhs(&self) -> std::result::Result<(), ParserError> {
        let mut frames = self.rhs.borrow_mut();
        if let Some(frame) = frames.last_mut() {
            if !frame.active && self.depth.get() + 1 == frame.depth {
                self.enter()?;
                frame.active = true;
            }
        }
        Ok(())
    }
    /// Expressions retain the dependency's move-based Pratt implementation.
    /// Owned query/prefix/list frames and observed ordinary RHS frames share
    /// Go's active depth; AST admission remains an independent structural check.
    pub(super) fn expression(
        &self,
        parser: &mut Parser,
        precedence: u8,
    ) -> std::result::Result<Expr, ParserError> {
        let _frame = self.query_guard()?;
        parser.parse_subexpr(precedence)
    }
    pub(super) fn take_stop(&self) -> Result<()> {
        match self.stop.borrow_mut().take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

struct RhsFrame {
    depth: usize,
    precedence: u8,
    between: bool,
    active: bool,
}
pub(super) struct FrameGuard<'a> {
    dialect: &'a QueryDialect,
    depth: usize,
    rhs: usize,
}
impl Drop for FrameGuard<'_> {
    fn drop(&mut self) {
        // On an error a native infix can return without its final precedence
        // hook. Remove only frames opened within this owned entry, preserving
        // an outer RHS across nested queries, calls, CASE and parenthesized input.
        self.dialect.rhs.borrow_mut().truncate(self.rhs);
        self.dialect.depth.set(self.depth);
    }
}

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

    fn get_reserved_keywords_for_select_item_operator(&self) -> &[Keyword] {
        // sqlparser's '*' / 't.*' fast paths bypass parse_prefix. This public
        // select-item hook records the same original stop before those paths.
        // The dependency has no fallible hook here; take_stop propagates it
        // before any lowering, binding, provider access or public result.
        let _ = self.parser_check();
        &[]
    }
    fn is_select_item_alias(&self, _explicit: bool, kw: &Keyword, _parser: &mut Parser) -> bool {
        self.parser_check().is_ok() && *kw == Keyword::NoKeyword
    }
    fn is_table_factor_alias(&self, _explicit: bool, kw: &Keyword, _parser: &mut Parser) -> bool {
        self.parser_check().is_ok() && *kw == Keyword::NoKeyword
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
        precedence: u8,
    ) -> Option<std::result::Result<Expr, ParserError>> {
        if let Err(error) = self.parser_check() {
            return Some(Err(error));
        }
        let is_in = parser.peek_keyword(Keyword::IN)
            || parser.peek_keyword(Keyword::NOT)
                && matches!(parser.peek_nth_token(1).token, Token::Word(ref w) if w.keyword == Keyword::IN);
        if is_in {
            return Some((|| {
                let negated = parser.parse_keyword(Keyword::NOT);
                parser.expect_keyword(Keyword::IN)?;
                parser.expect_token(&Token::LParen)?;
                let input = Box::new(expr.clone());
                let result =
                    if parser.peek_keyword(Keyword::SELECT) || parser.peek_keyword(Keyword::WITH) {
                        Expr::InSubquery {
                            expr: input,
                            subquery: retrieval::parse(self, parser)?,
                            negated,
                        }
                    } else {
                        let mut list = Vec::new();
                        loop {
                            list.push(self.expression(parser, 0)?);
                            if !parser.consume_token(&Token::Comma) {
                                break;
                            }
                        }
                        Expr::InList {
                            expr: input,
                            list,
                            negated,
                        }
                    };
                parser.expect_token(&Token::RParen)?;
                Ok(result)
            })());
        }
        if !parser.peek_keyword(Keyword::IS) {
            let between = parser.peek_keyword(Keyword::BETWEEN)
                || parser.peek_keyword(Keyword::NOT)
                    && matches!(parser.peek_nth_token(1).token, Token::Word(ref w) if w.keyword == Keyword::BETWEEN);
            if let Err(error) = self.begin_rhs(precedence, between) {
                return Some(Err(error));
            }
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
        if let Err(error) = self.parser_check() {
            return Some(Err(error));
        }
        let next = match parser.peek_token().token {
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
        };
        self.end_rhs(next);
        Some(Ok(next))
    }
    fn parse_prefix(&self, parser: &mut Parser) -> Option<std::result::Result<Expr, ParserError>> {
        Some(self.primary(parser).and_then(|expr| {
            // Go primary consumes at most qualifier.field (or qualifier.*).
            // The public Pratt loop otherwise consumes more periods before its
            // infix hook, including fields on literals, calls and parentheses.
            // Stop at the same first leftover token, before any later RHS/query.
            if parser.peek_token().token == Token::Period {
                parser.expected("end of scalar expression", parser.peek_token())
            } else {
                Ok(expr)
            }
        }))
    }
}

impl QueryDialect {
    fn primary(&self, parser: &mut Parser) -> std::result::Result<Expr, ParserError> {
        self.resume_rhs()?;
        self.parser_check()?;
        if parser.peek_token().token == Token::LParen {
            return (|| {
                parser.next_token();
                let expr =
                    if parser.peek_keyword(Keyword::SELECT) || parser.peek_keyword(Keyword::WITH) {
                        Expr::Subquery(retrieval::parse(self, parser)?)
                    } else {
                        Expr::Nested(Box::new(self.expression(parser, 0)?))
                    };
                parser.expect_token(&Token::RParen)?;
                Ok(expr)
            })();
        }
        if parser.peek_keyword(Keyword::EXISTS) {
            return (|| {
                parser.next_token();
                parser.expect_token(&Token::LParen)?;
                let subquery = retrieval::parse(self, parser)?;
                parser.expect_token(&Token::RParen)?;
                Ok(Expr::Exists {
                    subquery,
                    negated: false,
                })
            })();
        }
        if parser.peek_keyword(Keyword::CASE) {
            return (|| {
                let case_token = AttachedToken(parser.next_token());
                let operand = if parser.peek_keyword(Keyword::WHEN) {
                    None
                } else {
                    Some(Box::new(self.expression(parser, 0)?))
                };
                let mut conditions = Vec::new();
                while parser.parse_keyword(Keyword::WHEN) {
                    let condition = self.expression(parser, 0)?;
                    parser.expect_keyword(Keyword::THEN)?;
                    let result = self.expression(parser, 0)?;
                    conditions.push(CaseWhen { condition, result });
                }
                if conditions.is_empty() {
                    return Err(ParserError::ParserError("CASE requires WHEN".into()));
                }
                let else_result = if parser.parse_keyword(Keyword::ELSE) {
                    Some(Box::new(self.expression(parser, 0)?))
                } else {
                    None
                };
                let end_token = AttachedToken(parser.expect_keyword(Keyword::END)?);
                Ok(Expr::Case {
                    case_token,
                    end_token,
                    operand,
                    conditions,
                    else_result,
                })
            })();
        }
        if matches!(parser.peek_token().token, Token::Word(ref word)
            if word.quote_style.is_none() && equal_fold(&word.value, "NOT"))
        {
            parser.next_token();
            // Generic's NOT EXISTS shortcut negates before following comparisons.
            // Go NOT always calls expression(3), including the EXISTS case.
            return self.expression(parser, 29).map(|expr| Expr::UnaryOp {
                op: UnaryOperator::Not,
                expr: Box::new(expr),
            });
        }
        if matches!(parser.peek_token().token, Token::Plus | Token::Minus) {
            let op = if parser.next_token().token == Token::Plus {
                UnaryOperator::Plus
            } else {
                UnaryOperator::Minus
            };
            return self.expression(parser, 60).map(|expr| Expr::UnaryOp {
                op,
                expr: Box::new(expr),
            });
        }
        if parser.peek_token().token == Token::Colon
            && !matches!(parser.peek_nth_token(1).token, Token::Word(ref word) if word.quote_style.is_none())
        {
            return Err(ParserError::ParserError("Expected named parameter".into()));
        }
        if parser.peek_token().token == Token::Mul {
            return Ok(Expr::Wildcard(AttachedToken(parser.next_token())));
        }
        let token = parser.peek_token();
        let scalar = match &token.token {
            Token::Number(value, _) => Some(Value::Number(value.clone(), false)),
            Token::SingleQuotedString(value) => Some(Value::SingleQuotedString(value.clone())),
            Token::Placeholder(value) => Some(Value::Placeholder(value.clone())),
            _ => None,
        };
        if let Some(value) = scalar {
            parser.next_token();
            return Ok(Expr::Value(value.with_span(token.span)));
        }
        let Token::Word(word) = token.token else {
            return parser.expected("scalar expression", parser.peek_token());
        };
        if word.quote_style.is_none() && equal_fold(&word.value, "CAST") {
            return (|| {
                parser.next_token();
                parser.expect_token(&Token::LParen)?;
                let expr = Box::new(self.expression(parser, 0)?);
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
            })();
        }
        if word.quote_style.is_none() {
            let value = if equal_fold(&word.value, "NULL") {
                Some(Value::Null)
            } else if equal_fold(&word.value, "TRUE") {
                Some(Value::Boolean(true))
            } else if equal_fold(&word.value, "FALSE") {
                Some(Value::Boolean(false))
            } else {
                None
            };
            if let Some(value) = value {
                parser.next_token();
                return Ok(Expr::Value(value.with_span(token.span)));
            }
        }
        if word.quote_style.is_none() && reserved(&word.value) {
            return Err(ParserError::ParserError("Expected identifier".into()));
        }
        if parser.peek_nth_token(1).token == Token::Period
            && parser.peek_nth_token(2).token == Token::Mul
        {
            return (|| {
                let name = parser.parse_identifier()?;
                parser.expect_token(&Token::Period)?;
                Ok(Expr::QualifiedWildcard(
                    ObjectName::from(vec![name]),
                    AttachedToken(parser.next_token()),
                ))
            })();
        }
        if parser.peek_nth_token(1).token != Token::LParen {
            // CURRENT_DATE, INTERVAL and other sqlparser-specific words are
            // ordinary Go names, rather than special expressions or functions.
            return (|| {
                let first = parser.parse_identifier()?;
                identifier(&first).map_err(|e| ParserError::ParserError(e.message))?;
                if parser.consume_token(&Token::Period) {
                    let second = parser.parse_identifier()?;
                    identifier(&second).map_err(|e| ParserError::ParserError(e.message))?;
                    Ok(Expr::CompoundIdentifier(vec![first, second]))
                } else {
                    Ok(Expr::Identifier(first))
                }
            })();
        }
        (|| {
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
                    args.push(FunctionArg::Unnamed(self.expression(parser, 0)?.into()));
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
        })()
    }
}
