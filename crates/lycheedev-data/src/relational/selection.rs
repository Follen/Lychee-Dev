//! Go 3.2.3 bare numeric IN selection over explicit query-scoped providers.
//! The legacy opt-in owned-row adapter retains its independently charged index;
//! the default SQL WDC/effective route uses existing provider Equal directly.
use super::*;
use crate::resource::{Budget, RetainedReservation};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Null,
    Number {
        negative: bool,
        significand: u64,
        exponent: i16,
    },
}
impl Key {
    fn number(negative: bool, significand: u64, exponent: i16) -> Self {
        if significand == 0 {
            return Self::Number {
                negative: false,
                significand: 0,
                exponent: 0,
            };
        }
        let zeros = significand.trailing_zeros();
        Self::Number {
            negative,
            significand: significand >> zeros,
            exponent: exponent + zeros as i16,
        }
    }
    fn of(value: &Scalar) -> Result<Self> {
        Ok(match value {
            Scalar::Null => Self::Null,
            Scalar::Int(n) => Self::number(*n < 0, n.unsigned_abs(), 0),
            Scalar::UInt(n) => Self::number(false, *n, 0),
            Scalar::Float(n) if n.is_finite() => {
                let bits = n.to_bits();
                let exponent = ((bits >> 52) & 0x7ff) as i16;
                let fraction = bits & ((1u64 << 52) - 1);
                let (significand, exponent) = if exponent == 0 {
                    (fraction, -1074)
                } else {
                    (fraction | (1u64 << 52), exponent - 1023 - 52)
                };
                Self::number(bits >> 63 != 0, significand, exponent)
            }
            Scalar::Float(_) => return Err(numeric()),
            _ => return Err(type_error()),
        })
    }
    // Length of Go's "n" + big.Rat.RatString(), without heap-allocating a
    // rational or decimal string. A finite f64 requires at most 324 digits.
    fn go_len(self) -> usize {
        match self {
            Self::Null => 1,
            Self::Number {
                negative,
                significand,
                exponent,
            } => {
                1 + usize::from(negative)
                    + if exponent >= 0 {
                        decimal_digits(significand, exponent as u16)
                    } else {
                        decimal_digits(significand, 0) + 1 + decimal_digits(1, (-exponent) as u16)
                    }
            }
        }
    }
}
fn decimal_digits(mut value: u64, power: u16) -> usize {
    const BASE: u64 = 1_000_000_000;
    let mut limbs = [0u32; 36];
    let mut used = 0;
    loop {
        limbs[used] = (value % BASE) as u32;
        used += 1;
        value /= BASE;
        if value == 0 {
            break;
        }
    }
    for _ in 0..power {
        let mut carry = 0;
        for limb in &mut limbs[..used] {
            let next = u64::from(*limb) * 2 + carry;
            *limb = (next % BASE) as u32;
            carry = next / BASE;
        }
        if carry != 0 {
            limbs[used] = carry as u32;
            used += 1;
        }
    }
    9 * (used - 1)
        + if limbs[used - 1] == 0 {
            1
        } else {
            limbs[used - 1].ilog10() as usize + 1
        }
}

struct Temporary<'a> {
    allowance: usize,
    charged: usize,
    shared: Option<&'a Budget>,
    reservation: Option<RetainedReservation<'a>>,
}
impl<'a> Temporary<'a> {
    fn new(work: &Work<'a>) -> Self {
        Self {
            allowance: work.bytes / 2,
            charged: 0,
            shared: work.shared,
            reservation: None,
        }
    }
    fn reserve(&mut self, bytes: usize, safety_bytes: usize, work: &mut Work<'_>) -> Result<bool> {
        // Only this optional local allowance permits fallback. An independent
        // safety-ledger failure or stop is always propagated, never retried.
        work.consume(0)?;
        if bytes > self.allowance - self.charged {
            return Ok(false);
        }
        if let Some(shared) = self.shared {
            if let Some(reservation) = &mut self.reservation {
                reservation.grow(safety_bytes)?;
            } else {
                self.reservation = Some(shared.reserve_retained(safety_bytes)?);
            }
        }
        work.bytes -= bytes;
        self.charged += bytes;
        Ok(true)
    }
}

type Yield<'a> = dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()> + 'a;
trait Equality {
    fn finish(&mut self) {}
    fn prepare(
        &mut self,
        column: usize,
        temporary: &mut Temporary<'_>,
        work: &mut Work<'_>,
    ) -> Result<bool>;
    fn equal(
        &mut self,
        column: usize,
        value: &Scalar,
        work: &mut Work<'_>,
        emit: &mut Yield<'_>,
    ) -> Result<bool>;
}
struct Rows<'a> {
    source: &'a Source,
    identity_column: usize,
    buckets: BTreeMap<Key, Vec<usize>>,
}
impl Equality for Rows<'_> {
    fn finish(&mut self) {
        self.buckets = BTreeMap::new();
    }
    fn prepare(
        &mut self,
        column: usize,
        temporary: &mut Temporary<'_>,
        work: &mut Work<'_>,
    ) -> Result<bool> {
        if column != self.identity_column {
            return Ok(false);
        }
        // Conservative index admission includes map nodes, spare vector
        // capacities and ordinal storage, before any index allocation.
        let bytes = self
            .source
            .rows
            .len()
            .checked_mul(256)
            .and_then(|n| n.checked_add(4096))
            .ok_or_else(budget)?;
        if !temporary.reserve(bytes, bytes, work)? {
            return Ok(false);
        }
        for (ordinal, row) in self.source.rows.iter().enumerate() {
            work.spend()?;
            if row.len() != self.source.columns.len() {
                return Err(binding());
            }
            row_cost(row)?;
            let key = match Key::of(&row[column]) {
                Ok(key) => key,
                // A heterogeneous source keeps its original scan/type-error
                // behavior. Do not silently hide incompatible unmatched rows.
                Err(error) if error.code == "relational.type_mismatch" => return Ok(false),
                Err(error) => return Err(error),
            };
            if key != Key::Null {
                self.buckets.entry(key).or_default().push(ordinal);
            }
        }
        Ok(true)
    }
    fn equal(
        &mut self,
        _: usize,
        value: &Scalar,
        work: &mut Work<'_>,
        emit: &mut Yield<'_>,
    ) -> Result<bool> {
        work.spend()?;
        let measurement = work.scan_index(self.source);
        work.scan_call(measurement, true);
        if let Some(ordinals) = self.buckets.get(&Key::of(value)?) {
            for ordinal in ordinals {
                work.scan_row(measurement);
                emit(&self.source.rows[*ordinal], work)?;
            }
        }
        Ok(true)
    }
}
fn supported(term: &Term) -> bool {
    match term {
        Term::Number(_) | Term::Parameter(_) | Term::Value(Scalar::Null | Scalar::Bool(_)) => true,
        Term::Unary(UnaryOperator::Plus | UnaryOperator::Minus, value) => {
            matches!(value.as_ref(), Term::Number(_))
        }
        _ => false,
    }
}
const BUFFER_FULL: &str = "relational.selection_buffer_limit";
fn collect<P: Equality>(
    filter: Option<&Term>,
    columns: &[String],
    alias: ColumnOwners<'_>,
    parameters: &BTreeMap<String, Scalar>,
    provider: &mut P,
    work: &mut Work<'_>,
    yield_row: &mut Yield<'_>,
) -> Result<bool> {
    let Some(Term::In(input, items, false)) = filter else {
        return Ok(false);
    };
    let Term::Column(name) = input.as_ref() else {
        return Ok(false);
    };
    if items.is_empty() {
        return Ok(false);
    }
    // A correlated column belongs to an enclosing row, not this provider's
    // identity. The original predicate will evaluate it during the scan.
    let Ok(column) = column(columns, alias, name) else {
        return Ok(false);
    };
    let mut temporary = Temporary::new(work);
    let result = (|| {
        let mut values = Vec::new();
        let mut seen = BTreeSet::new();
        for term in items {
            if !supported(term) {
                return Ok(false);
            }
            // Go evaluates constants/parameters before declining nonnumeric
            // types. Charge that node without cloning an unsupported string;
            // otherwise a speculative clone could change an empty scan's error.
            let unsupported_scalar = match term {
                Term::Value(Scalar::Bool(_)) => true,
                Term::Parameter(name) => parameters
                    .get(name)
                    .is_some_and(|value| matches!(value, Scalar::Bool(_) | Scalar::String(_))),
                _ => false,
            };
            if unsupported_scalar {
                work.spend()?;
                return Ok(false);
            }
            let value = match eval(term, &[], columns, alias, parameters, work) {
                Ok(value) => value,
                Err(error) if error.code == "relational.numeric_range" => {
                    // An invalid literal only fails if the original scan
                    // reaches a row. A concurrent stop still takes priority.
                    work.consume(0)?;
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
            let key = Key::of(&value)?;
            if !seen.contains(&key) {
                let bytes = 96 + key.go_len();
                if !temporary.reserve(bytes, bytes.max(512), work)? {
                    return Ok(false);
                }
                seen.insert(key);
                values.push(value);
            }
        }
        if !provider.prepare(column, &mut temporary, work)? {
            work.consume(0)?;
            return Ok(false);
        }
        let mut rows = Vec::new();
        for value in &values {
            let handled = provider.equal(column, value, work, &mut |row, work| {
                work.spend()?;
                if row.len() != columns.len() {
                    return Err(binding());
                }
                let bytes = row_cost(row)?.checked_add(40).ok_or_else(budget)?;
                let safety_bytes = bytes.checked_add(96).ok_or_else(budget)?;
                if !temporary.reserve(bytes, safety_bytes, work)? {
                    return Err(fault(
                        BUFFER_FULL,
                        3,
                        "Optional IN candidate buffer is full",
                    ));
                }
                rows.push(row.to_vec());
                Ok(())
            });
            match handled {
                Err(error) if error.code == BUFFER_FULL => {
                    work.consume(0)?;
                    return Ok(false);
                }
                Err(error) => return Err(error),
                Ok(false) => {
                    work.consume(0)?;
                    return Ok(false);
                }
                Ok(true) => {}
            }
        }
        // Candidate order is first distinct IN value, then provider row order.
        // Sorting happens only in the caller's explicit ORDER BY path. Keep
        // the original predicate there as the final filter, including NULL.
        for row in rows {
            yield_row(&row, work)?;
        }
        Ok(true)
    })();
    // Refund only this adapter's exact local admission. Output allocations
    // made by the callback remain charged, including on errors. The shared
    // safety reservation similarly releases only its own admitted bytes.
    provider.finish();
    work.bytes += temporary.charged;
    result
}

/// The provider already owns its physical lookup. Preparation must not build
/// an index, scan records, or decline before candidate evaluation has completed.
struct Direct<'a> {
    provider: &'a dyn RowProvider,
    source: &'a Source,
}
impl Equality for Direct<'_> {
    fn prepare(&mut self, _: usize, _: &mut Temporary<'_>, _: &mut Work<'_>) -> Result<bool> {
        Ok(true)
    }
    fn equal(
        &mut self,
        column: usize,
        value: &Scalar,
        work: &mut Work<'_>,
        emit: &mut Yield<'_>,
    ) -> Result<bool> {
        provider::equal(
            self.source,
            self.provider,
            &self.source.columns[column],
            value,
            work,
            emit,
        )
        .map(|outcome| outcome == EqualityOutcome::Handled)
    }
}
fn equality(
    source: &Source,
    filter: Option<&Term>,
    alias: ColumnOwners<'_>,
    parameters: &BTreeMap<String, Scalar>,
    provider: &dyn RowProvider,
    work: &mut Work<'_>,
    emit: &mut Yield<'_>,
) -> Result<bool> {
    let Some(Term::Binary(left, BinaryOperator::Eq, right)) = filter else {
        return Ok(false);
    };
    let (column_term, value) = if matches!(left.as_ref(), Term::Column(_)) {
        (left.as_ref(), right.as_ref())
    } else {
        (right.as_ref(), left.as_ref())
    };
    let Term::Column(name) = column_term else {
        return Ok(false);
    };
    if !matches!(value, Term::Number(_) | Term::Value(_) | Term::Parameter(_)) {
        return Ok(false);
    }
    let Ok(index) = column(&source.columns, alias, name) else {
        return Ok(false);
    };
    let value = eval(value, &[], &source.columns, alias, parameters, work)?;
    let result = provider::equal(source, provider, &source.columns[index], &value, work, emit)?;
    // A decline never erases a stop observed at its boundary.
    if result == EqualityOutcome::Declined {
        work.consume(0)?;
    }
    Ok(result == EqualityOutcome::Handled)
}

pub(super) fn visit(
    source: &Source,
    selected: Option<&[usize]>,
    filter: Option<&Term>,
    alias: ColumnOwners<'_>,
    parameters: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
    yield_row: &mut Yield<'_>,
) -> Result<()> {
    if let Some(provider) = provider::find(source, work) {
        // No capability means no speculative candidate evaluation at all.
        if provider.has_equal() {
            if equality(source, filter, alias, parameters, provider, work, yield_row)? {
                return Ok(());
            }
            let mut direct = Direct { provider, source };
            if collect(
                filter,
                &source.columns,
                alias,
                parameters,
                &mut direct,
                work,
                yield_row,
            )? {
                return Ok(());
            }
        }
        return provider::visit(source, selected, work, yield_row);
    }
    let identity_column = work
        .selection_sources
        .iter()
        .find_map(|(prepared, column)| std::ptr::eq(*prepared, source).then_some(*column));
    let Some(identity_column) = identity_column else {
        return provider::visit(source, selected, work, yield_row);
    };
    let mut provider = Rows {
        source,
        identity_column,
        buckets: BTreeMap::new(),
    };
    let handled = collect(
        filter,
        &source.columns,
        alias,
        parameters,
        &mut provider,
        work,
        yield_row,
    )?;
    // Drop optional index allocations before starting a fallback scan.
    drop(provider);
    if !handled {
        provider::visit(source, selected, work, yield_row)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
