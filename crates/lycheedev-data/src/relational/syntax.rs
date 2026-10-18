//! Go lexical identity and original compilation controls for the public token
//! stream. Expression continuations live in expression_parser, not native Pratt
//! recursion. Function semantic checks remain in the real schema binder.
use super::*;
use sqlparser::{dialect::Dialect, keywords::Keyword, parser::ParserError};

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
    pub(super) diagnostic: std::cell::RefCell<Option<diagnostic::Diagnostic>>,
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
            diagnostic: Default::default(),
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
    pub(super) fn enter(&self) -> std::result::Result<(), ParserError> {
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
        self.enter()?;
        Ok(FrameGuard {
            dialect: self,
            depth,
        })
    }
    pub(super) fn leave_expression(&self) {
        self.depth.set(self.depth.get() - 1);
    }
    /// Only the finite Go expression grammar is interpreted here. Recursive
    /// expression work is held in bounded heap continuations; native frames
    /// carry boxed ASTs. The same original control/depth guards cover queries.
    pub(super) fn expression(
        &self,
        parser: &mut Parser,
        precedence: u8,
    ) -> std::result::Result<Box<Expr>, ParserError> {
        let _frame = self.query_guard()?;
        expression_parser::parse(self, parser, precedence).map(ast_owner::Owned::into_box)
    }
    pub(super) fn take_stop(&self) -> Result<()> {
        match self.stop.borrow_mut().take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

pub(super) struct FrameGuard<'a> {
    dialect: &'a QueryDialect,
    depth: usize,
}
impl Drop for FrameGuard<'_> {
    fn drop(&mut self) {
        // Also unwinds pending heap continuations on syntax/control errors.
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
    fn get_next_precedence(
        &self,
        _parser: &Parser,
    ) -> Option<std::result::Result<u8, ParserError>> {
        Some(self.parser_check().map(|()| 0))
    }
    fn parse_prefix(&self, parser: &mut Parser) -> Option<std::result::Result<Expr, ParserError>> {
        // Compatibility for public parse_query callers (including AST-only
        // tests): consume the complete finite expression once. Production's
        // retrieval shell calls the boxed driver directly, so no dependency
        // parse_subexpr/parse_prefix/parse_infix frame recurs at legal depth.
        Some(self.expression(parser, 0).map(|expr| *expr))
    }
}
