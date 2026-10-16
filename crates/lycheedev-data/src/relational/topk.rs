//! Bounded stable selection, followed by heap extraction. All input rows and
//! order-key types are checked, including LIMIT 0 and discarded candidates.
use super::*;
type Output = (Vec<Scalar>, Vec<Scalar>);
struct Ranked {
    row: Output,
    ordinal: usize,
    charged: usize,
}
pub(super) struct TopRows {
    rows: Vec<Ranked>,
    size: usize,
    next: usize,
    order: Option<(bool, Option<bool>)>,
    example: Option<Scalar>,
}
impl TopRows {
    pub(super) fn new(
        program: &Program,
        grouped: bool,
        limit: usize,
        offset: usize,
    ) -> Option<Self> {
        if grouped || program.distinct || program.limit.is_none() || program.order.len() > 1 {
            return None;
        }
        let size = offset.checked_add(limit).filter(|n| *n <= 1_000_000)?;
        Some(Self {
            rows: vec![],
            size,
            next: 0,
            order: program.order.first().map(|(_, d, n)| (*d, *n)),
            example: None,
        })
    }
    fn worse(&self, a: &Ranked, b: &Ranked, work: &mut Work<'_>) -> Result<bool> {
        let order = if let Some((desc, nulls)) = self.order {
            work.spend()?;
            ordering(&a.row.1[0], &b.row.1[0], desc, nulls)?
        } else {
            Ordering::Equal
        };
        Ok(order == Ordering::Greater || order == Ordering::Equal && a.ordinal > b.ordinal)
    }
    fn sift(&mut self, mut at: usize, work: &mut Work<'_>) -> Result<()> {
        loop {
            let mut child = at * 2 + 1;
            if child >= self.rows.len() {
                return Ok(());
            }
            if child + 1 < self.rows.len()
                && self.worse(&self.rows[child + 1], &self.rows[child], work)?
            {
                child += 1;
            }
            if !self.worse(&self.rows[child], &self.rows[at], work)? {
                return Ok(());
            }
            self.rows.swap(at, child);
            at = child;
        }
    }
    pub(super) fn add(&mut self, row: Output, work: &mut Work<'_>) -> Result<()> {
        let ordinal = self.next;
        self.next = self.next.checked_add(1).ok_or_else(budget)?;
        if let Some(value) = row.1.first().filter(|v| **v != Scalar::Null) {
            if let Some(example) = &self.example {
                compare(example, value)?;
            } else {
                // Only scalar comparability is needed. Keep a zero-allocation
                // representative rather than a second owned copy of text.
                self.example = Some(match value {
                    Scalar::String(_) => Scalar::String(String::new()),
                    Scalar::Bool(_) => Scalar::Bool(false),
                    _ => Scalar::Int(0),
                });
            }
        }
        if self.size == 0 {
            return Ok(());
        }
        let charged = results::output_cost(&row, 128)?;
        let mut candidate = Ranked {
            row,
            ordinal,
            charged,
        };
        if self.rows.len() < self.size {
            work.retain(charged)?;
            self.rows.push(candidate);
            let mut at = self.rows.len() - 1;
            while at > 0 {
                let parent = (at - 1) / 2;
                if !self.worse(&self.rows[at], &self.rows[parent], work)? {
                    break;
                }
                self.rows.swap(at, parent);
                at = parent;
            }
        } else if self.worse(&self.rows[0], &candidate, work)? {
            // A retained slot is reused. The local allowance charges only
            // growth; the independent safety ledger still admits allocations.
            candidate.charged = candidate.charged.max(self.rows[0].charged);
            work.retain(candidate.charged - self.rows[0].charged)?;
            self.rows[0] = candidate;
            self.sift(0, work)?;
        }
        Ok(())
    }
    pub(super) fn finish(mut self, work: &mut Work<'_>) -> Result<Vec<Output>> {
        work.safety_retain(
            self.rows
                .len()
                .checked_mul(std::mem::size_of::<Output>())
                .ok_or_else(budget)?,
        )?;
        let mut out = Vec::with_capacity(self.rows.len());
        while !self.rows.is_empty() {
            work.consume(0)?;
            let last = self.rows.swap_remove(0);
            out.push(last.row);
            if !self.rows.is_empty() {
                self.sift(0, work)?;
            }
        }
        out.reverse();
        Ok(out)
    }
}
pub(super) fn ordering(
    a: &Scalar,
    b: &Scalar,
    desc: bool,
    nulls: Option<bool>,
) -> Result<Ordering> {
    if a == &Scalar::Null && b == &Scalar::Null {
        return Ok(Ordering::Equal);
    }
    if a == &Scalar::Null || b == &Scalar::Null {
        return Ok(if (a == &Scalar::Null) == nulls.unwrap_or(!desc) {
            Ordering::Less
        } else {
            Ordering::Greater
        });
    }
    let order = compare(a, b)?;
    Ok(if desc { order.reverse() } else { order })
}
