//! Streaming groups retain keys and aggregate state, never arbitrary input rows.
use super::*;
/// Validate only when the real binder/prepareGroups reaches this call. A star
/// is syntax in the immutable Program, but COUNT(*) has no per-row input term.
pub(super) fn input<'a>(name: &str, args: &'a [Term], distinct: bool) -> Result<Option<&'a Term>> {
    let [input] = args else {
        return Err(type_error());
    };
    if let Term::Star(qualifier) = input {
        if name != "COUNT" || distinct || !qualifier.is_empty() {
            return Err(type_error());
        }
        Ok(None)
    } else {
        Ok(Some(input))
    }
}
#[derive(Clone)]
struct Spec {
    name: String,
    input: Option<Box<Term>>,
    distinct: bool,
}
struct State {
    count: i64,
    sum: BigRational,
    integer: bool,
    extreme: Scalar,
    extreme_bytes: usize,
    extreme_allocation: Option<crate::resource::OwnedRetainedReservation>,
    seen: Option<BTreeSet<String>>,
    admitted_numeric_bytes: usize,
}
impl State {
    fn new(spec: &Spec) -> Self {
        Self {
            count: 0,
            sum: BigRational::zero(),
            integer: true,
            extreme: Scalar::Null,
            extreme_bytes: 0,
            extreme_allocation: None,
            seen: spec.distinct.then(BTreeSet::new),
            admitted_numeric_bytes: 512,
        }
    }
    fn add(&mut self, spec: &Spec, value: Scalar, work: &mut Work<'_>) -> Result<()> {
        work.spend()?;
        if spec.input.is_some() && value == Scalar::Null {
            return Ok(());
        }
        if let Scalar::String(text) = &value
            && text.len() > 1 << 20
        {
            return Err(budget());
        }
        if let Some(seen) = &mut self.seen {
            let payload = if let Scalar::String(text) = &value {
                text.len()
            } else {
                0
            };
            work.temporary(payload + 8192)?;
            let key = results::equal_key(&value)?;
            if seen.contains(&key) {
                return Ok(());
            }
            work.retain(key.len() + 64)?;
            seen.insert(key);
        }
        if self.count == i64::MAX {
            return Err(numeric());
        }
        match spec.name.as_str() {
            "MIN" | "MAX" => {
                if self.count == 0 || {
                    let cmp = compare(&value, &self.extreme)?;
                    (spec.name == "MIN" && cmp == Ordering::Less)
                        || (spec.name == "MAX" && cmp == Ordering::Greater)
                } {
                    let bytes = if let Scalar::String(text) = &value {
                        text.len()
                    } else {
                        0
                    };
                    if bytes > self.extreme_bytes {
                        work.bytes = work
                            .bytes
                            .checked_sub(bytes - self.extreme_bytes)
                            .ok_or_else(budget)?;
                    } else {
                        work.bytes = work
                            .bytes
                            .checked_add(self.extreme_bytes - bytes)
                            .ok_or_else(budget)?;
                    }
                    let allocation = if let Some(shared) = work.shared {
                        let owner = std::sync::Arc::new(shared.safety_only());
                        Some(owner.reserve_retained(bytes)?.into_owned(&owner)?)
                    } else {
                        None
                    };
                    self.extreme_bytes = bytes;
                    self.extreme = value;
                    self.extreme_allocation = allocation;
                }
            }
            "SUM" | "AVG" => {
                let n = rational(&value)?;
                let numerator_bits = (self.sum.numer().bits() + n.denom().bits())
                    .max(n.numer().bits() + self.sum.denom().bits())
                    + 1;
                let denominator_bits = self.sum.denom().bits() + n.denom().bits();
                let capacity = usize::try_from((numerator_bits + denominator_bits).div_ceil(8))
                    .map_err(|_| budget())?
                    .checked_mul(2)
                    .and_then(|n| n.checked_add(128))
                    .ok_or_else(budget)?;
                if capacity > self.admitted_numeric_bytes {
                    work.safety_retain(capacity - self.admitted_numeric_bytes)?;
                    self.admitted_numeric_bytes = capacity;
                }
                self.sum += n;
                self.integer &= int(&value).is_some();
            }
            "COUNT" => {}
            _ => return Err(unsupported()),
        }
        self.count += 1;
        Ok(())
    }
    fn result(&self, spec: &Spec, work: &mut Work<'_>) -> Result<Scalar> {
        work.consume(0)?;
        if spec.name == "COUNT" {
            return Ok(Scalar::Int(self.count));
        }
        if self.count == 0 {
            return Ok(Scalar::Null);
        }
        match spec.name.as_str() {
            "MIN" | "MAX" => work.clone_scalar(&self.extreme),
            "SUM" if self.integer => integer(self.sum.to_integer().to_i128().ok_or_else(numeric)?),
            "SUM" => Ok(Scalar::Float(
                self.sum
                    .to_f64()
                    .filter(|f| f.is_finite())
                    .ok_or_else(numeric)?,
            )),
            "AVG" => {
                let ratio = &self.sum / BigRational::from_integer(self.count.into());
                Ok(Scalar::Float(
                    ratio
                        .to_f64()
                        .filter(|f| f.is_finite())
                        .ok_or_else(numeric)?,
                ))
            }
            _ => Err(unsupported()),
        }
    }
}
pub(super) fn contains(term: &Term) -> bool {
    let mut pending = vec![term];
    while let Some(term) = pending.pop() {
        if matches!(term, Term::Aggregate(..)) {
            return true;
        }
        term_tree::children(term, &mut pending);
    }
    false
}
/// The inspection order and node charges match the synchronous binder. Only
/// the nested query boundary suspends; no Work or borrowed context view is
/// retained by the child request.
async fn validate_correlations_async<'a, 'w: 'a>(
    node: &Term,
    keys: &[Term],
    context: &execution::Context<'a>,
    control: &execution::Control<'a, 'w>,
) -> Result<()> {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        control.work(|work| work.spend())?;
        if keys
            .iter()
            .any(|key| subquery::same_group_expression(node, key, &context.view()))
        {
            continue;
        }
        if matches!(
            node,
            Term::Aggregate(..) | Term::InSubquery(..) | Term::ScalarSubquery(_) | Term::Exists(_)
        ) {
            let group = context.group.as_ref().ok_or_else(binding)?;
            binding::bind_async(node, &group.fields, true, context, control).await?;
        } else {
            term_tree::children(node, &mut pending);
        }
    }
    Ok(())
}

struct Group {
    values: Vec<Scalar>,
    states: Vec<State>,
}
pub(super) struct Grouping<'a> {
    keys: Vec<Term>,
    specs: Vec<Spec>,
    having: Option<Term>,
    lookup: BTreeMap<String, usize>,
    order: Vec<Group>,
    columns: Vec<String>,
    _allocation: Option<crate::resource::RetainedReservation<'a>>,
}
impl<'a> Grouping<'a> {
    fn rewrite(
        &mut self,
        node: &Term,
        allow_alias: bool,
        aliases: &[String],
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
    ) -> Result<Term> {
        term_tree::rewrite(node, |node| {
            work.consume(0)?;
            if let Some(i) = self
                .keys
                .iter()
                .position(|t| subquery::same_group_expression(t, node, context))
            {
                return Ok(Some(Term::Column(ColumnRef::new(self.columns[i].clone()))));
            }
            match node {
                Term::Aggregate(name, args, distinct) => {
                    let input = input(name, args, *distinct)?;
                    if input.is_some_and(contains) {
                        return Err(type_error());
                    }
                    let column = format!("__agg_{}", self.specs.len());
                    work.safety_retain(128)?;
                    self.columns.push(column.clone());
                    self.specs.push(Spec {
                        name: name.clone(),
                        input: input.cloned().map(Box::new),
                        distinct: *distinct,
                    });
                    Ok(Some(Term::Column(ColumnRef::new(column))))
                }
                Term::Ordinal(_) | Term::Star(_) => Err(binding()),
                Term::Column(name) => {
                    let outer = context.group.is_some_and(|group| {
                        matches!(
                            subquery::resolve(
                                group.columns,
                                group.alias,
                                name,
                                context.scope.outer
                            ),
                            Ok(subquery::Position::Outer(..))
                        )
                    });
                    if outer
                        || allow_alias
                            && name.qualifier.is_empty()
                            && aliases.iter().any(|a| equal_fold(a, &name.name))
                    {
                        Ok(None)
                    } else {
                        Err(binding())
                    }
                }
                Term::Cast(_, kind) => {
                    scalar::cast_kind(kind)?;
                    Ok(None)
                }
                _ => Ok(None),
            }
        })
    }

    fn create(&mut self, key: String, values: Vec<Scalar>, work: &mut Work<'_>) -> Result<usize> {
        // Scalar states use the original per-aggregate admission. Numeric
        // persistent growth is charged before addition, and the plan reserves
        // one bounded scratch buffer shared by sequential arithmetic work.
        work.retain(key.len() + row_cost(&values)? - 24 + self.specs.len() * 512 + 128)?;
        let i = self.order.len();
        self.order.push(Group {
            values,
            states: self.specs.iter().map(State::new).collect(),
        });
        self.lookup.insert(key, i);
        Ok(i)
    }
    pub(super) fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Prepare all rewritten expressions before validating correlations, just
    /// as the original prepareGroups path does. Immutable scalar-query Arcs
    /// remain shared by the existing Term clone/rewrite implementation.
    pub(super) async fn new_async<'w: 'a>(
        keys: &[Term],
        outputs: &mut [Term],
        order: &mut [(Term, bool, Option<bool>)],
        having: Option<&Term>,
        output_columns: &[String],
        context: &execution::Context<'a>,
        control: &execution::Control<'a, 'w>,
    ) -> Result<Box<Self>> {
        let mut this = control.work(|work| {
            work.safety_retain(keys.len() * 64 + outputs.len() * 64 + order.len() * 64 + 256)?;
            let allocation = work
                .shared
                .map(|budget| budget.reserve_retained(std::mem::size_of::<Self>()))
                .transpose()?;
            Ok(Box::new(Self {
                keys: keys.to_vec(),
                specs: vec![],
                having: None,
                lookup: BTreeMap::new(),
                order: vec![],
                columns: (0..keys.len()).map(|i| format!("__key_{i}")).collect(),
                _allocation: allocation,
            }))
        })?;
        let (rewritten_outputs, rewritten_order) = control.work(|work| {
            let context = context.view();
            let rewritten_outputs = outputs
                .iter()
                .map(|term| {
                    this.rewrite(
                        term,
                        false,
                        output_columns,
                        control.parameters(),
                        &context,
                        work,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            this.having = having
                .map(|term| {
                    this.rewrite(
                        term,
                        false,
                        output_columns,
                        control.parameters(),
                        &context,
                        work,
                    )
                })
                .transpose()?;
            let rewritten_order = order
                .iter()
                .map(|(term, _, _)| {
                    this.rewrite(
                        term,
                        false,
                        output_columns,
                        control.parameters(),
                        &context,
                        work,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((rewritten_outputs, rewritten_order))
        })?;
        if control.planning() {
            for term in outputs
                .iter()
                .chain(having)
                .chain(order.iter().map(|(term, _, _)| term))
            {
                validate_correlations_async(term, keys, context, control).await?;
            }
        }
        for (output, rewritten) in outputs.iter_mut().zip(rewritten_outputs) {
            *output = rewritten;
        }
        for ((order, _, _), rewritten) in order.iter_mut().zip(rewritten_order) {
            *order = rewritten;
        }
        Ok(this)
    }

    pub(super) async fn add_async<'w: 'a>(
        &mut self,
        row: &execution::Row<'a>,
        fields: &execution::Fields<'a>,
        context: &execution::Context<'a>,
        control: &execution::Control<'a, 'w>,
    ) -> Result<()> {
        control.work(|work| work.spend())?;
        let mut values = vec![];
        let mut key = String::new();
        for node in &self.keys {
            let value = evaluation::eval_async(node, row, fields, context, control).await?;
            control.work(|work| {
                let payload = if let Scalar::String(text) = &value {
                    text.len()
                } else {
                    0
                };
                work.temporary(payload + 8192)?;
                let part = results::equal_key(&value)?;
                work.consume(part.len())?;
                work.temporary(
                    part.len()
                        .checked_add(32)
                        .and_then(|n| n.checked_mul(2))
                        .ok_or_else(budget)?,
                )?;
                key.push_str(&format!("{}:{part}", part.len()));
                if key.len() > work.bytes {
                    return Err(budget());
                }
                Ok(())
            })?;
            values.push(value);
        }
        let index = if let Some(index) = self.lookup.get(&key) {
            *index
        } else {
            control.work(|work| self.create(key, values, work))?
        };
        for (spec, state) in self.specs.iter().zip(&mut self.order[index].states) {
            let value = if let Some(node) = &spec.input {
                evaluation::eval_async(node, row, fields, context, control).await?
            } else {
                Scalar::Null
            };
            control.work(|work| state.add(spec, value, work))?;
        }
        Ok(())
    }

    pub(super) fn into_finish<'c, 'w: 'a>(
        mut self: Box<Self>,
        control: &'c execution::Control<'a, 'w>,
    ) -> Result<Finish<'c, 'a, 'w>> {
        if self.order.is_empty() && self.keys.is_empty() {
            control.work(|work| self.create(String::new(), vec![], work))?;
        }
        let groups = std::mem::take(&mut self.order).into_iter();
        Ok(Finish {
            grouping: self,
            groups,
            control,
            temporary_mark: None,
        })
    }
}

/// Finite result cursor over retained aggregate groups. A yielded row's
/// temporary scope includes the caller's projection/order expressions, just as
/// the synchronous emit callback did. Drop the row, then call finish_row;
/// dropping this cursor also releases that scope after an error or cancellation.
pub(super) struct Finish<'c, 'a, 'w: 'a> {
    grouping: Box<Grouping<'a>>,
    groups: std::vec::IntoIter<Group>,
    control: &'c execution::Control<'a, 'w>,
    temporary_mark: Option<usize>,
}

impl<'a, 'w: 'a> Finish<'_, 'a, 'w> {
    pub(super) async fn next(
        &mut self,
        fields: &execution::Fields<'a>,
        context: &execution::Context<'a>,
    ) -> Result<Option<execution::Row<'a>>> {
        self.finish_row();
        while let Some(group) = self.groups.next() {
            self.temporary_mark = Some(self.control.temporary_mark());
            let result = async {
                let row = self.control.work(|work| {
                    work.spend()?;
                    let mut row = group.values;
                    for (spec, state) in self.grouping.specs.iter().zip(group.states) {
                        row.push(state.result(spec, work)?);
                    }
                    Ok(row)
                })?;
                let row = execution::Row::Values(self.control.share(row)?);
                if let Some(node) = &self.grouping.having
                    && truth(
                        &evaluation::eval_async(node, &row, fields, context, self.control).await?,
                    )? != Some(true)
                {
                    return Ok(None);
                }
                Ok(Some(row))
            }
            .await;
            match result {
                Ok(Some(row)) => return Ok(Some(row)),
                Ok(None) => self.finish_row(),
                Err(error) => {
                    self.finish_row();
                    return Err(error);
                }
            }
        }
        Ok(None)
    }

    pub(super) fn finish_row(&mut self) {
        if let Some(mark) = self.temporary_mark.take() {
            self.control.temporary_end(mark);
        }
    }
}

impl<'a, 'w: 'a> Drop for Finish<'_, 'a, 'w> {
    fn drop(&mut self) {
        self.finish_row();
    }
}
