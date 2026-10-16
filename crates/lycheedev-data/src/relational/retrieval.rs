//! Fixed Go retrieval order as continuations on the expression driver's heap
//! stack. WITH, derived/scalar queries and UNION never recurse natively. The
//! grammar, public tokens/AST and deferred schema/execution checks are unchanged.
use super::{ast_owner, equal_fold, expression_parser, identifier_lower, syntax};
use expression_parser::{Expression, QueryOwner, Task as Next, take, take_query};
use sqlparser::{
    ast::{helpers::attached_token::AttachedToken, *},
    parser::{Parser, ParserError},
    tokenizer::{Token, TokenWithSpan},
};
use std::collections::BTreeSet;

type ParseResult<T> = std::result::Result<T, ParserError>;
type Relation = ast_owner::Owned<TableFactor>;
type Order = Vec<(Expression, Option<OrderBySort>, Option<bool>)>;

pub(super) struct WithState {
    token: AttachedToken,
    recursive: bool,
    names: BTreeSet<String>,
    ctes: Vec<(Ident, QueryOwner, AttachedToken)>,
}
pub(super) struct SelectState {
    with: Option<Box<WithState>>,
    token: AttachedToken,
    distinct: Option<Distinct>,
    projection: Vec<(Expression, Option<TableAlias>)>,
    input: Option<Relation>,
    joins: Vec<(Relation, u8, Option<Expression>)>,
    selection: Option<Expression>,
    groups: Vec<Expression>,
    having: Option<Expression>,
}
pub(super) enum Task {
    Start,
    CteName(Box<WithState>),
    CteBody(Box<WithState>, Ident),
    Projection(Box<SelectState>),
    Input(Box<SelectState>, Option<u8>),
    DerivedInput(Box<SelectState>, Option<u8>),
    Joins(Box<SelectState>),
    JoinCondition(Box<SelectState>, Relation, u8),
    Selection(Box<SelectState>),
    GroupItem(Box<SelectState>),
    Having(Box<SelectState>),
    OrderItem(QueryOwner, Order),
    Union(QueryOwner),
}

pub(super) fn parse(
    dialect: &syntax::QueryDialect,
    parser: &mut Parser,
) -> ParseResult<Box<Query>> {
    dialect.parser_check()?;
    let _guard = dialect.query_guard()?;
    expression_parser::parse_query(dialect, parser).map(QueryOwner::into_box)
}

#[inline(never)]
pub(super) fn step(
    dialect: &syntax::QueryDialect,
    parser: &mut Parser,
    task: Task,
    tasks: &mut Vec<Next>,
    value: &mut Option<Expression>,
    query: &mut Option<QueryOwner>,
) -> ParseResult<()> {
    match task {
        Task::Start => {
            if let Some(token) = take_word(parser, "WITH") {
                let recursive = take_word(parser, "RECURSIVE").is_some();
                tasks.push(Next::Query(Task::CteName(Box::new(WithState {
                    token: AttachedToken(token),
                    recursive,
                    names: BTreeSet::new(),
                    ctes: Vec::new(),
                }))));
            } else {
                select_start(parser, None, tasks)?;
            }
        }
        Task::CteName(mut state) => {
            let name = identifier(parser)?;
            if !state.names.insert(identifier_lower(&name.value)) {
                return parser.expected("unique common table name", parser.peek_token());
            }
            require_word(parser, "AS")?;
            parser.expect_token(&Token::LParen)?;
            tasks.extend([Next::Query(Task::CteBody(state, name)), Next::QueryEntry]);
        }
        Task::CteBody(mut state, name) => {
            let child = take_query(query)?;
            let end = AttachedToken(parser.expect_token(&Token::RParen)?);
            state.ctes.push((name, child, end));
            if parser.consume_token(&Token::Comma) {
                tasks.push(Next::Query(Task::CteName(state)));
            } else {
                select_start(parser, Some(state), tasks)?;
            }
        }
        Task::Projection(mut state) => {
            state
                .projection
                .push((take(value)?, optional_alias(parser)?));
            if parser.consume_token(&Token::Comma) {
                tasks.extend([Next::Query(Task::Projection(state)), Next::Expression(0)]);
            } else {
                require_word(parser, "FROM")?;
                tasks.push(Next::Query(Task::Input(state, None)));
            }
        }
        Task::Input(state, kind) => {
            if parser.consume_token(&Token::LParen) {
                tasks.extend([
                    Next::Query(Task::DerivedInput(state, kind)),
                    Next::QueryEntry,
                ]);
            } else {
                let mut parts = vec![identifier(parser)?];
                if parser.consume_token(&Token::Period) {
                    parts.push(identifier(parser)?);
                }
                let relation = named_relation(parts, optional_alias(parser)?);
                input_ready(parser, state, kind, relation, tasks)?;
            }
        }
        Task::DerivedInput(state, kind) => {
            let child = take_query(query)?;
            parser.expect_token(&Token::RParen)?;
            let relation = derived_relation(child, optional_alias(parser)?);
            input_ready(parser, state, kind, relation, tasks)?;
        }
        Task::Joins(state) => {
            let kind = if take_word(parser, "JOIN").is_some() {
                Some(0)
            } else if take_word(parser, "INNER").is_some() {
                require_word(parser, "JOIN")?;
                Some(0)
            } else if take_word(parser, "LEFT").is_some() {
                let _ = take_word(parser, "OUTER");
                require_word(parser, "JOIN")?;
                Some(1)
            } else if take_word(parser, "CROSS").is_some() {
                require_word(parser, "JOIN")?;
                Some(2)
            } else {
                None
            };
            if let Some(kind) = kind {
                tasks.push(Next::Query(Task::Input(state, Some(kind))));
            } else if take_word(parser, "WHERE").is_some() {
                tasks.extend([Next::Query(Task::Selection(state)), Next::Expression(0)]);
            } else {
                group_start(parser, state, tasks, query)?;
            }
        }
        Task::JoinCondition(mut state, relation, kind) => {
            state.joins.push((relation, kind, Some(take(value)?)));
            tasks.push(Next::Query(Task::Joins(state)));
        }
        Task::Selection(mut state) => {
            state.selection = Some(take(value)?);
            group_start(parser, state, tasks, query)?;
        }
        Task::GroupItem(mut state) => {
            state.groups.push(take(value)?);
            if parser.consume_token(&Token::Comma) {
                tasks.extend([Next::Query(Task::GroupItem(state)), Next::Expression(0)]);
            } else {
                having_start(parser, state, tasks, query)?;
            }
        }
        Task::Having(mut state) => {
            state.having = Some(take(value)?);
            suffix_start(parser, select_query(state), tasks, query)?;
        }
        Task::OrderItem(mut branch, mut order) => {
            let expr = take(value)?;
            let sort = if take_word(parser, "DESC").is_some() {
                Some(OrderBySort::Desc)
            } else if take_word(parser, "ASC").is_some() {
                Some(OrderBySort::Asc)
            } else {
                None
            };
            let nulls = if take_word(parser, "NULLS").is_some() {
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
            order.push((expr, sort, nulls));
            if parser.consume_token(&Token::Comma) {
                tasks.extend([
                    Next::Query(Task::OrderItem(branch, order)),
                    Next::Expression(0),
                ]);
            } else {
                branch.order_by = Some(order_by(order));
                tail(parser, branch, tasks, query)?;
            }
        }
        Task::Union(left) => {
            *query = Some(union_query(left, take_query(query)?));
        }
    }
    // The same original controls cover both grammar continuations. In
    // particular, no work after a completed child gets a replacement deadline.
    dialect.parser_check()
}

fn select_start(
    parser: &mut Parser,
    with: Option<Box<WithState>>,
    tasks: &mut Vec<Next>,
) -> ParseResult<()> {
    let token = AttachedToken(require_word(parser, "SELECT")?);
    let distinct = take_word(parser, "DISTINCT").map(|_| Distinct::Distinct);
    tasks.extend([
        Next::Query(Task::Projection(Box::new(SelectState {
            with,
            token,
            distinct,
            projection: Vec::new(),
            input: None,
            joins: Vec::new(),
            selection: None,
            groups: Vec::new(),
            having: None,
        }))),
        Next::Expression(0),
    ]);
    Ok(())
}
fn input_ready(
    parser: &mut Parser,
    mut state: Box<SelectState>,
    kind: Option<u8>,
    relation: Relation,
    tasks: &mut Vec<Next>,
) -> ParseResult<()> {
    match kind {
        None => {
            state.input = Some(relation);
            tasks.push(Next::Query(Task::Joins(state)));
        }
        Some(2) => {
            state.joins.push((relation, 2, None));
            tasks.push(Next::Query(Task::Joins(state)));
        }
        Some(kind) => {
            require_word(parser, "ON")?;
            tasks.extend([
                Next::Query(Task::JoinCondition(state, relation, kind)),
                Next::Expression(0),
            ]);
        }
    }
    Ok(())
}
fn group_start(
    parser: &mut Parser,
    state: Box<SelectState>,
    tasks: &mut Vec<Next>,
    query: &mut Option<QueryOwner>,
) -> ParseResult<()> {
    if take_word(parser, "GROUP").is_some() {
        require_word(parser, "BY")?;
        tasks.extend([Next::Query(Task::GroupItem(state)), Next::Expression(0)]);
        Ok(())
    } else {
        having_start(parser, state, tasks, query)
    }
}
fn having_start(
    parser: &mut Parser,
    state: Box<SelectState>,
    tasks: &mut Vec<Next>,
    query: &mut Option<QueryOwner>,
) -> ParseResult<()> {
    if take_word(parser, "HAVING").is_some() {
        tasks.extend([Next::Query(Task::Having(state)), Next::Expression(0)]);
        Ok(())
    } else {
        suffix_start(parser, select_query(state), tasks, query)
    }
}
fn suffix_start(
    parser: &mut Parser,
    branch: QueryOwner,
    tasks: &mut Vec<Next>,
    query: &mut Option<QueryOwner>,
) -> ParseResult<()> {
    if take_word(parser, "ORDER").is_some() {
        require_word(parser, "BY")?;
        tasks.extend([
            Next::Query(Task::OrderItem(branch, Vec::new())),
            Next::Expression(0),
        ]);
        Ok(())
    } else {
        tail(parser, branch, tasks, query)
    }
}
fn tail(
    parser: &mut Parser,
    mut branch: QueryOwner,
    tasks: &mut Vec<Next>,
    query: &mut Option<QueryOwner>,
) -> ParseResult<()> {
    limit_clause(parser, &mut branch)?;
    if take_word(parser, "UNION").is_some() {
        require_word(parser, "ALL")?;
        tasks.extend([Next::Query(Task::Union(branch)), Next::QueryEntry]);
    } else {
        *query = Some(branch);
    }
    Ok(())
}

// Public enum construction stays outside the task dispatcher, and only takes
// ownership once no fallible parser operation can interrupt this construction.
#[inline(never)]
fn select_query(state: Box<SelectState>) -> QueryOwner {
    let with = state.with.map(|with| With {
        with_token: with.token,
        recursive: with.recursive,
        cte_tables: with
            .ctes
            .into_iter()
            .map(|(name, query, closing_paren_token)| Cte {
                alias: table_alias(name, false),
                query: query.into_box(),
                from: None,
                materialized: None,
                closing_paren_token,
            })
            .collect(),
    });
    let select = Select {
        select_token: state.token,
        optimizer_hints: Vec::new(),
        distinct: state.distinct,
        select_modifiers: None,
        top: None,
        top_before_distinct: false,
        projection: state
            .projection
            .into_iter()
            .map(|(expr, alias)| match alias {
                Some(alias) => SelectItem::ExprWithAlias {
                    expr: *expr.into_box(),
                    alias: alias.name,
                },
                None => SelectItem::UnnamedExpr(*expr.into_box()),
            })
            .collect(),
        exclude: None,
        into: None,
        from: vec![TableWithJoins {
            relation: *state.input.expect("completed FROM").into_box(),
            joins: state
                .joins
                .into_iter()
                .map(|(relation, kind, condition)| {
                    let constraint = condition.map_or(JoinConstraint::None, |expr| {
                        JoinConstraint::On(*expr.into_box())
                    });
                    Join {
                        relation: *relation.into_box(),
                        global: false,
                        join_operator: match kind {
                            0 => JoinOperator::Inner(constraint),
                            1 => JoinOperator::LeftOuter(constraint),
                            _ => JoinOperator::CrossJoin(constraint),
                        },
                    }
                })
                .collect(),
        }],
        lateral_views: Vec::new(),
        prewhere: None,
        selection: state.selection.map(|expr| *expr.into_box()),
        connect_by: Vec::new(),
        group_by: GroupByExpr::Expressions(
            state
                .groups
                .into_iter()
                .map(|expr| *expr.into_box())
                .collect(),
            Vec::new(),
        ),
        cluster_by: Vec::new(),
        distribute_by: Vec::new(),
        sort_by: Vec::new(),
        having: state.having.map(|expr| *expr.into_box()),
        named_window: Vec::new(),
        qualify: None,
        window_before_qualify: false,
        value_table_mode: None,
        flavor: SelectFlavor::Standard,
    };
    let mut query = empty_query(SetExpr::Select(Box::new(select)));
    query.with = with;
    QueryOwner::new(Box::new(query))
}
#[inline(never)]
fn union_query(mut left: QueryOwner, right: QueryOwner) -> QueryOwner {
    let with = left.with.take();
    let mut query = empty_query(SetExpr::SetOperation {
        op: SetOperator::Union,
        set_quantifier: SetQuantifier::All,
        left: Box::new(SetExpr::Query(left.into_box())),
        right: Box::new(SetExpr::Query(right.into_box())),
    });
    query.with = with;
    QueryOwner::new(Box::new(query))
}
#[inline(never)]
fn order_by(order: Order) -> OrderBy {
    OrderBy {
        kind: OrderByKind::Expressions(
            order
                .into_iter()
                .map(|(expr, sort, nulls_first)| OrderByExpr {
                    expr: *expr.into_box(),
                    options: OrderByOptions { sort, nulls_first },
                    with_fill: None,
                })
                .collect(),
        ),
        interpolate: None,
    }
}
#[inline(never)]
fn derived_relation(subquery: QueryOwner, alias: Option<TableAlias>) -> Relation {
    Relation::new(Box::new(TableFactor::Derived {
        lateral: false,
        subquery: subquery.into_box(),
        alias,
        sample: None,
    }))
}
#[inline(never)]
fn named_relation(parts: Vec<Ident>, alias: Option<TableAlias>) -> Relation {
    Relation::new(Box::new(TableFactor::Table {
        name: ObjectName::from(parts),
        alias,
        args: None,
        with_hints: Vec::new(),
        version: None,
        with_ordinality: false,
        partitions: Vec::new(),
        json_path: None,
        sample: None,
        index_hints: Vec::new(),
    }))
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

#[inline(never)]
fn limit_clause(parser: &mut Parser, branch: &mut Query) -> ParseResult<()> {
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
    Ok(())
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
