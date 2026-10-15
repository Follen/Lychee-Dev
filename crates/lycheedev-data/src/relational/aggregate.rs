//! Streaming groups retain keys and aggregate state, never arbitrary input rows.
use super::*;
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
            seen: spec.distinct.then(BTreeSet::new),
            admitted_numeric_bytes: 512,
        }
    }
    fn add(&mut self, spec: &Spec, value: Scalar, work: &mut Work<'_>) -> Result<()> {
        work.spend()?;
        if spec.input.is_some() && value == Scalar::Null {
            return Ok(());
        }
        if let Some(seen) = &mut self.seen {
            let key = equal_key(&value, work)?;
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
                    self.extreme = work.clone_scalar(&value)?;
                }
            }
            "SUM" | "AVG" => {
                let n = rational(&value)?;
                if n.numer().bits() > 4096 || n.denom().bits() > 4096 {
                    return Err(budget());
                }
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
                    work.retain(capacity - self.admitted_numeric_bytes)?;
                    self.admitted_numeric_bytes = capacity;
                }
                self.sum += n;
                self.integer &= int(&value).is_some();
                if self.sum.numer().bits() > 4096 || self.sum.denom().bits() > 4096 {
                    return Err(budget());
                }
            }
            "COUNT" => {}
            _ => return Err(unsupported()),
        }
        self.count += 1;
        Ok(())
    }
    fn result(&self, spec: &Spec, work: &mut Work<'_>) -> Result<Scalar> {
        work.spend()?;
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
fn equal_key(value: &Scalar, work: &mut Work<'_>) -> Result<String> {
    work.spend()?;
    Ok(match value {
        Scalar::Null => "z".into(),
        Scalar::Bool(v) => format!("b{}", u8::from(*v)),
        Scalar::String(s) => {
            work.consume(s.len())?;
            if s.len() > 1 << 20 {
                return Err(budget());
            }
            format!("s{s}")
        }
        _ => format!("n{}", rational(value)?),
    })
}
pub(super) fn contains(term: &Term) -> bool {
    match term {
        Term::Aggregate(..) => true,
        Term::Unary(_, t) | Term::NullTest(t, _) => contains(t),
        Term::Binary(a, _, b) => contains(a) || contains(b),
        Term::Between(a, b, c, _) => contains(a) || contains(b) || contains(c),
        Term::In(t, args, _) => contains(t) || args.iter().any(contains),
        Term::Call(_, args) => args.iter().any(contains),
        Term::Case(base, args, other) => {
            base.as_deref().is_some_and(contains)
                || args.iter().any(|(a, b)| contains(a) || contains(b))
                || other.as_deref().is_some_and(contains)
        }
        _ => false,
    }
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
        work: &mut Work<'_>,
    ) -> Result<Self> {
        work.retain(keys.len() * 64 + outputs.len() * 64 + order.len() * 64 + 256)?;
        let mut this = Self {
            keys: keys.to_vec(),
            specs: vec![],
            having: None,
            lookup: BTreeMap::new(),
            order: vec![],
            columns: (0..keys.len()).map(|i| format!("__key_{i}")).collect(),
        };
        for t in outputs {
            *t = this.rewrite(t, false, output_columns, work)?;
        }
        for (t, _, _) in order {
            *t = this.rewrite(t, true, output_columns, work)?;
        }
        if this
            .specs
            .iter()
            .any(|s| ["SUM", "AVG"].contains(&s.name.as_str()))
        {
            work.retain(4096)?;
        }
        this.having = having
            .map(|t| this.rewrite(t, false, output_columns, work))
            .transpose()?;
        Ok(this)
    }
    fn rewrite(
        &mut self,
        node: &Term,
        allow_alias: bool,
        aliases: &[String],
        work: &mut Work<'_>,
    ) -> Result<Term> {
        work.spend()?;
        if let Some(i) = self.keys.iter().position(|t| t == node) {
            return Ok(Term::Column(self.columns[i].clone()));
        }
        let recurse = |this: &mut Self, t: &Term, work: &mut Work<'_>| {
            this.rewrite(t, allow_alias, aliases, work)
        };
        Ok(match node {
            Term::Aggregate(name, input, distinct) => {
                if input.as_deref().is_some_and(contains) {
                    return Err(type_error());
                }
                let column = format!("__agg_{}", self.specs.len());
                work.retain(128)?;
                self.columns.push(column.clone());
                self.specs.push(Spec {
                    name: name.clone(),
                    input: input.clone(),
                    distinct: *distinct,
                });
                Term::Column(column)
            }
            // Internal compound ordinals are input columns, not literals.
            // A matching group key was handled above; otherwise projecting
            // one from an aggregate group is invalid even for empty input.
            Term::Ordinal(_) => return Err(binding()),
            Term::Column(name) => {
                if allow_alias && aliases.iter().any(|a| a.eq_ignore_ascii_case(name)) {
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
            Term::Call(name, args) => Term::Call(
                name.clone(),
                args.iter()
                    .map(|a| recurse(self, a, work))
                    .collect::<Result<Vec<_>>>()?,
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
        work.retain(key.len() + row_cost(&values)? + self.specs.len() * 512 + 128)?;
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
        alias: &str,
        params: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<()> {
        work.spend()?;
        let mut values = vec![];
        let mut key = String::new();
        for node in &self.keys {
            let v = eval(node, row, columns, alias, params, work)?;
            let part = equal_key(&v, work)?;
            work.consume(part.len())?;
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
                eval(node, row, columns, alias, params, work)?
            } else {
                Scalar::Null
            };
            state.add(spec, value, work)?;
        }
        Ok(())
    }
    pub(super) fn finish(
        mut self,
        complete: bool,
        params: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<Source> {
        if self.order.is_empty() && self.keys.is_empty() {
            self.create(String::new(), vec![], work)?;
        }
        let mut rows = vec![];
        for group in self.order {
            work.spend()?;
            let mut row = group.values;
            for (spec, state) in self.specs.iter().zip(group.states) {
                row.push(state.result(spec, work)?);
            }
            if let Some(node) = &self.having
                && truth(&eval(node, &row, &self.columns, "", params, work)?)? != Some(true)
            {
                continue;
            }
            work.retain(row_cost(&row)? + 24)?;
            rows.push(row);
        }
        Ok(Source {
            columns: self.columns,
            rows,
            complete,
        })
    }
}
