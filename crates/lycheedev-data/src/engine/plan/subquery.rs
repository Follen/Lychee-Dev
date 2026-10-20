//! Recovery reconstruction from retained excerpts and the original semantic
//! contract. Not byte-identical to the lost helper; no execution qualification.
//! Native relational operators implement membership/existence/cardinality.
use super::super::{QueryOwner, external, numeric, unsupported};
use arrow_schema::DataType;
use datafusion::{
    common::{Column, DFSchema, Result as DfResult, ScalarValue, TableReference, tree_node::{TreeNode, TreeNodeRecursion}},
    dataframe::DataFrame,
    functions_aggregate::expr_fn::{bool_or, count, first_value},
    functions_window::expr_fn::row_number,
    logical_expr::{Aggregate, ColumnarValue, Expr, ExprSchemable, JoinType, LogicalPlan, LogicalPlanBuilder, Operator, Volatility,
        expr_fn::create_udf, expr_rewriter::{replace_col, strip_outer_reference}, lit, scalar_subquery, when},
    optimizer::decorrelate::PullUpCorrelatedExpr,
};
use std::{collections::{BTreeSet, HashMap}, sync::Arc};
fn cardinality() -> datafusion::common::DataFusionError {
    external(crate::fault("relational.invalid_cardinality", 4, "Subquery must return one column and scalar subqueries at most one row"))
}
pub(super) fn reserve_clone(plan: &LogicalPlan, owner: &Arc<QueryOwner>) -> DfResult<()> {
    let mut nodes = 0usize;
    plan.apply_with_subqueries(|node| {
        owner.work(1)?;
        nodes = nodes.checked_add(1).ok_or_else(|| external(super::super::limit()))?;
        node.apply_expressions(|expr| expr.apply(|_| {
            nodes = nodes.checked_add(1).ok_or_else(|| external(super::super::limit()))?;
            Ok(TreeNodeRecursion::Continue)
        }))?;
        Ok(TreeNodeRecursion::Continue)
    })?;
    owner.retain(nodes.checked_mul(2048).ok_or_else(|| external(super::super::limit()))?)
}
fn require_one_column(plan: &LogicalPlan) -> DfResult<Column> {
    if plan.schema().fields().len() != 1 { return Err(cardinality()); }
    Ok(plan.schema().columns().remove(0))
}
pub(super) fn immediate_references(plan: &LogicalPlan, outer: &DFSchema, owner: &Arc<QueryOwner>) -> DfResult<()> {
    for reference in plan.all_out_ref_exprs() {
        owner.work(1)?;
        let Expr::OuterReferenceColumn(field, column) = reference else { return Err(unsupported("Invalid native subquery outer reference")); };
        let index = outer.index_of_column(&column).map_err(|_| unsupported("Subquery references must resolve in the immediate outer query"))?;
        if outer.field(index).data_type() != field.data_type() { return Err(unsupported("Subquery outer reference type changed")); }
        plan.apply(|node| {
            if node.schema().index_of_column(&column).is_ok() { return Err(unsupported("Correlated self-references need distinct scoped relation aliases")); }
            Ok(TreeNodeRecursion::Continue)
        })?;
    }
    Ok(())
}
struct Pulled { plan: LogicalPlan, predicates: Vec<Expr>, inner_columns: BTreeSet<Column> }
fn pull_for_mark(plan: LogicalPlan, outer: &DFSchema, owner: &Arc<QueryOwner>) -> DfResult<Pulled> {
    if plan.all_out_ref_exprs().is_empty() { return Ok(Pulled { plan, predicates: vec![], inner_columns: BTreeSet::new() }); }
    immediate_references(&plan, outer, owner)?;
    plan.apply(|node| {
        owner.work(1)?;
        if let LogicalPlan::Aggregate(aggregate) = node
            && aggregate.group_expr.is_empty() && !node.all_out_ref_exprs().is_empty() {
            return Err(unsupported("Correlated global aggregates need empty-input compensation before membership or existence lowering"));
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    let mut pull = PullUpCorrelatedExpr::new();
    let plan = plan.rewrite(&mut pull)?.data;
    if !pull.can_pull_up || !plan.all_out_ref_exprs().is_empty() { return Err(unsupported("DataFusion cannot safely decorrelate this subquery shape")); }
    let inner_columns = pull.correlated_subquery_cols_map.values().flatten().cloned().collect();
    Ok(Pulled { plan, predicates: pull.join_filters, inner_columns })
}
fn alias_right(pulled: &Pulled, value: Option<&Expr>, alias: &str, owner: &Arc<QueryOwner>) -> DfResult<(LogicalPlan, Option<Expr>, Vec<Expr>)> {
    owner.check()?;
    let columns = pulled.plan.schema().columns();
    let names = (0..columns.len()).map(|index| format!("__sq_value_{index}")).collect::<Vec<_>>();
    let mut replacements = Vec::new();
    let mut inner_columns = pulled.inner_columns.clone();
    if let Some(value) = value { inner_columns.extend(value.column_refs().into_iter().cloned()); }
    for column in &inner_columns {
        if !value.is_some_and(|value| value.column_refs().contains(column)) && !pulled.predicates.iter().any(|predicate| predicate.column_refs().contains(column)) { continue; }
        let index = pulled.plan.schema().index_of_column(column).map_err(|_| unsupported("Correlated subquery column is not exposed by its result"))?;
        replacements.push((column.clone(), Column::new(Some(TableReference::bare(alias)), &names[index])));
    }
    let replacement_map = replacements.iter().map(|(old, new)| (old, new)).collect::<HashMap<_, _>>();
    let value_expression = value.map(|value| replace_col(value.clone(), &replacement_map).map(strip_outer_reference)).transpose()?;
    let predicates = pulled.predicates.iter().cloned().map(|predicate| replace_col(predicate, &replacement_map)).collect::<DfResult<Vec<_>>>()?;
    let mut projection = columns.into_iter().zip(names).map(|(column, name)| Expr::Column(column).alias(name)).collect::<Vec<_>>();
    projection.push(lit(true).alias("__sq_present"));
    reserve_clone(&pulled.plan, owner)?;
    let right = LogicalPlanBuilder::from(pulled.plan.clone()).project_with_validation(projection.into_iter().map(|expr| (expr, false)).collect())?
        .alias(TableReference::bare(alias))?.build()?;
    Ok((right, value_expression, predicates))
}
fn hidden_name(schema: &DFSchema, base: &str) -> String {
    let mut candidate = base.to_owned();
    let mut index = 0usize;
    while schema.fields().iter().any(|f| f.name() == &candidate) { index += 1; candidate = format!("{base}_{index}"); }
    candidate
}
fn validate_outer_expression(value: &Expr, outer: &DFSchema, owner: &Arc<QueryOwner>) -> DfResult<()> {
    value.apply(|value| {
        owner.work(1)?;
        if let Expr::OuterReferenceColumn(field, column) = value {
            let index = outer.index_of_column(column).map_err(|_| unsupported("Projected subquery reference is not immediate"))?;
            if outer.field(index).data_type() != field.data_type() { return Err(unsupported("Projected outer reference type changed")); }
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    Ok(())
}
fn metered_comparison(expression: Expr, owner: &Arc<QueryOwner>) -> Expr {
    let owner = owner.clone();
    create_udf("__lychee_membership_comparison", vec![DataType::Boolean], DataType::Boolean, Volatility::Volatile,
        Arc::new(move |arguments| {
            owner.check()?;
            let [value] = arguments else { return Err(unsupported("Membership meter arity changed")); };
            let rows = match value { ColumnarValue::Array(array) => array.len(), ColumnarValue::Scalar(_) => 1 };
            owner.work(rows.checked_add(1).ok_or_else(|| external(super::super::limit()))?)?;
            Ok(value.clone())
        })).call(vec![expression])
}
fn add_mark(left: DataFrame, right: LogicalPlan, predicates: Vec<Expr>, alias: &str) -> DfResult<(DataFrame, Expr)> {
    let old_columns = left.schema().columns();
    let width = old_columns.len();
    let (state, plan) = left.into_parts();
    let right = DataFrame::new(state.clone(), right);
    let conditions = if predicates.is_empty() { vec![lit(true)] } else { predicates };
    let joined = DataFrame::new(state, plan).join_on(right, JoinType::LeftMark, conditions)?;
    if joined.schema().fields().len() != width + 1 { return Err(unsupported("Native LeftMark schema changed")); }
    let mark = Expr::Column(joined.schema().columns()[width].clone());
    let mut projection = old_columns.into_iter().map(Expr::Column).collect::<Vec<_>>();
    projection.push(mark.alias(alias));
    Ok((joined.select(projection)?, Expr::Column(Column::from_name(alias))))
}
pub(super) fn existence(left: DataFrame, right: DataFrame, negated: bool, alias: &str, owner: &Arc<QueryOwner>) -> DfResult<(DataFrame, Expr)> {
    immediate_references(right.logical_plan(), left.schema(), owner)?;
    if !right.logical_plan().all_out_ref_exprs().is_empty() && guaranteed_one_row(right.logical_plan()) {
        if right.schema().fields().len() != 1 { return Err(unsupported("Correlated aggregate EXISTS currently requires one aggregate output")); }
        let value = scalar(right, owner)?;
        let kind = value.get_type(left.schema())?;
        let owner = owner.clone();
        let result = create_udf("__lychee_aggregate_exists", vec![kind], DataType::Boolean, Volatility::Volatile, Arc::new(move |arguments| {
            owner.check()?;
            let [value] = arguments else { return Err(unsupported("Aggregate EXISTS arity changed")); };
            match value {
                ColumnarValue::Scalar(_) => Ok(ColumnarValue::Scalar(ScalarValue::Boolean(Some(!negated)))),
                ColumnarValue::Array(array) => {
                    owner.work(array.len().checked_add(1).ok_or_else(|| external(super::super::limit()))?)?;
                    owner.retain(array.len().checked_add(1024).ok_or_else(|| external(super::super::limit()))?)?;
                    Ok(ColumnarValue::Array(Arc::new(arrow_array::BooleanArray::from(vec![!negated; array.len()]))))
                }
            }
        })).call(vec![value]);
        return Ok((left, result));
    }
    let pulled = pull_for_mark(right.into_parts().1, left.schema(), owner)?;
    let (right, _, predicates) = alias_right(&pulled, None, alias, owner)?;
    let (left, value) = add_mark(left, right, predicates, alias)?;
    Ok((left, if negated { Expr::Not(Box::new(value)) } else { value }))
}
pub(super) fn membership(left: DataFrame, right: DataFrame, needle: Expr, negated: bool, alias: &str, owner: &Arc<QueryOwner>) -> DfResult<(DataFrame, Expr)> {
    let output = require_one_column(right.logical_plan())?;
    immediate_references(right.logical_plan(), left.schema(), owner)?;
    if !right.logical_plan().all_out_ref_exprs().is_empty() && guaranteed_one_row(right.logical_plan()) {
        let value = scalar(right, owner)?;
        let comparison = metered_comparison(numeric::binary(needle, Operator::Eq, value, left.schema(), owner)?, owner);
        return Ok((left, if negated { Expr::Not(Box::new(comparison)) } else { comparison }));
    }
    let mut value = Expr::Column(output);
    let mut logical = right.into_parts().1;
    if let LogicalPlan::Projection(projection) = &logical {
        let candidate = match &projection.expr[0] { Expr::Alias(alias) => *alias.expr.clone(), value => value.clone() };
        let mut has_outer = false;
        candidate.apply(|value| { if matches!(value, Expr::OuterReferenceColumn(..)) { has_outer = true; } Ok(TreeNodeRecursion::Continue) })?;
        if has_outer {
            validate_outer_expression(&candidate, left.schema(), owner)?;
            value = candidate;
            logical = projection.input.as_ref().clone();
        }
    }
    let pulled = pull_for_mark(logical, left.schema(), owner)?;
    let (right, value, predicates) = alias_right(&pulled, Some(&value), alias, owner)?;
    let value = value.ok_or_else(|| unsupported("Membership value disappeared"))?;
    let original = left.schema().columns();
    let row_id = hidden_name(left.schema(), &format!("{alias}_row"));
    let (state, left) = left.into_parts();
    let numbered = LogicalPlanBuilder::from(left).window(vec![row_number().alias(&row_id)])?.build()?;
    let conditions = if predicates.is_empty() { vec![lit(true)] } else { predicates };
    let joined = DataFrame::new(state.clone(), numbered).join_on(DataFrame::new(state, right), JoinType::Left, conditions)?;
    let comparison = metered_comparison(numeric::binary(needle, Operator::Eq, value, joined.schema(), owner)?, owner);
    let present = Expr::Column(Column::new(Some(TableReference::bare(alias)), "__sq_present")).is_not_null();
    let hit = format!("{alias}_hit");
    let unknown = format!("{alias}_unknown");
    let mut metrics = original.iter().enumerate().map(|(index, column)| first_value(Expr::Column(column.clone())).alias(format!("{alias}_left_{index}"))).collect::<Vec<_>>();
    metrics.push(bool_or(present.clone().and(comparison.clone().is_true())).alias(&hit));
    metrics.push(bool_or(present.and(comparison.is_null())).alias(&unknown));
    let aggregate = joined.aggregate(vec![Expr::Column(Column::from_name(&row_id))], metrics)?;
    let ordered = aggregate.sort(vec![Expr::Column(Column::from_name(&row_id)).sort(true, false)])?;
    let result = when(Expr::Column(Column::from_name(&hit)), lit(true))
        .when(Expr::Column(Column::from_name(&unknown)), Expr::Literal(ScalarValue::Boolean(None), None)).otherwise(lit(false))?;
    let result = if negated { Expr::Not(Box::new(result)) } else { result };
    let mut projection = original.iter().enumerate().map(|(index, column)| {
        Expr::Column(Column::from_name(format!("{alias}_left_{index}"))).alias_qualified(column.relation.clone(), &column.name)
    }).collect::<Vec<_>>();
    projection.push(result.alias(alias));
    Ok((ordered.select(projection)?, Expr::Column(Column::from_name(alias))))
}
fn guaranteed_one_row(plan: &LogicalPlan) -> bool {
    match plan {
        LogicalPlan::Aggregate(aggregate) => aggregate.group_expr.is_empty(),
        LogicalPlan::Projection(projection) => guaranteed_one_row(&projection.input),
        LogicalPlan::SubqueryAlias(alias) => guaranteed_one_row(&alias.input),
        _ => false,
    }
}
fn pullable_scalar(plan: &LogicalPlan, owner: &Arc<QueryOwner>) -> DfResult<()> {
    reserve_clone(plan, owner)?;
    let mut pull = PullUpCorrelatedExpr::new().with_need_handle_count_bug(true);
    let candidate = plan.clone().rewrite(&mut pull)?.data;
    if !pull.can_pull_up || !candidate.all_out_ref_exprs().is_empty() { return Err(unsupported("Correlated scalar subquery requires decorrelatable equality predicates")); }
    Ok(())
}
/// B2 recovery seam: parent owns replay of the later mask/broadcast admission
/// revisions. The signature and zero/one/many/null-mask behavior are retained.
fn scalar_guard_values(arguments: &[ColumnarValue], result_kind: &DataType, owner: &Arc<QueryOwner>) -> DfResult<ColumnarValue> {
    use arrow_array::{Array, BooleanArray, Int64Array};
    owner.check()?;
    let [count, value] = arguments else { return Err(unsupported("Scalar cardinality function arity changed")); };
    let rows = arguments.iter().find_map(|argument| match argument { ColumnarValue::Array(array) => Some(array.len()), ColumnarValue::Scalar(_) => None });
    let length = rows.unwrap_or(1);
    if arguments.iter().any(|argument| matches!(argument, ColumnarValue::Array(array) if array.len() != length)) { return Err(unsupported("Scalar cardinality batch lengths differ")); }
    owner.work(length)?;
    let present = |row: usize| -> DfResult<bool> {
        if row % 64 == 0 { owner.check()?; }
        let count = match count {
            ColumnarValue::Scalar(ScalarValue::Int64(value)) => *value,
            ColumnarValue::Scalar(ScalarValue::Null) => None,
            ColumnarValue::Array(array) if array.data_type() == &DataType::Null => None,
            ColumnarValue::Array(array) => {
                let array = array.as_any().downcast_ref::<Int64Array>().ok_or_else(|| unsupported("Scalar cardinality count type changed"))?;
                if array.is_null(row) { None } else { Some(array.value(row)) }
            }
            _ => return Err(unsupported("Scalar cardinality count type changed")),
        };
        match count { None | Some(0) => Ok(false), Some(1) => Ok(true), Some(_) => Err(cardinality()) }
    };
    let mut selected = 0usize;
    for row in 0..length { if present(row)? { selected += 1; } }
    let variable_bytes = matches!(result_kind, DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View | DataType::Binary | DataType::LargeBinary | DataType::BinaryView);
    let metadata = result_kind.size().checked_mul(8).and_then(|n| n.checked_add(4096)).ok_or_else(|| external(super::super::limit()))?;
    match rows {
        None => {
            let ColumnarValue::Scalar(value) = value else { unreachable!() };
            if selected == 0 && variable_bytes {
                owner.retain(metadata)?;
                return Ok(ColumnarValue::Scalar(ScalarValue::try_new_null(result_kind)?));
            }
            let bytes = value.size().checked_mul(4).and_then(|n| n.checked_add(metadata)).ok_or_else(|| external(super::super::limit()))?;
            owner.retain(bytes)?;
            Ok(ColumnarValue::Scalar(if present(0)? { value.clone() } else { ScalarValue::try_new_null(result_kind)? }))
        }
        Some(length) => {
            if let ColumnarValue::Scalar(value) = value && variable_bytes {
                let slots = length.checked_mul(64).and_then(|n| n.checked_add(metadata)).ok_or_else(|| external(super::super::limit()))?;
                if selected == 0 {
                    owner.retain(slots)?;
                    return Ok(ColumnarValue::Array(arrow_array::new_null_array(result_kind, length)));
                }
                // Pinned Arrow view builder has an 8KiB minimum block. Admit
                // conservatively before constructing even a one-element array.
                let view_builder = if matches!(result_kind, DataType::Utf8View | DataType::BinaryView) { 16usize << 10 } else { 0 };
                let bytes = selected.checked_add(1).and_then(|n| value.size().checked_mul(n)).and_then(|n| n.checked_mul(4))
                    .and_then(|n| n.checked_add(slots)).and_then(|n| n.checked_add(view_builder)).ok_or_else(|| external(super::super::limit()))?;
                owner.retain(bytes)?;
                let one = value.to_array_of_size(1)?;
                let mut indexes = arrow_array::builder::UInt32Builder::with_capacity(length);
                for row in 0..length { if present(row)? { indexes.append_value(0); } else { indexes.append_null(); } }
                return Ok(ColumnarValue::Array(datafusion::arrow::compute::take(one.as_ref(), &indexes.finish(), None)?));
            }
            // Generic/fixed-list values retain the full conservative broadcast
            // admission. Null fixed-list slots still require all child slots.
            let masks = length.checked_mul(2).and_then(|n| n.checked_add(metadata)).ok_or_else(|| external(super::super::limit()))?;
            owner.retain(masks)?;
            let values = match value {
                ColumnarValue::Array(array) => Arc::clone(array),
                ColumnarValue::Scalar(value) => {
                    let bytes = value.size().checked_mul(length).and_then(|n| n.checked_mul(4)).and_then(|n| n.checked_add(metadata)).ok_or_else(|| external(super::super::limit()))?;
                    owner.retain(bytes)?;
                    value.to_array_of_size(length)?
                }
            };
            let mut null_mask = arrow_array::builder::BooleanBuilder::with_capacity(length);
            for row in 0..length { null_mask.append_value(!present(row)?); }
            let null_mask: BooleanArray = null_mask.finish();
            Ok(ColumnarValue::Array(datafusion::arrow::compute::nullif(values.as_ref(), &null_mask)?))
        }
    }
}
fn guard_scalar(count: Expr, value: Expr, kind: DataType, owner: &Arc<QueryOwner>) -> Expr {
    let result_kind = kind.clone();
    let owner = Arc::clone(owner);
    create_udf("__lychee_scalar_cardinality", vec![DataType::Int64, kind], result_kind.clone(), Volatility::Volatile,
        Arc::new(move |arguments| scalar_guard_values(arguments, &result_kind, &owner))).call(vec![count, value])
}

fn native_aggregate(input: LogicalPlan, metrics: Vec<Expr>) -> DfResult<LogicalPlan> {
    Ok(LogicalPlan::Aggregate(Aggregate::try_new(Arc::new(input), vec![], metrics)?))
}
pub(super) fn scalar(right: DataFrame, owner: &Arc<QueryOwner>) -> DfResult<Expr> {
    let value = require_one_column(right.logical_plan())?;
    let kind = right.schema().field(0).data_type().clone();
    let plan = right.into_parts().1;
    let correlated = !plan.all_out_ref_exprs().is_empty();
    if !correlated {
        let aggregate = native_aggregate(plan, vec![count(lit(1i64)).alias("__sq_count"), first_value(Expr::Column(value)).alias("__sq_first")])?;
        let guarded = guard_scalar(Expr::Column(Column::from_name("__sq_count")), Expr::Column(Column::from_name("__sq_first")), kind, owner).alias("__sq_scalar");
        let plan = LogicalPlanBuilder::from(aggregate).project_with_validation(vec![(guarded, false)])?.build()?;
        return Ok(scalar_subquery(Arc::new(plan)));
    }
    let mut aggregates = 0usize;
    plan.apply(|node| { if matches!(node, LogicalPlan::Aggregate(_)) { aggregates += 1; } Ok(TreeNodeRecursion::Continue) })?;
    if aggregates > 0 {
        if aggregates != 1 || !plan.max_rows().is_some_and(|rows| rows <= 1) { return Err(unsupported("Correlated scalar aggregate requires a native at-most-one-row shape")); }
        pullable_scalar(&plan, owner)?;
        return Ok(guard_scalar(lit(1i64), scalar_subquery(Arc::new(plan)), kind, owner));
    }
    reserve_clone(&plan, owner)?;
    let count = native_aggregate(plan.clone(), vec![count(lit(1i64)).alias("__sq_count")])?;
    let first = native_aggregate(plan, vec![first_value(Expr::Column(value)).alias("__sq_first")])?;
    pullable_scalar(&count, owner)?;
    pullable_scalar(&first, owner)?;
    Ok(guard_scalar(scalar_subquery(Arc::new(count)), scalar_subquery(Arc::new(first)), kind, owner))
}

#[cfg(test)]
mod scalar_guard_admission_tests {
    use super::*;
    use arrow_array::{Array,Int64Array,StringArray,ListArray,types::Int64Type};
    fn bounded_owner()->Arc<QueryOwner>{
        let mut limits=crate::resource::Cost::defaults();limits.retained_bytes=64<<10;
        QueryOwner::new(Arc::new(crate::resource::Budget::new(limits,std::time::Instant::now()+std::time::Duration::from_secs(60),None)))
    }
    #[test]
    fn long_array_values_are_null_masked_without_payload_copy(){
        let text="x".repeat(1<<20);let values=Arc::new(StringArray::from(vec![text.as_str(),text.as_str()]));
        let original=values.value_data().as_ptr();let owner=bounded_owner();
        let result=scalar_guard_values(&[ColumnarValue::Array(Arc::new(Int64Array::from(vec![Some(1),None]))),ColumnarValue::Array(values)],&DataType::Utf8,&owner).unwrap();
        let ColumnarValue::Array(result)=result else{panic!("expected array")};let result=result.as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(result.value_data().as_ptr(),original);assert_eq!(result.value(0),text);assert!(result.is_null(1));
        assert!(owner.budget.snapshot().unwrap().retained_bytes<64<<10);
    }
    #[test]
    fn long_scalar_clone_and_broadcast_fail_original_budget_before_copy(){
        for counts in [ColumnarValue::Scalar(ScalarValue::Int64(Some(1))),ColumnarValue::Array(Arc::new(Int64Array::from(vec![1,1])))]{
            let owner=bounded_owner();let error=scalar_guard_values(&[counts,ColumnarValue::Scalar(ScalarValue::Utf8(Some("x".repeat(1<<20))))],&DataType::Utf8,&owner).unwrap_err();
            assert_eq!(crate::engine::error(error).code,"records.query_resource_budget");
        }
    }
    #[test]
    fn list_mask_preserves_native_wide_values_and_nulls(){
        let values=Arc::new(ListArray::from_iter_primitive::<Int64Type,_,_>(vec![Some(vec![Some(i64::MAX),None]),Some(vec![Some(i64::MIN)])]));
        let child=values.values().as_any().downcast_ref::<Int64Array>().unwrap().values().as_ptr();let kind=values.data_type().clone();let owner=bounded_owner();
        let result=scalar_guard_values(&[ColumnarValue::Array(Arc::new(Int64Array::from(vec![1,0]))),ColumnarValue::Array(values)],&kind,&owner).unwrap();
        let ColumnarValue::Array(result)=result else{panic!("expected array")};let result=result.as_any().downcast_ref::<ListArray>().unwrap();
        assert_eq!(result.values().as_any().downcast_ref::<Int64Array>().unwrap().values().as_ptr(),child);
        let first=result.value(0);let first=first.as_any().downcast_ref::<Int64Array>().unwrap();assert_eq!(first.value(0),i64::MAX);assert!(first.is_null(1));assert!(result.is_null(1));
    }
    #[test]
    fn absent_long_byte_scalar_is_not_copied_or_charged_as_selected_payload(){
        for counts in [ColumnarValue::Scalar(ScalarValue::Int64(Some(0))),ColumnarValue::Array(Arc::new(Int64Array::from(vec![0,0])))]{
            let owner=bounded_owner();let result=scalar_guard_values(&[counts,ColumnarValue::Scalar(ScalarValue::Utf8(Some("x".repeat(1<<20))))],&DataType::Utf8,&owner).unwrap();
            match result{ColumnarValue::Scalar(value)=>assert_eq!(value,ScalarValue::Utf8(None)),ColumnarValue::Array(array)=>assert_eq!(array.null_count(),array.len())}
            assert!(owner.budget.snapshot().unwrap().retained_bytes<64<<10);
        }
    }
}
