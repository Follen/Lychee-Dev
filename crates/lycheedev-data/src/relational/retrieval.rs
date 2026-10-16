//! The fixed Go retrieval shell over sqlparser's public tokens, expressions and
//! AST. Keeping this small clause-order adapter avoids parsing a later suffix
//! before a required FROM, and retains each UNION branch's own suffix and CTEs.
use super::{equal_fold, identifier_lower, syntax};
use sqlparser::{
    ast::{helpers::attached_token::AttachedToken, *},
    parser::{Parser, ParserError},
    tokenizer::{Token, TokenWithSpan},
};
use std::collections::BTreeSet;

type ParseResult<T> = std::result::Result<T, ParserError>;

pub(super) fn parse(
    dialect: &syntax::QueryDialect,
    parser: &mut Parser,
) -> ParseResult<Box<Query>> {
    dialect.parser_check()?;
    let _guard = dialect.query_guard()?;
    let with = if let Some(token) = take_word(parser, "WITH") {
        let recursive = take_word(parser, "RECURSIVE").is_some();
        let mut names = BTreeSet::new();
        let mut cte_tables = Vec::new();
        loop {
            dialect.parser_check()?;
            let name = identifier(parser)?;
            // Go diagnoses a repeated name before looking for AS or its body.
            if !names.insert(identifier_lower(&name.value)) {
                return parser.expected("unique common table name", parser.peek_token());
            }
            require_word(parser, "AS")?;
            parser.expect_token(&Token::LParen)?;
            let query = parse(dialect, parser)?;
            let closing_paren_token = AttachedToken(parser.expect_token(&Token::RParen)?);
            cte_tables.push(Cte {
                alias: table_alias(name, false),
                query,
                from: None,
                materialized: None,
                closing_paren_token,
            });
            if !parser.consume_token(&Token::Comma) {
                break;
            }
        }
        Some(With {
            with_token: AttachedToken(token),
            recursive,
            cte_tables,
        })
    } else {
        None
    };

    let select_token = AttachedToken(require_word(parser, "SELECT")?);
    let distinct = take_word(parser, "DISTINCT").map(|_| Distinct::Distinct);
    let mut projection = Vec::new();
    loop {
        dialect.parser_check()?;
        let expr = dialect.expression(parser, 0)?;
        projection.push(match optional_alias(parser)? {
            Some(alias) => SelectItem::ExprWithAlias {
                expr,
                alias: alias.name,
            },
            None => SelectItem::UnnamedExpr(expr),
        });
        if !parser.consume_token(&Token::Comma) {
            break;
        }
    }
    // This check must precede parsing WHERE, GROUP, HAVING or any query suffix.
    require_word(parser, "FROM")?;
    let input = relation(dialect, parser)?;
    let mut joins = Vec::new();
    loop {
        dialect.parser_check()?;
        let kind = if take_word(parser, "JOIN").is_some() {
            0
        } else if take_word(parser, "INNER").is_some() {
            require_word(parser, "JOIN")?;
            0
        } else if take_word(parser, "LEFT").is_some() {
            let _ = take_word(parser, "OUTER");
            require_word(parser, "JOIN")?;
            1
        } else if take_word(parser, "CROSS").is_some() {
            require_word(parser, "JOIN")?;
            2
        } else {
            break;
        };
        let relation = relation(dialect, parser)?;
        let join_operator = if kind == 2 {
            JoinOperator::CrossJoin(JoinConstraint::None)
        } else {
            require_word(parser, "ON")?;
            let constraint = JoinConstraint::On(dialect.expression(parser, 0)?);
            if kind == 1 {
                JoinOperator::LeftOuter(constraint)
            } else {
                JoinOperator::Inner(constraint)
            }
        };
        joins.push(Join {
            relation,
            global: false,
            join_operator,
        });
    }
    let selection = if take_word(parser, "WHERE").is_some() {
        Some(dialect.expression(parser, 0)?)
    } else {
        None
    };
    let mut groups = Vec::new();
    if take_word(parser, "GROUP").is_some() {
        require_word(parser, "BY")?;
        loop {
            dialect.parser_check()?;
            groups.push(dialect.expression(parser, 0)?);
            if !parser.consume_token(&Token::Comma) {
                break;
            }
        }
    }
    let having = if take_word(parser, "HAVING").is_some() {
        Some(dialect.expression(parser, 0)?)
    } else {
        None
    };
    let mut branch = empty_query(SetExpr::Select(Box::new(Select {
        select_token,
        optimizer_hints: Vec::new(),
        distinct,
        select_modifiers: None,
        top: None,
        top_before_distinct: false,
        projection,
        exclude: None,
        into: None,
        from: vec![TableWithJoins {
            relation: input,
            joins,
        }],
        lateral_views: Vec::new(),
        prewhere: None,
        selection,
        connect_by: Vec::new(),
        group_by: GroupByExpr::Expressions(groups, Vec::new()),
        cluster_by: Vec::new(),
        distribute_by: Vec::new(),
        sort_by: Vec::new(),
        having,
        named_window: Vec::new(),
        qualify: None,
        window_before_qualify: false,
        value_table_mode: None,
        flavor: SelectFlavor::Standard,
    })));
    branch.with = with;
    if take_word(parser, "ORDER").is_some() {
        require_word(parser, "BY")?;
        let mut expressions = Vec::new();
        loop {
            dialect.parser_check()?;
            let expr = dialect.expression(parser, 0)?;
            let sort = if take_word(parser, "DESC").is_some() {
                Some(OrderBySort::Desc)
            } else if take_word(parser, "ASC").is_some() {
                Some(OrderBySort::Asc)
            } else {
                None
            };
            let nulls_first = if take_word(parser, "NULLS").is_some() {
                if take_word(parser, "FIRST").is_some() {
                    Some(true)
                } else if take_word(parser, "LAST").is_some() {
                    Some(false)
                } else {
                    return parser.expected("FIRST or LAST", parser.peek_token());
                }
            } else {
                None
            };
            expressions.push(OrderByExpr {
                expr,
                options: OrderByOptions { sort, nulls_first },
                with_fill: None,
            });
            if !parser.consume_token(&Token::Comma) {
                break;
            }
        }
        branch.order_by = Some(OrderBy {
            kind: OrderByKind::Expressions(expressions),
            interpolate: None,
        });
    }
    let limit = if take_word(parser, "LIMIT").is_some() {
        Some(count(parser)?)
    } else {
        None
    };
    let offset = if take_word(parser, "OFFSET").is_some() {
        Some(Offset {
            value: count(parser)?,
            rows: OffsetRows::None,
        })
    } else {
        None
    };
    if limit.is_some() || offset.is_some() {
        branch.limit_clause = Some(LimitClause::LimitOffset {
            limit,
            offset,
            limit_by: Vec::new(),
        });
    }
    if take_word(parser, "UNION").is_some() {
        require_word(parser, "ALL")?;
        let right = parse(dialect, parser)?;
        // A branch suffix is legal at compilation. Compound execution checks
        // a non-final suffix after CTE execution and in branch execution order.
        // Only this branch's WITH belongs to the containing compound scope.
        let with = branch.with.take();
        let mut compound = empty_query(SetExpr::SetOperation {
            op: SetOperator::Union,
            set_quantifier: SetQuantifier::All,
            left: Box::new(SetExpr::Query(Box::new(branch))),
            right: Box::new(SetExpr::Query(right)),
        });
        compound.with = with;
        Ok(Box::new(compound))
    } else {
        Ok(Box::new(branch))
    }
}

fn empty_query(body: SetExpr) -> Query {
    Query {
        with: None,
        body: Box::new(body),
        order_by: None,
        limit_clause: None,
        fetch: None,
        locks: Vec::new(),
        for_clause: None,
        settings: None,
        format_clause: None,
        pipe_operators: Vec::new(),
    }
}

fn take_word(parser: &mut Parser, expected: &str) -> Option<TokenWithSpan> {
    match &parser.peek_token().token {
        Token::Word(word) if word.quote_style.is_none() && equal_fold(&word.value, expected) => {
            Some(parser.next_token())
        }
        _ => None,
    }
}

fn require_word(parser: &mut Parser, expected: &str) -> ParseResult<TokenWithSpan> {
    match take_word(parser, expected) {
        Some(token) => Ok(token),
        None => parser.expected(expected, parser.peek_token()),
    }
}

fn identifier(parser: &mut Parser) -> ParseResult<Ident> {
    let token = parser.peek_token();
    // parse_identifier also accepts string literals; the fixed grammar does not.
    let Token::Word(word) = &token.token else {
        return parser.expected("identifier", token);
    };
    let name = word.clone().into_ident(token.span);
    if syntax::identifier(&name).is_err() {
        return parser.expected("identifier", token);
    }
    parser.next_token();
    Ok(name)
}

fn table_alias(name: Ident, explicit: bool) -> TableAlias {
    TableAlias {
        explicit,
        name,
        columns: Vec::new(),
        at: None,
    }
}

fn optional_alias(parser: &mut Parser) -> ParseResult<Option<TableAlias>> {
    let explicit = take_word(parser, "AS").is_some();
    let implicit = matches!(&parser.peek_token().token, Token::Word(word)
        if word.quote_style.is_some() || !syntax::reserved(&word.value));
    if explicit || implicit {
        Ok(Some(table_alias(identifier(parser)?, explicit)))
    } else {
        Ok(None)
    }
}

fn relation(dialect: &syntax::QueryDialect, parser: &mut Parser) -> ParseResult<TableFactor> {
    dialect.parser_check()?;
    if parser.consume_token(&Token::LParen) {
        let subquery = parse(dialect, parser)?;
        parser.expect_token(&Token::RParen)?;
        Ok(TableFactor::Derived {
            lateral: false,
            subquery,
            alias: optional_alias(parser)?,
            sample: None,
        })
    } else {
        let mut parts = vec![identifier(parser)?];
        if parser.consume_token(&Token::Period) {
            parts.push(identifier(parser)?);
        }
        Ok(TableFactor::Table {
            name: ObjectName::from(parts),
            alias: optional_alias(parser)?,
            args: None,
            with_hints: Vec::new(),
            version: None,
            with_ordinality: false,
            partitions: Vec::new(),
            json_path: None,
            sample: None,
            index_hints: Vec::new(),
        })
    }
}

fn count(parser: &mut Parser) -> ParseResult<Expr> {
    let token = parser.peek_token();
    let value = match &token.token {
        Token::Number(raw, _) => {
            if raw.is_empty() || !raw.bytes().all(|ch| ch.is_ascii_digit()) {
                return parser.expected("integer count", token);
            }
            Value::Number(raw.clone(), false)
        }
        Token::Placeholder(raw) => Value::Placeholder(raw.clone()),
        _ => return parser.expected("nonnegative integer or named parameter", token),
    };
    parser.next_token();
    Ok(Expr::Value(value.with_span(token.span)))
}
