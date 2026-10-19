//! Fixed Go results.go: all projections and order expressions complete before
//! DISTINCT, stable ordering and pagination. Logical charges follow Go; Rust
//! temporary key ownership is admitted separately and never refunds Go work.
use super::*;
use std::fmt::Write;
pub(super) type Output = (Vec<Scalar>, Vec<Scalar>);
pub(super) fn equal_key(value: &Scalar) -> Result<String> {
    Ok(match value {
        Scalar::Null => "z".into(),
        Scalar::Bool(value) => format!("b{}", u8::from(*value)),
        Scalar::String(value) => {
            if value.len() > 1 << 20 {
                return Err(budget());
            }
            format!("s{value}")
        }
        _ => format!("n{}", rational(value)?),
    })
}
pub(super) fn output_cost(row: &Output, overhead: usize) -> Result<usize> {
    row_cost(&row.0)?
        .checked_add(row_cost(&row.1)?)
        .and_then(|n| n.checked_sub(48))
        .and_then(|n| n.checked_add(overhead))
        .ok_or_else(budget)
}
pub(super) fn compare_rows(
    order: &[(Term, bool, Option<bool>)],
    a: &Output,
    b: &Output,
    work: &mut Work<'_>,
) -> Result<Ordering> {
    for (i, (_, descending, nulls)) in order.iter().enumerate() {
        work.spend()?;
        let result = topk::ordering(&a.1[i], &b.1[i], *descending, *nulls)?;
        if result != Ordering::Equal {
            return Ok(result);
        }
    }
    Ok(Ordering::Equal)
}
pub(super) fn finish(
    input: Vec<Output>,
    distinct: bool,
    order: &[(Term, bool, Option<bool>)],
    limit: usize,
    offset: usize,
    work: &mut Work<'_>,
) -> Result<Vec<Vec<Scalar>>> {
    work.retain(input.len().checked_mul(96).ok_or_else(budget)?)?;
    let mut rows;
    let mut seen = BTreeSet::new();
    if distinct {
        rows = Vec::with_capacity(input.len());
        for row in input {
            work.spend()?;
            if row.1.len() != order.len() {
                return Err(binding());
            }
            {
                let shared = work.shared;
                let mut allocation = shared.map(|b| b.reserve_retained(0)).transpose()?;
                let mut key = String::new();
                for value in &row.0 {
                    work.spend()?;
                    // A finite binary64 rational fits within this conservative
                    // temporary buffer; text payloads retain their exact bound.
                    let payload = if let Scalar::String(s) = value {
                        s.len()
                    } else {
                        0
                    };
                    let _part = shared
                        .map(|b| b.reserve_retained(payload + 8192))
                        .transpose()?;
                    let part = equal_key(value)?;
                    work.consume(part.len())?;
                    let grow = part.len().checked_add(32).ok_or_else(budget)?;
                    if key.len().checked_add(grow).is_none_or(|n| n > work.bytes) {
                        return Err(budget());
                    }
                    if let Some(a) = &mut allocation {
                        a.grow(grow.checked_mul(2).ok_or_else(budget)?)?;
                    }
                    write!(&mut key, "{}:{part}", part.len()).map_err(|_| budget())?;
                }
                if seen.contains(&key) {
                    continue;
                }
                work.retain(key.len().checked_add(64).ok_or_else(budget)?)?;
                seen.insert(key);
            }
            rows.push(row);
        }
    } else {
        rows = validate_rows(input, order.len(), work)?;
    }
    if !order.is_empty() {
        let mut buffer = Vec::with_capacity(rows.len());
        buffer.resize_with(rows.len(), || (Vec::new(), Vec::new()));
        let mut width = 1usize;
        while width < rows.len() {
            let mut start = 0;
            while start < rows.len() {
                let mid = (start + width).min(rows.len());
                let end = (mid + width).min(rows.len());
                let (mut left, mut right) = (start, mid);
                for slot in &mut buffer[start..end] {
                    work.spend()?;
                    let take_left = right == end
                        || left < mid
                            && compare_rows(order, &rows[left], &rows[right], work)?
                                != Ordering::Greater;
                    let at = if left < mid && take_left {
                        let at = left;
                        left += 1;
                        at
                    } else {
                        let at = right;
                        right += 1;
                        at
                    };
                    *slot = std::mem::take(&mut rows[at]);
                }
                start = end;
            }
            std::mem::swap(&mut rows, &mut buffer);
            width = width.checked_mul(2).ok_or_else(budget)?;
        }
    }
    Ok(rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|row| row.0)
        .collect())
}

/// Non-DISTINCT output already owns every retained row. Validate in the same
/// order, then transfer that allocation instead of making a second outer Vec.
/// Keep the original logical allowance in finish; retained capacity is the
/// producer's existing capacity, not a new memory/RSS bound.
fn validate_rows(
    rows: Vec<Output>,
    order_width: usize,
    work: &mut Work<'_>,
) -> Result<Vec<Output>> {
    for row in &rows {
        work.spend()?;
        if row.1.len() != order_width {
            return Err(binding());
        }
    }
    Ok(rows)
}

#[cfg(test)]
#[path = "results_tests.rs"]
mod tests;
