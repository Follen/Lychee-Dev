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
    match term {
        Term::Aggregate(..) => true,
        Term::Unary(_, t) | Term::NullTest(t, _) | Term::Cast(t, _) => contains(t),
        Term::Binary(a, _, b) | Term::Like(a, b, _) => contains(a) || contains(b),
        Term::Between(a, b, c, _) => contains(a) || contains(b) || contains(c),
        Term::In(t, args, _) => contains(t) || args.iter().any(contains),
        // Inner aggregates belong to the nested query, not this group.
        Term::InSubquery(input, _, _) => contains(input),
        Term::Call(_, args, _) => args.iter().any(contains),
        Term::Case(base, args, other) => {
            base.as_deref().is_some_and(contains)
                || args.iter().any(|(a, b)| contains(a) || contains(b))
                || other.as_deref().is_some_and(contains)
        }
        _ => false,
    }
}
// Go validates grouped correlations after prepareGroups has inspected every
// expression. This is distinct from the allocation-only rewrite above.
fn validate_correlations(
    node: &Term,
    keys: &[Term],
    params: &BTreeMap<String, Scalar>,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<()> {
    work.spend()?;
    if keys
        .iter()
        .any(|key| subquery::same_group_expression(node, key, context))
    {
        return Ok(());
    }
    if matches!(
        node,
        Term::Aggregate(..) | Term::InSubquery(..) | Term::ScalarSubquery(_) | Term::Exists(_)
    ) {
        let group = context.group.ok_or_else(binding)?;
        return bind_with_aggregates(
            node,
            group.columns,
            group.alias,
            params,
            true,
            context,
            work,
        );
    }
    let mut visit = |node: &Term| validate_correlations(node, keys, params, context, work);
    match node {
        Term::Unary(_, node) | Term::NullTest(node, _) | Term::Cast(node, _) => visit(node)?,
        Term::Binary(left, _, right) | Term::Like(left, right, _) => {
            visit(left)?;
            visit(right)?;
        }
        Term::Between(a, b, c, _) => {
            visit(a)?;
            visit(b)?;
            visit(c)?;
        }
        Term::In(node, args, _) => {
            visit(node)?;
            for arg in args {
                visit(arg)?;
            }
        }
        Term::Call(_, args, _) => {
            for arg in args {
                visit(arg)?;
            }
        }
        Term::Case(base, branches, fallback) => {
            if let Some(base) = base {
                visit(base)?;
            }
            for (condition, value) in branches {
                visit(condition)?;
                visit(value)?;
            }
            if let Some(fallback) = fallback {
                visit(fallback)?;
            }
        }
        _ => {}
    }
    Ok(())
}
struct Group {
    values: Vec<Scalar>,
    states: Vec<State>,
}
pub(super) struct Grouping {
    keys: Vec<Term>,
    specs: Vec<Spec>,
    having: Option<Term>,
    lookup: BTreeMap<String, usize>,
    order: Vec<Group>,
    columns: Vec<String>,
}
impl Grouping {
    pub(super) fn new(
        keys: &[Term],
        outputs: &mut [Term],
        order: &mut [(Term, bool, Option<bool>)],
        having: Option<&Term>,
        output_columns: &[String],
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
    ) -> Result<Self> {
        work.safety_retain(keys.len() * 64 + outputs.len() * 64 + order.len() * 64 + 256)?;
        let mut this = Self {
            keys: keys.to_vec(),
            specs: vec![],
            having: None,
            lookup: BTreeMap::new(),
            order: vec![],
            columns: (0..keys.len()).map(|i| format!("__key_{i}")).collect(),
        };
        let rewritten_outputs = outputs
            .iter()
            .map(|term| this.rewrite(term, false, output_columns, params, context, work))
            .collect::<Result<Vec<_>>>()?;
        this.having = having
            .map(|term| this.rewrite(term, false, output_columns, params, context, work))
            .transpose()?;
        let rewritten_order = order
            .iter()
            .map(|(term, _, _)| this.rewrite(term, false, output_columns, params, context, work))
            .collect::<Result<Vec<_>>>()?;
        if work.planning {
            for term in outputs
                .iter()
                .chain(having)
                .chain(order.iter().map(|(term, _, _)| term))
            {
                validate_correlations(term, keys, params, context, work)?;
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
    fn rewrite(
        &mut self,
        node: &Term,
        allow_alias: bool,
        aliases: &[String],
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
    ) -> Result<Term> {
        work.consume(0)?;
        if let Some(i) = self
            .keys
            .iter()
            .position(|t| subquery::same_group_expression(t, node, context))
        {
            return Ok(Term::Column(ColumnRef::new(self.columns[i].clone())));
        }
        let recurse = |this: &mut Self, t: &Term, work: &mut Work<'_>| {
            this.rewrite(t, allow_alias, aliases, params, context, work)
        };
        Ok(match node {
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
                Term::Column(ColumnRef::new(column))
            }
            // Internal compound ordinals are input columns, not literals.
            // A matching group key was handled above; otherwise projecting
            // one from an aggregate group is invalid even for empty input.
            Term::Ordinal(_) | Term::Star(_) => return Err(binding()),
            Term::Column(name) => {
                let outer = context.group.is_some_and(|group| {
                    matches!(
                        subquery::resolve(group.columns, group.alias, name, context.scope.outer),
                        Ok(subquery::Position::Outer(..))
                    )
                });
                if outer
                    || allow_alias
                        && name.qualifier.is_empty()
                        && aliases.iter().any(|a| equal_fold(a, &name.name))
                {
                    node.clone()
                } else {
                    return Err(binding());
                }
            }
            Term::Unary(op, t) => Term::Unary(*op, Box::new(recurse(self, t, work)?)),
            Term::Binary(a, op, b) => Term::Binary(
                Box::new(recurse(self, a, work)?),
                op.clone(),
                Box::new(recurse(self, b, work)?),
            ),
            Term::Cast(t, kind) => {
                scalar::cast_kind(kind)?;
                Term::Cast(Box::new(recurse(self, t, work)?), kind.clone())
            }
            Term::Like(a, b, negated) => Term::Like(
                Box::new(recurse(self, a, work)?),
                Box::new(recurse(self, b, work)?),
                *negated,
            ),
            Term::NullTest(t, n) => Term::NullTest(Box::new(recurse(self, t, work)?), *n),
            Term::Between(a, b, c, n) => Term::Between(
                Box::new(recurse(self, a, work)?),
                Box::new(recurse(self, b, work)?),
                Box::new(recurse(self, c, work)?),
                *n,
            ),
            Term::In(t, args, n) => Term::In(
                Box::new(recurse(self, t, work)?),
                args.iter()
                    .map(|a| recurse(self, a, work))
                    .collect::<Result<Vec<_>>>()?,
                *n,
            ),
            Term::ScalarSubquery(_) | Term::Exists(_) => node.clone(),
            Term::InSubquery(input, query, negated) => Term::InSubquery(
                Box::new(recurse(self, input, work)?),
                query.clone(),
                *negated,
            ),
            Term::Call(name, args, distinct) => Term::Call(
                name.clone(),
                args.iter()
                    .map(|a| recurse(self, a, work))
                    .collect::<Result<Vec<_>>>()?,
                *distinct,
            ),
            Term::Case(base, args, other) => Term::Case(
                base.as_ref()
                    .map(|t| recurse(self, t, work).map(Box::new))
                    .transpose()?,
                args.iter()
                    .map(|(a, b)| Ok((recurse(self, a, work)?, recurse(self, b, work)?)))
                    .collect::<Result<Vec<_>>>()?,
                other
                    .as_ref()
                    .map(|t| recurse(self, t, work).map(Box::new))
                    .transpose()?,
            ),
            _ => node.clone(),
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
    pub(super) fn add(
        &mut self,
        row: &[Scalar],
        columns: &[String],
        alias: ColumnOwners<'_>,
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
    ) -> Result<()> {
        work.spend()?;
        let mut values = vec![];
        let mut key = String::new();
        for node in &self.keys {
            let v = eval_scoped(node, row, columns, alias, params, context, work)?;
            let payload = if let Scalar::String(text) = &v {
                text.len()
            } else {
                0
            };
            work.temporary(payload + 8192)?;
            let part = results::equal_key(&v)?;
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
            values.push(v);
        }
        let i = if let Some(i) = self.lookup.get(&key) {
            *i
        } else {
            self.create(key, values, work)?
        };
        for (spec, state) in self.specs.iter().zip(&mut self.order[i].states) {
            let value = if let Some(node) = &spec.input {
                eval_scoped(node, row, columns, alias, params, context, work)?
            } else {
                Scalar::Null
            };
            state.add(spec, value, work)?;
        }
        Ok(())
    }
    pub(super) fn columns(&self) -> &[String] {
        &self.columns
    }
    pub(super) fn finish(
        mut self,
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
        emit: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
    ) -> Result<()> {
        if self.order.is_empty() && self.keys.is_empty() {
            self.create(String::new(), vec![], work)?;
        }
        for group in self.order {
            work.temporary_scope(|work| {
                work.spend()?;
                let mut row = group.values;
                for (spec, state) in self.specs.iter().zip(group.states) {
                    row.push(state.result(spec, work)?);
                }
                if let Some(node) = &self.having
                    && truth(&eval_scoped(
                        node,
                        &row,
                        &self.columns,
                        ColumnOwners::Single(""),
                        params,
                        context,
                        work,
                    )?)? != Some(true)
                {
                    return Ok(());
                }
                emit(&row, work)
            })?;
        }
        Ok(())
    }
}
