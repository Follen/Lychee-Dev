//! Recovered from retained source excerpts, not asserted byte-identical to the
//! lost sealed artifact. Native DataFusion owns all relational iteration.
use super::{children, column, external, unsupported, Node, QueryOwner, QueryPlan};
use crate::engine::{api::Expression, output_batches::OutputBatches, values};
use arrow_schema::{DataType, SchemaRef};
use datafusion::{
    catalog::{Session, TableProvider},
    common::{Column, DFSchema, Result, TableReference, tree_node::{Transformed, TransformedResult}},
    dataframe::DataFrame,
    datasource::{MemTable, cte_worktable::CteWorkTable, empty::EmptyTable, provider_as_source},
    logical_expr::{Expr, ExprSchemable, LogicalPlan, LogicalPlanBuilder, TableType, expr::Cast},
    physical_plan::{ExecutionPlan, ExecutionPlanProperties, union::UnionExec},
    prelude::SessionContext,
};
use futures_util::StreamExt;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone)]
pub(super) struct Scope {
    visible: DFSchema,
    physical: DFSchema,
    parent: Option<Arc<Scope>>,
}
impl Scope {
    pub(super) fn local(&self, name: &str) -> Result<Expr> {
        let index = self.visible.index_of_column(&column(name)?)?;
        Ok(Expr::Column(self.physical.columns()[index].clone()))
    }
    pub(super) fn outer(&self, name: &str, depth: usize) -> Result<Expr> {
        if depth == 0 { return Err(unsupported("OuterColumn depth starts at one")); }
        if depth > 1 {
            return self.parent.as_ref().ok_or_else(|| unsupported("OuterColumn depth exceeds its lexical query scope"))?.outer(name, depth - 1);
        }
        let index = self.visible.index_of_column(&column(name)?)?;
        Ok(Expr::OuterReferenceColumn(self.physical.fields()[index].clone(), self.physical.columns()[index].clone()))
    }
}
pub(super) fn restore(frame: DataFrame, scope: &Scope) -> Result<DataFrame> {
    let projection = scope.visible.iter().enumerate().map(|(index, (relation, field))| {
        Expr::Column(scope.physical.columns()[index].clone()).alias_qualified(relation.cloned(), field.name())
    }).collect::<Vec<_>>();
    frame.select(projection)
}
pub(super) struct DeltaBinding { binding: String, plan: LogicalPlan }
pub(super) struct Lowering<'a> {
    pub(super) context: &'a SessionContext,
    pub(super) owner: Arc<QueryOwner>,
    pub(super) delta: Option<DeltaBinding>,
    next: usize,
    pending: Vec<Materialization>,
}
impl<'a> Lowering<'a> {
    pub(super) fn new(context: &'a SessionContext, owner: Arc<QueryOwner>) -> Self {
        Self { context, owner, delta: None, next: 0, pending: vec![] }
    }
    pub(super) fn name(&mut self, purpose: &str) -> String {
        let id = self.next;
        self.next += 1;
        format!("__lychee_{purpose}_{id}")
    }
    pub(super) fn scope(&mut self, frame: DataFrame, parent: Option<&Scope>) -> Result<(DataFrame, Scope)> {
        self.owner.retain(frame.schema().fields().len().checked_mul(1024).ok_or_else(|| external(crate::engine::limit()))?)?;
        let visible = frame.schema().clone();
        // Qualified projections avoid fixed SubqueryAlias schemas hiding keys
        // which native decorrelation appends beneath a projection.
        let relation = self.name("scope");
        let projection = frame.schema().columns().into_iter().enumerate().map(|(index, column)| {
            Expr::Column(column).alias_qualified(Some(TableReference::bare(relation.clone())), format!("{relation}_slot_{index}"))
        }).collect::<Vec<_>>();
        let frame = frame.select(projection)?;
        let physical = frame.schema().clone();
        Ok((frame, Scope { visible, physical, parent: parent.cloned().map(Arc::new) }))
    }
    pub(super) fn begin_delta(&self, binding: &str, schema: &DFSchema) -> Result<DeltaBinding> {
        let name = format!("__lychee_delta_{binding}");
        let provider = Arc::new(CteWorkTable::new(&name, Arc::new(schema.as_arrow().clone())));
        let plan = LogicalPlanBuilder::scan(binding, provider_as_source(provider), None)?.build()?;
        Ok(DeltaBinding { binding: binding.to_owned(), plan })
    }
    pub(super) fn delta_frame(&self, binding: &str) -> Result<DataFrame> {
        let delta = self.delta.as_ref().filter(|v| v.binding == binding)
            .ok_or_else(|| unsupported("Delta is only valid inside its matching Iterate step"))?;
        Ok(DataFrame::new(self.context.state(), delta.plan.clone()))
    }
    pub(super) fn iterate(&mut self, binding: &str, seed: DataFrame, step: DataFrame) -> Result<DataFrame> {
        if !seed.logical_plan().all_out_ref_exprs().is_empty() || !step.logical_plan().all_out_ref_exprs().is_empty() {
            return Err(unsupported("Correlated Iterate cannot be independently completed by the native recursive kernel"));
        }
        let kinds = seed.schema().fields().iter().map(|f| f.data_type().clone()).collect::<Vec<_>>();
        let names = seed.schema().fields().iter().map(|f| f.name().clone()).collect::<Vec<_>>();
        let step = normalize(step, &kinds, &names)?;
        let native = LogicalPlanBuilder::from(seed.into_parts().1)
            .to_recursive_query(format!("__lychee_delta_{binding}"), step.into_parts().1, false)?.build()?;
        self.materialize(native, MaterializationKind::Recursive)
    }
    pub(super) fn union(&mut self, frames: Vec<DataFrame>) -> Result<DataFrame> {
        let (frames, kinds, names) = normalize_union(frames)?;
        let inputs = frames.iter().map(|f| f.logical_plan().clone()).collect::<Vec<_>>();
        let mut frames = frames.into_iter();
        let mut union = frames.next().ok_or_else(|| unsupported("UnionAll requires inputs"))?;
        for frame in frames { union = union.union(frame)?; }
        let union = normalize(union, &kinds, &names)?;
        if self.delta.is_some() || !union.logical_plan().all_out_ref_exprs().is_empty() {
            return Err(unsupported("Ordered UnionAll inside a recursive or correlated step requires a native ordered-apply kernel"));
        }
        self.materialize(union.into_parts().1, MaterializationKind::OrderedUnion(inputs))
    }
    fn materialize(&mut self, logical: LogicalPlan, kind: MaterializationKind) -> Result<DataFrame> {
        self.owner.work(1)?;
        let name = self.name("completed");
        let schema = Arc::new(logical.schema().as_arrow().clone());
        let placeholder = LogicalPlanBuilder::scan(name.clone(), provider_as_source(Arc::new(EmptyTable::new(schema))), None)?.build()?;
        self.pending.push(Materialization { name, logical, kind });
        Ok(DataFrame::new(self.context.state(), placeholder))
    }
    pub(super) fn pending_checkpoint(&self) -> usize { self.pending.len() }
    pub(super) fn discard_since(&mut self, checkpoint: usize) { self.pending.truncate(checkpoint); }
    pub(super) fn finish(self, root: DataFrame) -> Result<PreparedPlan> {
        Ok(PreparedPlan { root: root.into_parts().1, pending: self.pending })
    }
}
pub(super) fn recursive_seed(frame: DataFrame) -> Result<DataFrame> {
    let kinds = frame.schema().fields().iter().map(|field| match field.data_type() {
        DataType::Null => Err(unsupported("A recursive seed column needs a non-null exact type")),
        kind if values::integer(kind) => Ok(DataType::Decimal128(20, 0)),
        kind => Ok(kind.clone()),
    }).collect::<Result<Vec<_>>>()?;
    let names = frame.schema().fields().iter().map(|f| f.name().clone()).collect::<Vec<_>>();
    normalize(frame, &kinds, &names)
}
fn normalize_union(frames: Vec<DataFrame>) -> Result<(Vec<DataFrame>, Vec<DataType>, Vec<String>)> {
    let first = frames.first().ok_or_else(|| unsupported("UnionAll requires inputs"))?;
    let width = first.schema().fields().len();
    if width == 0 || frames.iter().any(|f| f.schema().fields().len() != width) {
        return Err(unsupported("UnionAll inputs require one matching nonempty positional width"));
    }
    let names = first.schema().fields().iter().map(|f| f.name().clone()).collect::<Vec<_>>();
    let mut kinds = Vec::with_capacity(width);
    for index in 0..width {
        let types = frames.iter().map(|f| f.schema().field(index).data_type()).filter(|kind| **kind != DataType::Null).collect::<Vec<_>>();
        let kind = if types.is_empty() { DataType::Null }
        else if types.iter().all(|kind| *kind == types[0]) { (*types[0]).clone() }
        else if types.iter().all(|kind| values::integer(kind)) { DataType::Decimal128(20, 0) }
        else if types.iter().all(|kind| matches!(kind, DataType::Float32 | DataType::Float64)) { DataType::Float64 }
        else { return Err(unsupported("UnionAll needs an exact common type; mixed integer/float and heterogeneous variants are not admitted")); };
        kinds.push(kind);
    }
    let normalized = frames.into_iter().map(|frame| normalize(frame, &kinds, &names)).collect::<Result<Vec<_>>>()?;
    Ok((normalized, kinds, names))
}
fn normalize(frame: DataFrame, kinds: &[DataType], names: &[String]) -> Result<DataFrame> {
    if frame.schema().fields().len() != kinds.len() { return Err(unsupported("Compound input column count differs")); }
    let columns = frame.schema().columns().into_iter().enumerate().map(|(index, column)| {
        let mut value = Expr::Column(column);
        let actual = value.get_type(frame.schema())?;
        let expected = &kinds[index];
        if &actual != expected {
            let safe = actual == DataType::Null || values::integer(&actual) && *expected == DataType::Decimal128(20, 0)
                || actual == DataType::Float32 && *expected == DataType::Float64;
            if !safe { return Err(unsupported("Compound coercion would narrow or round an exact value")); }
            value = Expr::Cast(Cast::new(Box::new(value), expected.clone()));
        }
        Ok(value.alias(&names[index]))
    }).collect::<Result<Vec<_>>>()?;
    frame.select(columns)
}
enum MaterializationKind { Recursive, OrderedUnion(Vec<LogicalPlan>) }
struct Materialization { name: String, logical: LogicalPlan, kind: MaterializationKind }
pub(in crate::engine) struct PreparedPlan { root: LogicalPlan, pending: Vec<Materialization> }
impl PreparedPlan {
    pub(in crate::engine) fn has_prerequisites(&self) -> bool { !self.pending.is_empty() }
    pub(in crate::engine) fn explain_frame(&self, context: &SessionContext) -> Result<DataFrame> {
        let mut expanded = BTreeMap::new();
        for item in &self.pending {
            let plan = substitute(item.logical.clone(), &expanded)?;
            expanded.insert(item.name.clone(), LogicalPlanBuilder::from(plan).alias(&item.name)?.build()?);
        }
        Ok(DataFrame::new(context.state(), substitute(self.root.clone(), &expanded)?))
    }
    /// Caller retains an admitted logical-plan clone through native stream drop
    /// and independent final output compaction. MemTable scans do not own guards.
    pub(in crate::engine) async fn complete(self, context: &SessionContext, owner: Arc<QueryOwner>, maximum_bytes: usize) -> Result<DataFrame> {
        owner.check()?;
        if maximum_bytes == 0 { return Err(external(crate::engine::limit())); }
        for item in &self.pending {
            owner.work(1)?;
            context.state().create_physical_plan(&item.logical).await?;
            if let MaterializationKind::OrderedUnion(inputs) = &item.kind {
                for input in inputs { context.state().create_physical_plan(input).await?; }
            }
        }
        context.state().create_physical_plan(&self.root).await?;
        let mut completed = BTreeMap::new();
        for item in self.pending {
            owner.check()?;
            let logical = substitute(item.logical, &completed)?;
            let physical = match item.kind {
                MaterializationKind::Recursive => context.state().create_physical_plan(&logical).await?,
                MaterializationKind::OrderedUnion(inputs) => {
                    let mut physical = Vec::with_capacity(inputs.len());
                    for input in inputs {
                        let input = substitute(input, &completed)?;
                        physical.push(context.state().create_physical_plan(&input).await?);
                    }
                    UnionExec::try_new(physical)?
                }
            };
            let schema = physical.schema();
            let mut admission = owner.budget.reserve_retained(maximum_bytes).map_err(external)?.into_owned(&owner.budget).map_err(external)?;
            let mut output = OutputBatches::new(schema.clone(), maximum_bytes);
            for partition in 0..physical.output_partitioning().partition_count() {
                owner.work(1)?;
                let mut stream = physical.execute(partition, context.task_ctx())?;
                loop {
                    owner.check()?;
                    let next = loop {
                        match tokio::time::timeout(Duration::from_millis(20), stream.next()).await {
                            Ok(next) => break next,
                            Err(_) => owner.check()?,
                        }
                    };
                    let Some(batch) = next else { break };
                    let batch = batch?;
                    owner.work(batch.num_rows().checked_add(1).ok_or_else(|| external(crate::engine::limit()))?)?;
                    output.push(batch, &owner.budget).map_err(external)?;
                }
            }
            let (batches, bytes, _) = output.finish(&owner.budget).map_err(external)?;
            admission.shrink_to(bytes).map_err(external)?;
            let table = Arc::new(OwnedTable { inner: MemTable::try_new(schema, vec![batches])?, _admission: admission, _owner: owner.clone() });
            let scan = LogicalPlanBuilder::scan(item.name.clone(), provider_as_source(table), None)?.build()?;
            completed.insert(item.name, scan);
        }
        owner.check()?;
        Ok(DataFrame::new(context.state(), substitute(self.root, &completed)?))
    }
}
fn substitute(plan: LogicalPlan, values: &BTreeMap<String, LogicalPlan>) -> Result<LogicalPlan> {
    plan.transform_up_with_subqueries(|plan| {
        if let LogicalPlan::TableScan(scan) = &plan {
            if let Some(value) = values.get(scan.table_name.table()) {
                if scan.projection.is_some() || !scan.filters.is_empty() || scan.fetch.is_some() {
                    return Err(unsupported("Completion placeholder was optimized before substitution"));
                }
                return Ok(Transformed::yes(value.clone()));
            }
        }
        Ok(Transformed::no(plan))
    }).data()
}
struct OwnedTable { inner: MemTable, _admission: crate::resource::OwnedRetainedReservation, _owner: Arc<QueryOwner> }
impl std::fmt::Debug for OwnedTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("CompletedNativeCompound") }
}
#[async_trait::async_trait]
impl TableProvider for OwnedTable {
    fn schema(&self) -> SchemaRef { self.inner.schema() }
    fn table_type(&self) -> TableType { TableType::Temporary }
    async fn scan(&self, state: &dyn Session, projection: Option<&Vec<usize>>, filters: &[Expr], limit: Option<usize>) -> Result<Arc<dyn ExecutionPlan>> {
        self._owner.check()?;
        self.inner.scan(state, projection, filters, limit).await
    }
}
pub(super) fn validate_scopes(plan: &QueryPlan) -> Result<()> {
    let mut pending = vec![(Node::Plan(plan), None::<&str>, 0usize)];
    while let Some((node, binding, in_query)) = pending.pop() {
        match node {
            Node::Plan(QueryPlan::Iterate { binding: name, seed, step }) => {
                if binding.is_some() { return Err(unsupported("Nested Iterate inside a recursive step is unsupported")); }
                let mut references = 0usize;
                let mut nodes = vec![Node::Plan(step)];
                while let Some(node) = nodes.pop() {
                    if let Node::Plan(QueryPlan::Delta { binding }) = node { if binding == name { references += 1; } }
                    children(node, &mut nodes);
                }
                if references != 1 { return Err(unsupported("Iterate step requires exactly one matching Delta reference")); }
                pending.push((Node::Plan(step), Some(name.as_str()), in_query));
                pending.push((Node::Plan(seed), None, in_query));
            }
            Node::Plan(QueryPlan::Delta { binding: name }) => {
                if binding != Some(name.as_str()) { return Err(unsupported("Unbound or wrong-scope Delta")); }
            }
            Node::Expression(Expression::OuterColumn { depth, .. }) if *depth == 0 || *depth > in_query => {
                return Err(unsupported("OuterColumn depth exceeds its lexical query scope"));
            }
            Node::Expression(Expression::InQuery { value, input, .. }) => {
                pending.push((Node::Plan(input), binding, in_query + 1));
                pending.push((Node::Expression(value), binding, in_query));
            }
            Node::Expression(Expression::ExistsQuery { input, .. } | Expression::ScalarQuery { input }) => pending.push((Node::Plan(input), binding, in_query + 1)),
            _ => {
                let mut nested = Vec::new();
                children(node, &mut nested);
                for node in nested { pending.push((node, binding, in_query)); }
            }
        }
    }
    Ok(())
}
