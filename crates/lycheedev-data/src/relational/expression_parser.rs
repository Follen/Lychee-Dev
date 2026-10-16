//! Finite Go expressions over the unchanged public sqlparser token stream/AST.
//!
//! The public Pratt implementation retains several unboxed Expr temporaries in
//! each parse_subexpr/prefix/infix frame. Wrapping that call in a depth guard
//! does not make Go's legal 128 active frames fit an ordinary Windows stack.
//! Query and expression continuations share one heap driver. AST construction
//! is isolated in nonrecursive builders. No depth, deadline or thread is added.
use super::{retrieval, syntax, *};
use sqlparser::{
    ast::helpers::attached_token::AttachedToken, keywords::Keyword, parser::ParserError,
    tokenizer::Token,
};

type ParseResult<T> = std::result::Result<T, ParserError>;
pub(super) type Expression = ast_owner::Owned<Expr>;
pub(super) type QueryOwner = ast_owner::Owned<Query>;

pub(super) struct Call {
    name: ObjectName,
    distinct: bool,
    args: Vec<Expression>,
}
pub(super) struct Case {
    token: AttachedToken,
    operand: Option<Expression>,
    conditions: Vec<(Expression, Expression)>,
    otherwise: Option<Expression>,
}
pub(super) struct InList {
    input: Expression,
    negated: bool,
    values: Vec<Expression>,
}

// Each task stores only handles/small enums. Even nested CASE/function/list
// state is boxed; a query parsed by Primary cannot retain a large task variant
// or any unboxed public Expr on its native caller's stack.
pub(super) enum Task {
    QueryEntry,
    Query(retrieval::Task),
    Subquery(bool),
    InSubquery(Expression, bool),
    Expression(u8),
    EndExpression,
    Primary,
    PrimaryEnd,
    Infix(u8),
    Nested,
    Unary(UnaryOperator),
    Cast,
    CallArg(Box<Call>),
    CaseOperand(Box<Case>),
    CaseCondition(Box<Case>),
    CaseResult(Box<Case>, Expression),
    CaseElse(Box<Case>),
    Binary(Expression, BinaryOperator),
    Like(Expression, bool),
    BetweenLow(Expression, bool),
    BetweenHigh(Expression, Expression, bool),
    InList(Box<InList>),
}

enum Operation {
    Binary(BinaryOperator),
    Is,
    In,
    Between,
    Like,
}

/// The caller owns the root expression's original depth guard. Each scheduled
/// child enters before its first token, and exits before its continuation; this
/// is the Go active-call budget, independent of later logical AST admission.
#[inline(never)]
pub(super) fn parse(
    dialect: &syntax::QueryDialect,
    parser: &mut Parser,
    minimum: u8,
) -> ParseResult<Expression> {
    let (mut value, _) = run(
        dialect,
        parser,
        vec![Task::Infix(minimum), Task::PrimaryEnd, Task::Primary],
    )?;
    take(&mut value)
}

pub(super) fn parse_query(
    dialect: &syntax::QueryDialect,
    parser: &mut Parser,
) -> ParseResult<QueryOwner> {
    let (_, mut query) = run(dialect, parser, vec![Task::Query(retrieval::Task::Start)])?;
    take_query(&mut query)
}

#[inline(never)]
fn run(
    dialect: &syntax::QueryDialect,
    parser: &mut Parser,
    mut tasks: Vec<Task>,
) -> ParseResult<(Option<Expression>, Option<QueryOwner>)> {
    let mut value = None;
    let mut query = None;
    while let Some(task) = tasks.pop() {
        dialect.parser_check()?;
        match task {
            Task::QueryEntry => {
                dialect.enter()?;
                tasks.extend([Task::EndExpression, Task::Query(retrieval::Task::Start)]);
            }
            Task::Query(task) => {
                retrieval::step(dialect, parser, task, &mut tasks, &mut value, &mut query)?
            }
            Task::Subquery(exists) => {
                parser.expect_token(&Token::RParen)?;
                value = Some(build(Node::Subquery(take_query(&mut query)?, exists)));
            }
            Task::InSubquery(input, negated) => {
                parser.expect_token(&Token::RParen)?;
                value = Some(build(Node::InSubquery(
                    input,
                    take_query(&mut query)?,
                    negated,
                )));
            }
            Task::Expression(minimum) => {
                dialect.enter()?;
                tasks.extend([
                    Task::EndExpression,
                    Task::Infix(minimum),
                    Task::PrimaryEnd,
                    Task::Primary,
                ]);
            }
            Task::EndExpression => dialect.leave_expression(),
            Task::Primary => primary(dialect, parser, &mut tasks, &mut value)?,
            Task::PrimaryEnd => {
                // Go primary permits at most qualifier.field or qualifier.*.
                if parser.peek_token().token == Token::Period {
                    return parser.expected("end of scalar expression", parser.peek_token());
                }
            }
            Task::Infix(minimum) => {
                infix(dialect, parser, minimum, &mut tasks, &mut value)?;
            }
            Task::Nested => {
                parser.expect_token(&Token::RParen)?;
                value = Some(build(Node::Nested(take(&mut value)?)));
            }
            Task::Unary(op) => value = Some(build(Node::Unary(op, take(&mut value)?))),
            Task::Cast => {
                let input = take(&mut value)?;
                parser.expect_keyword(Keyword::AS)?;
                let name = parser.parse_identifier()?;
                syntax::identifier(&name).map_err(|e| ParserError::ParserError(e.message))?;
                parser.expect_token(&Token::RParen)?;
                value = Some(build(Node::Cast(input, name)));
            }
            Task::CallArg(mut call) => {
                call.args.push(take(&mut value)?);
                if parser.consume_token(&Token::Comma) {
                    tasks.extend([Task::CallArg(call), Task::Expression(0)]);
                } else {
                    parser.expect_token(&Token::RParen)?;
                    value = Some(build(Node::Call(call)));
                }
            }
            Task::CaseOperand(mut case) => {
                case.operand = Some(take(&mut value)?);
                if !parser.parse_keyword(Keyword::WHEN) {
                    return Err(ParserError::ParserError("CASE requires WHEN".into()));
                }
                tasks.extend([Task::CaseCondition(case), Task::Expression(0)]);
            }
            Task::CaseCondition(case) => {
                let condition = take(&mut value)?;
                parser.expect_keyword(Keyword::THEN)?;
                tasks.extend([Task::CaseResult(case, condition), Task::Expression(0)]);
            }
            Task::CaseResult(mut case, condition) => {
                case.conditions.push((condition, take(&mut value)?));
                if parser.parse_keyword(Keyword::WHEN) {
                    tasks.extend([Task::CaseCondition(case), Task::Expression(0)]);
                } else if parser.parse_keyword(Keyword::ELSE) {
                    tasks.extend([Task::CaseElse(case), Task::Expression(0)]);
                } else {
                    finish_case(parser, case, &mut value)?;
                }
            }
            Task::CaseElse(mut case) => {
                case.otherwise = Some(take(&mut value)?);
                finish_case(parser, case, &mut value)?;
            }
            Task::Binary(left, op) => {
                value = Some(build(Node::Binary(left, op, take(&mut value)?)));
            }
            Task::Like(left, negated) => {
                value = Some(build(Node::Like(left, take(&mut value)?, negated)));
            }
            Task::BetweenLow(left, negated) => {
                let low = take(&mut value)?;
                parser.expect_keyword(Keyword::AND)?;
                tasks.extend([Task::BetweenHigh(left, low, negated), Task::Expression(30)]);
            }
            Task::BetweenHigh(left, low, negated) => {
                value = Some(build(Node::Between(left, low, take(&mut value)?, negated)));
            }
            Task::InList(mut list) => {
                list.values.push(take(&mut value)?);
                if parser.consume_token(&Token::Comma) {
                    tasks.extend([Task::InList(list), Task::Expression(0)]);
                } else {
                    parser.expect_token(&Token::RParen)?;
                    value = Some(build(Node::InList(list)));
                }
            }
        }
    }
    Ok((value, query))
}

pub(super) fn take_query(value: &mut Option<QueryOwner>) -> ParseResult<QueryOwner> {
    value
        .take()
        .ok_or_else(|| ParserError::ParserError("Missing query continuation value".into()))
}

pub(super) fn take(value: &mut Option<Expression>) -> ParseResult<Expression> {
    value
        .take()
        .ok_or_else(|| ParserError::ParserError("Missing expression continuation value".into()))
}

fn finish_case(
    parser: &mut Parser,
    case: Box<Case>,
    value: &mut Option<Expression>,
) -> ParseResult<()> {
    let end = AttachedToken(parser.expect_keyword(Keyword::END)?);
    *value = Some(build(Node::Case(case, end)));
    Ok(())
}

#[inline(never)]
fn primary(
    _dialect: &syntax::QueryDialect,
    parser: &mut Parser,
    tasks: &mut Vec<Task>,
    value: &mut Option<Expression>,
) -> ParseResult<()> {
    if parser.consume_token(&Token::LParen) {
        if parser.peek_keyword(Keyword::SELECT) || parser.peek_keyword(Keyword::WITH) {
            tasks.extend([Task::Subquery(false), Task::QueryEntry]);
        } else {
            tasks.extend([Task::Nested, Task::Expression(0)]);
        }
        return Ok(());
    }
    if parser.parse_keyword(Keyword::EXISTS) {
        parser.expect_token(&Token::LParen)?;
        tasks.extend([Task::Subquery(true), Task::QueryEntry]);
        return Ok(());
    }
    if parser.peek_keyword(Keyword::CASE) {
        let case = Box::new(Case {
            token: AttachedToken(parser.next_token()),
            operand: None,
            conditions: Vec::new(),
            otherwise: None,
        });
        let resume = if parser.parse_keyword(Keyword::WHEN) {
            Task::CaseCondition(case)
        } else {
            Task::CaseOperand(case)
        };
        tasks.extend([resume, Task::Expression(0)]);
        return Ok(());
    }
    if matches!(parser.peek_token().token, Token::Word(ref word)
        if word.quote_style.is_none() && equal_fold(&word.value, "NOT"))
    {
        parser.next_token();
        tasks.extend([Task::Unary(UnaryOperator::Not), Task::Expression(29)]);
        return Ok(());
    }
    if matches!(parser.peek_token().token, Token::Plus | Token::Minus) {
        let op = if parser.next_token().token == Token::Plus {
            UnaryOperator::Plus
        } else {
            UnaryOperator::Minus
        };
        tasks.extend([Task::Unary(op), Task::Expression(60)]);
        return Ok(());
    }
    if matches!(parser.peek_token().token, Token::Word(ref word)
        if word.quote_style.is_none() && equal_fold(&word.value, "CAST"))
    {
        parser.next_token();
        parser.expect_token(&Token::LParen)?;
        tasks.extend([Task::Cast, Task::Expression(0)]);
        return Ok(());
    }
    if matches!(parser.peek_token().token, Token::Word(ref word)
        if (word.quote_style.is_some() || !syntax::reserved(&word.value))
            && parser.peek_nth_token(1).token == Token::LParen)
    {
        let name = ObjectName::from(vec![parser.parse_identifier()?]);
        parser.expect_token(&Token::LParen)?;
        let distinct = matches!(parser.peek_token().token, Token::Word(ref word)
            if word.quote_style.is_none() && equal_fold(&word.value, "DISTINCT"));
        if distinct {
            parser.next_token();
        }
        let call = Box::new(Call {
            name,
            distinct,
            args: Vec::new(),
        });
        if parser.consume_token(&Token::RParen) {
            *value = Some(build(Node::Call(call)));
        } else {
            tasks.extend([Task::CallArg(call), Task::Expression(0)]);
        }
        return Ok(());
    }
    *value = Some(ast_owner::Owned::new(leaf(parser)?));
    Ok(())
}

fn operation(parser: &Parser) -> Option<(Operation, u8, bool)> {
    let negated = parser.peek_keyword(Keyword::NOT);
    let token = if negated {
        parser.peek_nth_token(1)
    } else {
        parser.peek_token()
    };
    let (operation, precedence) = match token.token {
        Token::Word(word) => match word.keyword {
            Keyword::OR => (Operation::Binary(BinaryOperator::Or), 10),
            Keyword::AND => (Operation::Binary(BinaryOperator::And), 20),
            Keyword::IS => (Operation::Is, 30),
            Keyword::IN => (Operation::In, 30),
            Keyword::BETWEEN => (Operation::Between, 30),
            Keyword::LIKE => (Operation::Like, 30),
            _ => return None,
        },
        Token::Eq => (Operation::Binary(BinaryOperator::Eq), 30),
        Token::Neq => (Operation::Binary(BinaryOperator::NotEq), 30),
        Token::Lt => (Operation::Binary(BinaryOperator::Lt), 30),
        Token::LtEq => (Operation::Binary(BinaryOperator::LtEq), 30),
        Token::Gt => (Operation::Binary(BinaryOperator::Gt), 30),
        Token::GtEq => (Operation::Binary(BinaryOperator::GtEq), 30),
        Token::StringConcat => (Operation::Binary(BinaryOperator::StringConcat), 40),
        Token::Plus => (Operation::Binary(BinaryOperator::Plus), 50),
        Token::Minus => (Operation::Binary(BinaryOperator::Minus), 50),
        Token::Mul => (Operation::Binary(BinaryOperator::Multiply), 60),
        Token::Div => (Operation::Binary(BinaryOperator::Divide), 60),
        Token::Mod => (Operation::Binary(BinaryOperator::Modulo), 60),
        _ => return None,
    };
    if negated
        && !matches!(
            operation,
            Operation::In | Operation::Between | Operation::Like
        )
    {
        None
    } else {
        Some((operation, precedence, negated))
    }
}

#[inline(never)]
fn infix(
    _dialect: &syntax::QueryDialect,
    parser: &mut Parser,
    minimum: u8,
    tasks: &mut Vec<Task>,
    value: &mut Option<Expression>,
) -> ParseResult<()> {
    let Some((operation, precedence, negated)) = operation(parser) else {
        return Ok(());
    };
    if precedence <= minimum {
        return Ok(());
    }
    parser.next_token();
    if negated {
        parser.next_token();
    }
    let left = take(value)?;
    tasks.push(Task::Infix(minimum));
    match operation {
        Operation::Is => {
            let negated = parser.parse_keyword(Keyword::NOT);
            parser.expect_keyword(Keyword::NULL)?;
            *value = Some(build(Node::Is(left, negated)));
        }
        Operation::In => {
            parser.expect_token(&Token::LParen)?;
            if parser.peek_keyword(Keyword::SELECT) || parser.peek_keyword(Keyword::WITH) {
                tasks.extend([Task::InSubquery(left, negated), Task::QueryEntry]);
            } else {
                tasks.extend([
                    Task::InList(Box::new(InList {
                        input: left,
                        negated,
                        values: Vec::new(),
                    })),
                    Task::Expression(0),
                ]);
            }
        }
        Operation::Between => {
            tasks.extend([
                Task::BetweenLow(left, negated),
                Task::Expression(precedence),
            ]);
        }
        Operation::Like => {
            // ESCAPE/ANY are ordinary Go identifiers, never extra syntax.
            tasks.extend([Task::Like(left, negated), Task::Expression(precedence)]);
        }
        Operation::Binary(op) => {
            tasks.extend([Task::Binary(left, op), Task::Expression(precedence)]);
        }
    }
    Ok(())
}

// Neither function below enters another expression/query. Keep all unboxed
// public Expr construction and moves here, out of the recursive query path.
#[inline(never)]
fn leaf(parser: &mut Parser) -> ParseResult<Box<Expr>> {
    if parser.peek_token().token == Token::Colon
        && !matches!(parser.peek_nth_token(1).token, Token::Word(ref word) if word.quote_style.is_none())
    {
        return Err(ParserError::ParserError("Expected named parameter".into()));
    }
    if parser.peek_token().token == Token::Mul {
        return Ok(Box::new(Expr::Wildcard(AttachedToken(parser.next_token()))));
    }
    let token = parser.peek_token();
    let scalar = match &token.token {
        Token::Number(value, _) => Some(Value::Number(value.clone(), false)),
        Token::SingleQuotedString(value) => Some(Value::SingleQuotedString(value.clone())),
        Token::Placeholder(value) => Some(Value::Placeholder(value.clone())),
        Token::Word(word) if word.quote_style.is_none() => {
            if equal_fold(&word.value, "NULL") {
                Some(Value::Null)
            } else if equal_fold(&word.value, "TRUE") {
                Some(Value::Boolean(true))
            } else if equal_fold(&word.value, "FALSE") {
                Some(Value::Boolean(false))
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(value) = scalar {
        parser.next_token();
        return Ok(Box::new(Expr::Value(value.with_span(token.span))));
    }
    let Token::Word(word) = token.token else {
        return parser.expected("scalar expression", parser.peek_token());
    };
    if word.quote_style.is_none() && syntax::reserved(&word.value) {
        return Err(ParserError::ParserError("Expected identifier".into()));
    }
    let first = parser.parse_identifier()?;
    syntax::identifier(&first).map_err(|e| ParserError::ParserError(e.message))?;
    if parser.consume_token(&Token::Period) {
        if parser.peek_token().token == Token::Mul {
            return Ok(Box::new(Expr::QualifiedWildcard(
                ObjectName::from(vec![first]),
                AttachedToken(parser.next_token()),
            )));
        }
        let second = parser.parse_identifier()?;
        syntax::identifier(&second).map_err(|e| ParserError::ParserError(e.message))?;
        Ok(Box::new(Expr::CompoundIdentifier(vec![first, second])))
    } else {
        Ok(Box::new(Expr::Identifier(first)))
    }
}

enum Node {
    Nested(Expression),
    Unary(UnaryOperator, Expression),
    Cast(Expression, Ident),
    Call(Box<Call>),
    Case(Box<Case>, AttachedToken),
    Subquery(QueryOwner, bool),
    Binary(Expression, BinaryOperator, Expression),
    Like(Expression, Expression, bool),
    Is(Expression, bool),
    Between(Expression, Expression, Expression, bool),
    InList(Box<InList>),
    InSubquery(Expression, QueryOwner, bool),
}

#[inline(never)]
fn build(node: Node) -> Expression {
    ast_owner::Owned::new(Box::new(match node {
        Node::Nested(input) => Expr::Nested(input.into_box()),
        Node::Unary(op, expr) => Expr::UnaryOp {
            op,
            expr: expr.into_box(),
        },
        Node::Cast(expr, name) => Expr::Cast {
            kind: CastKind::Cast,
            expr: expr.into_box(),
            data_type: DataType::Custom(ObjectName::from(vec![name]), Vec::new()),
            format: None,
        },
        Node::Call(call) => Expr::Function(Function {
            name: call.name,
            uses_odbc_syntax: false,
            parameters: FunctionArguments::None,
            args: FunctionArguments::List(FunctionArgumentList {
                duplicate_treatment: call.distinct.then_some(DuplicateTreatment::Distinct),
                args: call
                    .args
                    .into_iter()
                    .map(|expr| FunctionArg::Unnamed((*expr.into_box()).into()))
                    .collect(),
                clauses: Vec::new(),
            }),
            within_group: Vec::new(),
            filter: None,
            null_treatment: None,
            over: None,
        }),
        Node::Case(case, end_token) => Expr::Case {
            case_token: case.token,
            end_token,
            operand: case.operand.map(Expression::into_box),
            conditions: case
                .conditions
                .into_iter()
                .map(|(condition, result)| CaseWhen {
                    condition: *condition.into_box(),
                    result: *result.into_box(),
                })
                .collect(),
            else_result: case.otherwise.map(Expression::into_box),
        },
        Node::Subquery(subquery, true) => Expr::Exists {
            subquery: subquery.into_box(),
            negated: false,
        },
        Node::Subquery(subquery, false) => Expr::Subquery(subquery.into_box()),
        Node::Binary(left, op, right) => Expr::BinaryOp {
            left: left.into_box(),
            op,
            right: right.into_box(),
        },
        Node::Like(expr, pattern, negated) => Expr::Like {
            negated,
            any: false,
            expr: expr.into_box(),
            pattern: pattern.into_box(),
            escape_char: None,
        },
        Node::Is(expr, true) => Expr::IsNotNull(expr.into_box()),
        Node::Is(expr, false) => Expr::IsNull(expr.into_box()),
        Node::Between(expr, low, high, negated) => Expr::Between {
            expr: expr.into_box(),
            negated,
            low: low.into_box(),
            high: high.into_box(),
        },
        Node::InList(list) => Expr::InList {
            expr: list.input.into_box(),
            list: list
                .values
                .into_iter()
                .map(|expr| *expr.into_box())
                .collect(),
            negated: list.negated,
        },
        Node::InSubquery(expr, subquery, negated) => Expr::InSubquery {
            expr: expr.into_box(),
            subquery: subquery.into_box(),
            negated,
        },
    }))
}
