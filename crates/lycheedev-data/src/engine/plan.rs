//! Task-native plans lower directly to DataFusion. No SQL is synthesized and no
//! external-table/file/function lookup is reachable through this frontend.
use super::{
    QueryOwner, aggregate,
    api::{
        AggregateFunction, BinaryOperator, Expression, JoinKind, QueryPlan, SourceMode,
        TableRequest,
    },
    external, numeric, scalar_functions, unsupported,
};
use datafusion::{
    common::{Column, DFSchema, Result as DfResult, ScalarValue, TableReference},
    dataframe::DataFrame,
    functions_aggregate::expr_fn::{count, max, min},
    logical_expr::{Expr, ExprSchemable, JoinType, Operator},
    prelude::SessionContext,
};
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc};

pub(super) const MAX_DEPTH: usize = 64;
pub(super) const MAX_NODES: usize = 4096;
pub(super) const MAX_VALUES: usize = 16_384;
pub(super) const MAX_TEXT_BYTES: usize = 1 << 20;

/// Internal registration names cannot collide across source modes. Callers use
/// these exact bare names, never SQL identifiers or catalog/schema paths.
pub(super) fn table_name(table: &TableRequest) -> DfResult<String> {
    identifier(&table.name)?;
    if table.hotfix_captures.len() > 16
        || matches!(table.mode, SourceMode::Effective) && table.hotfix_captures.is_empty()
        || matches!(table.mode, SourceMode::Static) && !table.hotfix_captures.is_empty()
    {
        return Err(unsupported(
            "Static sources cannot specify Hotfix captures; effective sources require 1 to 16",
        ));
    }
    let mut captures = BTreeSet::new();
    for capture in &table.hotfix_captures {
        if capture.is_empty()
            || capture.len() > 256
            || capture.chars().any(char::is_control)
            || !captures.insert(capture)
        {
            return Err(unsupported(
                "Hotfix capture IDs must be unique nonempty strings of at most 256 bytes",
            ));
        }
    }
    Ok(format!(
        "__lychee_{}_{}",
        match table.mode {
            SourceMode::Static => "static",
            SourceMode::Effective => "effective",
        },
        table.name
    ))
}
fn identifier(name: &str) -> DfResult<()> {
    let mut bytes = name.bytes();
    if name.len() > 128
        || !bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(unsupported(
            "Table names must be one ASCII identifier of at most 128 bytes",
        ));
    }
    Ok(())
}
fn output_name(name: &str) -> DfResult<()> {
    if name.is_empty()
        || name.len() > 256
        || name.contains('.')
        || name.chars().any(char::is_control)
    {
        return Err(unsupported(
            "Column names must be 1..256 bytes without dots or control characters",
        ));
    }
    Ok(())
}
fn column(name: &str) -> DfResult<Column> {
    if let Some((relation, name)) = name.split_once('.') {
        identifier(relation)?;
        output_name(name)?;
        Ok(Column::new(Some(TableReference::bare(relation)), name))
    } else {
        output_name(name)?;
        Ok(Column::from_name(name))
    }
}
fn scalar(value: &Value) -> DfResult<ScalarValue> {
    Ok(match value {
        Value::Null => ScalarValue::Null,
        Value::Bool(value) => ScalarValue::Boolean(Some(*value)),
        Value::String(value) => ScalarValue::Utf8(Some(value.clone())),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                ScalarValue::Int64(Some(value))
            } else if let Some(value) = value.as_u64() {
                ScalarValue::UInt64(Some(value))
            } else {
                // Large integer lexemes must not quietly become rounded f64.
                let text = value.as_str();
                if !text.contains(['.', 'e', 'E']) {
                    return Err(external(crate::relational::engine_compat::numeric_error()));
                }
                let value = value
                    .as_f64()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| external(crate::relational::engine_compat::numeric_error()))?;
                ScalarValue::Float64(Some(value))
            }
        }
        Value::Array(_) | Value::Object(_) => {
            return Err(unsupported("Plan literals must be JSON scalars"));
        }
    })
}

struct Admission {
    nodes: usize,
    values: usize,
    text: usize,
    expanded: usize,
}
impl Admission {
    fn text(&mut self, bytes: usize) -> DfResult<()> {
        self.text = self
            .text
            .checked_add(bytes)
            .ok_or_else(|| external(super::limit()))?;
        if self.text > MAX_TEXT_BYTES {
            return Err(external(super::limit()));
        }
        Ok(())
    }
    fn values(&mut self, count: usize) -> DfResult<()> {
        self.values = self
            .values
            .checked_add(count)
            .ok_or_else(|| external(super::limit()))?;
        if self.values > MAX_VALUES {
            return Err(external(super::limit()));
        }
        Ok(())
    }
    fn literal(&mut self, value: &Value) -> DfResult<()> {
        self.values(1)?;
        match value {
            Value::String(value) => self.text(value.len())?,
            Value::Number(value) => self.text(value.as_str().len())?,
            Value::Array(_) | Value::Object(_) => {
                return Err(unsupported("Plan literals must be JSON scalars"));
            }
            _ => {}
        }
        // Validate numeric range without retaining strings in the preflight.
        if matches!(value, Value::Number(_)) {
            scalar(value)?;
        }
        Ok(())
    }
    fn name(&mut self, name: &str) -> DfResult<()> {
        output_name(name)?;
        self.text(name.len())
    }
    fn column(&mut self, name: &str) -> DfResult<()> {
        column(name)?;
        self.text(name.len())
    }
}
enum Node<'a> {
    Plan(&'a QueryPlan),
    Expression(&'a Expression),
}

/// Iterative whole-request preflight happens before any recursive lowering,
/// provider lookup or expression cloning. The wire decoder must independently
/// retain its normal JSON depth/byte limit before constructing this value.
fn validate(plan: &QueryPlan, owner: Option<&Arc<QueryOwner>>) -> DfResult<Admission> {
    let mut admitted = Admission {
        nodes: 0,
        values: 0,
        text: 0,
        expanded: 0,
    };
    let mut pending = vec![(Node::Plan(plan), 1)];
    while let Some((node, depth)) = pending.pop() {
        if let Some(owner) = owner {
            owner.check()?;
        }
        admitted.nodes += 1;
        if depth > MAX_DEPTH || admitted.nodes > MAX_NODES {
            return Err(external(super::limit()));
        }
        let nodes = admitted.nodes;
        let mut push = |node| -> DfResult<()> {
            if pending.len() + nodes >= MAX_NODES {
                return Err(external(super::limit()));
            }
            pending.push((node, depth + 1));
            Ok(())
        };
        match node {
            Node::Plan(plan) => match plan {
                QueryPlan::Scan { table } | QueryPlan::Lookup { table, .. } => {
                    table_name(table)?;
                    admitted.text(table.name.len())?;
                    for capture in &table.hotfix_captures {
                        admitted.text(capture.len())?;
                    }
                    if let QueryPlan::Lookup { ids, .. } = plan {
                        admitted.values(ids.len())?;
                    }
                }
                QueryPlan::Filter { input, predicate } => {
                    push(Node::Plan(input))?;
                    push(Node::Expression(predicate))?;
                }
                QueryPlan::Project { input, columns } => {
                    if columns.is_empty() || columns.len() > MAX_NODES {
                        return Err(unsupported("Project needs a bounded, nonempty column list"));
                    }
                    push(Node::Plan(input))?;
                    let mut names = BTreeSet::new();
                    for item in columns {
                        admitted.name(&item.name)?;
                        if !names.insert(&item.name) {
                            return Err(unsupported("Duplicate projection column name"));
                        }
                        push(Node::Expression(&item.expression))?;
                    }
                }
                QueryPlan::Join {
                    left, right, on, ..
                } => {
                    if on.is_empty() || on.len() > MAX_NODES {
                        return Err(unsupported("Join needs a bounded, nonempty key list"));
                    }
                    push(Node::Plan(left))?;
                    push(Node::Plan(right))?;
                    admitted.values(on.len())?;
                    for key in on {
                        admitted.column(&key.left)?;
                        admitted.column(&key.right)?;
                    }
                }
                QueryPlan::Aggregate {
                    input,
                    groups,
                    metrics,
                } => {
                    if groups.is_empty() && metrics.is_empty()
                        || groups.len().saturating_add(metrics.len()) > MAX_NODES
                    {
                        return Err(unsupported(
                            "Aggregate needs a bounded, nonempty group or metric list",
                        ));
                    }
                    push(Node::Plan(input))?;
                    let mut names = BTreeSet::new();
                    for item in groups {
                        admitted.name(&item.name)?;
                        if !names.insert(&item.name) {
                            return Err(unsupported("Duplicate aggregate output name"));
                        }
                        push(Node::Expression(&item.expression))?;
                    }
                    for item in metrics {
                        admitted.name(&item.name)?;
                        if !names.insert(&item.name) {
                            return Err(unsupported("Duplicate aggregate output name"));
                        }
                        if let Some(expression) = &item.expression {
                            push(Node::Expression(expression))?;
                        } else if !matches!(item.function, AggregateFunction::Count) {
                            return Err(unsupported(
                                "Only count accepts an omitted/null expression",
                            ));
                        }
                    }
                }
                QueryPlan::Sort { input, keys } => {
                    if keys.is_empty() || keys.len() > MAX_NODES {
                        return Err(unsupported("Sort needs a bounded, nonempty key list"));
                    }
                    push(Node::Plan(input))?;
                    for key in keys {
                        push(Node::Expression(&key.expression))?;
                    }
                }
                QueryPlan::Limit {
                    input,
                    offset,
                    count,
                } => {
                    let end = offset
                        .checked_add(*count)
                        .ok_or_else(|| external(super::limit()))?;
                    if end > i64::MAX as usize {
                        return Err(external(super::limit()));
                    }
                    push(Node::Plan(input))?;
                }
            },
            Node::Expression(expression) => {
                admitted.expanded = admitted
                    .expanded
                    .checked_add(expression_expansion(expression)?)
                    .ok_or_else(|| external(super::limit()))?;
                if admitted.expanded > 64 << 20 {
                    return Err(external(super::limit()));
                }
                match expression {
                    Expression::Column { name } => admitted.column(name)?,
                    Expression::Literal { value } => admitted.literal(value)?,
                    Expression::Binary { left, right, .. } => {
                        push(Node::Expression(left))?;
                        push(Node::Expression(right))?;
                    }
                    Expression::Not { value }
                    | Expression::IsNull { value }
                    | Expression::Cast { value, .. }
                    | Expression::Index { value, .. } => push(Node::Expression(value))?,
                    Expression::Like { value, pattern, .. } => {
                        push(Node::Expression(value))?;
                        push(Node::Expression(pattern))?;
                    }
                    Expression::Between {
                        value, low, high, ..
                    } => {
                        push(Node::Expression(value))?;
                        push(Node::Expression(low))?;
                        push(Node::Expression(high))?;
                    }
                    Expression::NullIf { left, right } => {
                        push(Node::Expression(left))?;
                        push(Node::Expression(right))?;
                    }
                    Expression::Case {
                        branches,
                        otherwise,
                    } => {
                        if branches.is_empty() || branches.len() > 64 {
                            return Err(unsupported("CASE requires 1..64 branches"));
                        }
                        for branch in branches {
                            push(Node::Expression(&branch.when))?;
                            push(Node::Expression(&branch.then))?;
                        }
                        if let Some(value) = otherwise {
                            push(Node::Expression(value))?;
                        }
                    }
                    Expression::Coalesce { values } => {
                        if values.is_empty() || values.len() > 64 {
                            return Err(unsupported("COALESCE requires 1..64 inputs"));
                        }
                        for value in values {
                            push(Node::Expression(value))?;
                        }
                    }
                    Expression::Call { arguments, .. } => {
                        if arguments.len() > 2 {
                            return Err(unsupported("Scalar function arity exceeds bound"));
                        }
                        for value in arguments {
                            push(Node::Expression(value))?;
                        }
                    }
                    Expression::In { value, values } => {
                        push(Node::Expression(value))?;
                        if values.len() > MAX_VALUES {
                            return Err(external(super::limit()));
                        }
                        for value in values {
                            admitted.literal(value)?;
                        }
                    }
                }
            }
        }
    }
    Ok(admitted)
}

// Account for cloning a computed IN input into pairwise comparisons. Nested
// membership must not turn a small wire request into exponentially many Exprs.
fn expression_expansion(value: &Expression) -> DfResult<usize> {
    let mut pending = vec![(value, 1usize, 1usize)];
    let mut bytes = 0usize;
    let mut nodes = 0usize;
    while let Some((value, copies, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_NODES || depth > MAX_DEPTH {
            return Err(external(super::limit()));
        }
        let extra = match value {
            Expression::Column { name } => name.len(),
            Expression::Literal {
                value: Value::String(value),
            } => value.len(),
            Expression::Literal {
                value: Value::Number(value),
            } => value.as_str().len(),
            _ => 0,
        };
        bytes = bytes
            .checked_add(
                copies
                    .checked_mul(2048usize.saturating_add(extra))
                    .ok_or_else(|| external(super::limit()))?,
            )
            .ok_or_else(|| external(super::limit()))?;
        if bytes > 64 << 20 {
            return Err(external(super::limit()));
        }
        match value {
            Expression::Binary { left, right, .. } => {
                pending.push((left, copies, depth + 1));
                pending.push((right, copies, depth + 1));
            }
            Expression::Not { value }
            | Expression::IsNull { value }
            | Expression::Cast { value, .. }
            | Expression::Index { value, .. } => {
                pending.push((value, copies, depth + 1));
            }
            Expression::Like { value, pattern, .. } => {
                pending.push((value, copies, depth + 1));
                pending.push((pattern, copies, depth + 1));
            }
            Expression::Between {
                value, low, high, ..
            } => {
                pending.push((value, copies, depth + 1));
                pending.push((low, copies, depth + 1));
                pending.push((high, copies, depth + 1));
            }
            Expression::NullIf { left, right } => {
                pending.push((
                    left,
                    copies
                        .checked_mul(2)
                        .ok_or_else(|| external(super::limit()))?,
                    depth + 1,
                ));
                pending.push((right, copies, depth + 1));
            }
            Expression::Case {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    pending.push((&branch.when, copies, depth + 1));
                    pending.push((&branch.then, copies, depth + 1));
                }
                if let Some(value) = otherwise {
                    pending.push((value, copies, depth + 1));
                }
            }
            Expression::Coalesce { values } => {
                let copies = copies
                    .checked_mul(2)
                    .ok_or_else(|| external(super::limit()))?;
                for value in values {
                    pending.push((value, copies, depth + 1));
                }
            }
            Expression::Call {
                function,
                arguments,
            } => {
                for (index, value) in arguments.iter().enumerate() {
                    let copies = if index == 0
                        && matches!(
                            function,
                            super::api::ScalarFunction::HasFlag
                                | super::api::ScalarFunction::BitAnd
                        ) {
                        copies
                            .checked_mul(2)
                            .ok_or_else(|| external(super::limit()))?
                    } else {
                        copies
                    };
                    pending.push((value, copies, depth + 1));
                }
            }
            Expression::In { value, values } => {
                let copies = copies
                    .checked_mul(values.len().max(1))
                    .ok_or_else(|| external(super::limit()))?;
                pending.push((value, copies, depth + 1));
            }
            _ => {}
        }
    }
    Ok(bytes)
}

pub(super) async fn lower(
    context: &SessionContext,
    plan: &QueryPlan,
    owner: Arc<QueryOwner>,
) -> DfResult<DataFrame> {
    let admitted = validate(plan, Some(&owner))?;
    tables(plan)?;
    // Retain conservative logical-plan, expression, identifier and literal
    // ownership for the query lifetime; native execution has its own pool.
    let bytes = admitted
        .nodes
        .checked_mul(2048)
        .and_then(|n| {
            admitted
                .values
                .checked_mul(256)
                .and_then(|v| n.checked_add(v))
        })
        .and_then(|n| admitted.text.checked_mul(4).and_then(|v| n.checked_add(v)))
        .and_then(|n| n.checked_add(admitted.expanded))
        .ok_or_else(|| external(super::limit()))?;
    owner.retain(bytes)?;
    lower_validated(context, plan, &owner).await
}
/// Returns source requests only after validating the entire typed task, before
/// callers acquire providers or perform source I/O. Capture order is meaningful.
pub(super) fn tables(plan: &QueryPlan) -> DfResult<Vec<TableRequest>> {
    validate(plan, None)?;
    let mut tables: Vec<TableRequest> = Vec::new();
    let mut pending = vec![plan];
    while let Some(plan) = pending.pop() {
        match plan {
            QueryPlan::Scan { table } | QueryPlan::Lookup { table, .. } => {
                if let Some(previous) = tables
                    .iter()
                    .find(|previous| previous.name == table.name && previous.mode == table.mode)
                {
                    if previous != table {
                        return Err(unsupported(
                            "One query cannot bind different capture histories to the same table and mode",
                        ));
                    }
                } else {
                    tables.push(table.clone());
                }
            }
            QueryPlan::Filter { input, .. }
            | QueryPlan::Project { input, .. }
            | QueryPlan::Aggregate { input, .. }
            | QueryPlan::Sort { input, .. }
            | QueryPlan::Limit { input, .. } => pending.push(input),
            QueryPlan::Join { left, right, .. } => {
                pending.push(right);
                pending.push(left);
            }
        }
    }
    Ok(tables)
}

fn lower_validated<'a>(
    context: &'a SessionContext,
    plan: &'a QueryPlan,
    owner: &'a Arc<QueryOwner>,
) -> BoxFuture<'a, DfResult<DataFrame>> {
    Box::pin(async move {
        owner.work(1)?;
        match plan {
            QueryPlan::Scan { table } | QueryPlan::Lookup { table, .. } => {
                let frame = context
                    .table(TableReference::bare(table_name(table)?))
                    .await?
                    .alias(&format!("\"{}\"", table.name))?;
                if let QueryPlan::Lookup { ids, .. } = plan {
                    let id = Expr::Column(Column::from_name("ID"));
                    // Lookup is membership, so repeated IDs do not duplicate rows.
                    frame.filter(
                        id.in_list(
                            ids.iter()
                                .map(|id| {
                                    Expr::Literal(ScalarValue::UInt64(Some(u64::from(*id))), None)
                                })
                                .collect(),
                            false,
                        ),
                    )
                } else {
                    Ok(frame)
                }
            }
            QueryPlan::Filter { input, predicate } => {
                let frame = lower_validated(context, input, owner).await?;
                let predicate = expression(predicate, frame.schema(), owner)?;
                frame.filter(predicate)
            }
            QueryPlan::Project { input, columns } => {
                let frame = lower_validated(context, input, owner).await?;
                let columns = columns
                    .iter()
                    .map(|item| {
                        Ok(expression(&item.expression, frame.schema(), owner)?.alias(&item.name))
                    })
                    .collect::<DfResult<Vec<_>>>()?;
                frame.select(columns)
            }
            QueryPlan::Join {
                left,
                right,
                kind,
                on,
            } => {
                let left = lower_validated(context, left, owner).await?;
                let right = lower_validated(context, right, owner).await?;
                // Resolve each key on its own side before rebinding. Fixed side
                // aliases permit self-joins and make duplicate names explicit.
                let names = on
                    .iter()
                    .map(|key| {
                        let l = left.schema().index_of_column(&column(&key.left)?)?;
                        let r = right.schema().index_of_column(&column(&key.right)?)?;
                        Ok((
                            left.schema().field(l).name().clone(),
                            right.schema().field(r).name().clone(),
                        ))
                    })
                    .collect::<DfResult<Vec<_>>>()?;
                let left = left.alias("left")?;
                let right = right.alias("right")?;
                let schema = left.schema().join(right.schema())?;
                let predicates = names
                    .into_iter()
                    .map(|(l, r)| {
                        numeric::binary(
                            Expr::Column(Column::new(Some(TableReference::bare("left")), l)),
                            Operator::Eq,
                            Expr::Column(Column::new(Some(TableReference::bare("right")), r)),
                            &schema,
                            owner,
                        )
                    })
                    .collect::<DfResult<Vec<_>>>()?;
                let kind = match kind {
                    JoinKind::Inner => JoinType::Inner,
                    JoinKind::Left => JoinType::Left,
                    JoinKind::Full => JoinType::Full,
                    JoinKind::Semi => JoinType::LeftSemi,
                    JoinKind::Anti => JoinType::LeftAnti,
                };
                left.join_on(right, kind, predicates)
            }
            QueryPlan::Aggregate {
                input,
                groups,
                metrics,
            } => {
                let frame = lower_validated(context, input, owner).await?;
                let groups = groups
                    .iter()
                    .map(|item| {
                        Ok(expression(&item.expression, frame.schema(), owner)?.alias(&item.name))
                    })
                    .collect::<DfResult<Vec<_>>>()?;
                let exact = aggregate::functions(owner);
                let metrics = metrics
                    .iter()
                    .map(|item| {
                        let argument = item
                            .expression
                            .as_ref()
                            .map(|expr| expression(expr, frame.schema(), owner))
                            .transpose()?;
                        let result = match (item.function, argument) {
                            (AggregateFunction::Count, Some(argument)) => count(argument),
                            (AggregateFunction::Count, None) => {
                                count(Expr::Literal(ScalarValue::Int64(Some(1)), None))
                            }
                            (AggregateFunction::Sum, Some(argument)) => {
                                exact[0].call(vec![argument])
                            }
                            (AggregateFunction::Average, Some(argument)) => {
                                exact[1].call(vec![argument])
                            }
                            (AggregateFunction::Minimum, Some(argument)) => min(argument),
                            (AggregateFunction::Maximum, Some(argument)) => max(argument),
                            _ => return Err(unsupported("Missing aggregate expression")),
                        };
                        Ok(result.alias(&item.name))
                    })
                    .collect::<DfResult<Vec<_>>>()?;
                frame.aggregate(groups, metrics)
            }
            QueryPlan::Sort { input, keys } => {
                let frame = lower_validated(context, input, owner).await?;
                let keys = keys
                    .iter()
                    .map(|key| {
                        Ok(expression(&key.expression, frame.schema(), owner)?
                            .sort(!key.descending, key.nulls_first))
                    })
                    .collect::<DfResult<Vec<_>>>()?;
                frame.sort(keys)
            }
            QueryPlan::Limit {
                input,
                offset,
                count,
            } => lower_validated(context, input, owner)
                .await?
                .limit(*offset, Some(*count)),
        }
    })
}
fn expression(value: &Expression, schema: &DFSchema, owner: &Arc<QueryOwner>) -> DfResult<Expr> {
    owner.work(1)?;
    Ok(match value {
        Expression::Column { name } => Expr::Column(column(name)?),
        Expression::Literal { value } => Expr::Literal(scalar(value)?, None),
        Expression::Binary {
            left,
            operator,
            right,
        } => numeric::binary(
            expression(left, schema, owner)?,
            operator_value(*operator),
            expression(right, schema, owner)?,
            schema,
            owner,
        )?,
        Expression::Not { value } => Expr::Not(Box::new(expression(value, schema, owner)?)),
        Expression::IsNull { value } => expression(value, schema, owner)?.is_null(),
        Expression::Cast { value, target } => {
            scalar_functions::cast(expression(value, schema, owner)?, *target, schema, owner)?
        }
        Expression::Call {
            function,
            arguments,
        } => scalar_functions::call(
            *function,
            arguments
                .iter()
                .map(|v| expression(v, schema, owner))
                .collect::<DfResult<Vec<_>>>()?,
            schema,
            owner,
        )?,
        Expression::Index { value, index } => {
            scalar_functions::index(expression(value, schema, owner)?, *index, schema, owner)?
        }
        Expression::Like {
            value,
            pattern,
            negated,
        } => scalar_functions::like(
            expression(value, schema, owner)?,
            expression(pattern, schema, owner)?,
            *negated,
            schema,
            owner,
        )?,
        Expression::Between {
            value,
            low,
            high,
            negated,
        } => scalar_functions::between(
            expression(value, schema, owner)?,
            expression(low, schema, owner)?,
            expression(high, schema, owner)?,
            *negated,
            schema,
            owner,
        )?,
        Expression::Case {
            branches,
            otherwise,
        } => {
            let branches = branches
                .iter()
                .map(|b| {
                    Ok((
                        expression(&b.when, schema, owner)?,
                        expression(&b.then, schema, owner)?,
                    ))
                })
                .collect::<DfResult<Vec<_>>>()?;
            let otherwise = otherwise
                .as_deref()
                .map(|v| expression(v, schema, owner))
                .transpose()?;
            exact_case(branches, otherwise, schema)?
        }
        Expression::Coalesce { values } => {
            let branches = values
                .iter()
                .map(|v| {
                    let value = expression(v, schema, owner)?;
                    Ok((value.clone().is_not_null(), value))
                })
                .collect::<DfResult<Vec<_>>>()?;
            exact_case(branches, None, schema)?
        }
        Expression::NullIf { left, right } => {
            let left = expression(left, schema, owner)?;
            let condition = numeric::binary(
                left.clone(),
                Operator::Eq,
                expression(right, schema, owner)?,
                schema,
                owner,
            )?;
            exact_case(
                vec![(condition, Expr::Literal(ScalarValue::Null, None))],
                Some(left),
                schema,
            )?
        }
        Expression::In { value, values } => {
            let value = expression(value, schema, owner)?;
            // Native IN coerces the whole list to one type, losing UInt64 bits
            // for mixed floats. A balanced OR preserves exact pair comparisons
            // and bounds optimizer recursion even for large membership lists.
            let mut comparisons = values
                .iter()
                .map(|item| {
                    numeric::binary(
                        value.clone(),
                        Operator::Eq,
                        Expr::Literal(scalar(item)?, None),
                        schema,
                        owner,
                    )
                })
                .collect::<DfResult<Vec<_>>>()?;
            if comparisons.is_empty() {
                numeric::binary(
                    value.is_null(),
                    Operator::And,
                    Expr::Literal(ScalarValue::Boolean(Some(false)), None),
                    schema,
                    owner,
                )?
            } else {
                while comparisons.len() > 1 {
                    let mut next = Vec::with_capacity(comparisons.len().div_ceil(2));
                    let mut iter = comparisons.into_iter();
                    while let Some(left) = iter.next() {
                        next.push(if let Some(right) = iter.next() {
                            numeric::binary(left, Operator::Or, right, schema, owner)?
                        } else {
                            left
                        });
                    }
                    comparisons = next;
                }
                comparisons
                    .pop()
                    .ok_or_else(|| unsupported("Empty IN reduction"))?
            }
        }
    })
}
fn operator_value(value: BinaryOperator) -> Operator {
    match value {
        BinaryOperator::Add => Operator::Plus,
        BinaryOperator::Subtract => Operator::Minus,
        BinaryOperator::Multiply => Operator::Multiply,
        BinaryOperator::Divide => Operator::Divide,
        BinaryOperator::Modulo => Operator::Modulo,
        BinaryOperator::Equal => Operator::Eq,
        BinaryOperator::NotEqual => Operator::NotEq,
        BinaryOperator::Less => Operator::Lt,
        BinaryOperator::LessEqual => Operator::LtEq,
        BinaryOperator::Greater => Operator::Gt,
        BinaryOperator::GreaterEqual => Operator::GtEq,
        BinaryOperator::And => Operator::And,
        BinaryOperator::Or => Operator::Or,
    }
}

#[cfg(test)]
mod tests;

/// Select a representation that cannot silently round a branch. Heterogeneous
/// integer/float or scalar-kind outputs need an explicit variant representation
/// before they can be exposed; the native coercion fallback is forbidden.
fn exact_case(
    mut branches: Vec<(Expr, Expr)>,
    mut otherwise: Option<Expr>,
    schema: &DFSchema,
) -> DfResult<Expr> {
    use arrow_schema::DataType;
    use datafusion::logical_expr::expr::{Case, Cast};
    let mut kinds = Vec::new();
    for (condition, value) in &branches {
        if !matches!(
            condition.get_type(schema)?,
            DataType::Boolean | DataType::Null
        ) {
            return Err(external(crate::relational::engine_compat::type_error()));
        }
        let kind = value.get_type(schema)?;
        if kind != DataType::Null {
            kinds.push(kind);
        }
    }
    if let Some(value) = &otherwise {
        let kind = value.get_type(schema)?;
        if kind != DataType::Null {
            kinds.push(kind);
        }
    }
    let kind = if kinds.is_empty() {
        DataType::Null
    } else if kinds.iter().all(|v| v == &kinds[0]) {
        kinds[0].clone()
    } else if kinds.iter().all(super::values::integer) {
        DataType::Decimal128(20, 0)
    } else if kinds
        .iter()
        .all(|v| matches!(v, DataType::Float32 | DataType::Float64))
    {
        DataType::Float64
    } else {
        return Err(unsupported(
            "Conditional branches currently require one exact common type; mixed integer/float or heterogeneous variants are not admitted",
        ));
    };
    let normalize = |value: Expr| -> DfResult<Expr> {
        if value.get_type(schema)? == kind {
            Ok(value)
        } else {
            Ok(Expr::Cast(Cast::new(Box::new(value), kind.clone())))
        }
    };
    for (_, value) in &mut branches {
        *value = normalize(value.clone())?;
    }
    if let Some(value) = otherwise.take() {
        otherwise = Some(normalize(value)?);
    }
    Ok(Expr::Case(Case::new(
        None,
        branches
            .into_iter()
            .map(|(a, b)| (Box::new(a), Box::new(b)))
            .collect(),
        otherwise.map(Box::new),
    )))
}

/// One provider per exact table identity. Multiple lookups share the union of
/// their requested IDs, while each plan leaf retains its own membership filter.
/// Any full scan dominates selective requirements for that table.
pub(super) struct SourceRequirement {
    pub(super) table: TableRequest,
    pub(super) ids: Option<Vec<u32>>,
}
pub(super) fn requirements(
    plan: &QueryPlan,
    owner: &Arc<QueryOwner>,
) -> DfResult<Vec<SourceRequirement>> {
    let admission = validate(plan, Some(owner))?;
    let bytes = admission
        .nodes
        .checked_mul(1024)
        .and_then(|n| n.checked_add(admission.values.checked_mul(16)?))
        .and_then(|n| n.checked_add(admission.text.checked_mul(4)?))
        .and_then(|n| n.checked_add(4096))
        .ok_or_else(|| external(super::limit()))?;
    owner.retain(bytes)?;
    let mut output: Vec<SourceRequirement> = Vec::new();
    let mut pending = vec![plan];
    while let Some(plan) = pending.pop() {
        owner.work(output.len().saturating_add(1))?;
        match plan {
            QueryPlan::Scan { table } | QueryPlan::Lookup { table, .. } => {
                if output.iter().any(|entry| {
                    entry.table.name == table.name
                        && entry.table.mode == table.mode
                        && entry.table != *table
                }) {
                    return Err(unsupported(
                        "One query cannot bind different capture histories to the same table and mode",
                    ));
                }
                let ids = match plan {
                    QueryPlan::Lookup { ids, .. } => Some(ids.as_slice()),
                    _ => None,
                };
                if let Some(existing) = output.iter_mut().find(|entry| entry.table == *table) {
                    match (&mut existing.ids, ids) {
                        (_, None) => existing.ids = None,
                        (Some(current), Some(ids)) => current.extend_from_slice(ids),
                        (None, Some(_)) => {}
                    }
                } else {
                    output.push(SourceRequirement {
                        table: table.clone(),
                        ids: ids.map(<[u32]>::to_vec),
                    });
                }
            }
            QueryPlan::Filter { input, .. }
            | QueryPlan::Project { input, .. }
            | QueryPlan::Aggregate { input, .. }
            | QueryPlan::Sort { input, .. }
            | QueryPlan::Limit { input, .. } => pending.push(input),
            QueryPlan::Join { left, right, .. } => {
                pending.push(right);
                pending.push(left);
            }
        }
    }
    for entry in &mut output {
        if let Some(ids) = &mut entry.ids {
            owner.work(ids.len().saturating_mul(16).saturating_add(1))?;
            ids.sort_unstable();
            ids.dedup();
        }
    }
    Ok(output)
}
