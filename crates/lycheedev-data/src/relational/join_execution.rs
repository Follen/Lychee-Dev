//! Pausable JOIN DFS. Right inputs retain the original materialization policy;
//! the first input remains one opened cursor with a live current-row lease.
use super::*;
use crate::relational::execution::{
    Context, Control, Fields, Owners, Query, Row, Shared, SourceRef,
};
use crate::relational::execution_scan::Scan;
use std::cell::RefCell;
struct Right<'a> {
    source: SourceRef<'a>,
    selected: Vec<usize>,
    start: usize,
    end: usize,
    index: usize,
}
pub(super) struct Plan<'a> {
    pub fields: Fields<'a>,
    query: Query<'a>,
    first: SourceRef<'a>,
    first_selected: Vec<usize>,
    projected: bool,
    rights: Vec<Right<'a>>,
    selected: Vec<bool>,
}
pub(super) async fn prepare<'a, 'w: 'a>(
    query: Query<'a>,
    inputs: &[SourceRef<'a>],
    context: &Context<'a>,
    control: &Control<'a, 'w>,
) -> Result<Plan<'a>> {
    let first = inputs.first().ok_or_else(binding)?.clone();
    control.work(|w| {
        w.safety_retain(
            first
                .columns
                .iter()
                .map(|c| query.alias.len() + c.len() + 2 * std::mem::size_of::<String>())
                .sum(),
        )
    })?;
    let mut columns = first.columns.clone();
    let mut owners = vec![query.alias.clone(); columns.len()];
    control.work(|w| w.safety_retain(query.joins.len() * std::mem::size_of::<Right<'_>>()))?;
    let mut rights = Vec::with_capacity(query.joins.len());
    let mut complete = first.complete;
    for (index, link) in query.joins.iter().enumerate() {
        let right = inputs.get(index + 1).ok_or_else(binding)?.clone();
        let start = columns.len();
        complete &= right.complete;
        control.work(|w| {
            w.safety_retain(
                right
                    .columns
                    .iter()
                    .map(|c| link.alias.len() + c.len() + 2 * std::mem::size_of::<String>())
                    .sum(),
            )
        })?;
        columns.extend(right.columns.iter().cloned());
        owners.extend(std::iter::repeat_n(link.alias.clone(), right.columns.len()));
        rights.push(Right {
            source: right,
            selected: Vec::new(),
            start,
            end: columns.len(),
            index,
        });
    }
    let source = SourceRef::Owned(control.share(Source {
        columns,
        rows: Vec::new(),
        complete,
    })?);
    let owners = control.share(owners)?;
    let fields = Fields::new(source, Owners::Columns(owners));
    for right in &rights {
        if let Some(condition) = &query.joins[right.index].condition {
            binding::bind_async(
                condition,
                &fields.prefix(right.end),
                false,
                context,
                control,
            )
            .await?;
        }
    }
    let width = fields.view().columns.len();
    control.work(|w| {
        w.safety_retain(
            width
                * (std::mem::size_of::<Scalar>()
                    + std::mem::size_of::<String>()
                    + std::mem::size_of::<usize>()),
        )?;
        w.safety_retain(width)
    })?;
    let mut selected = vec![false; width];
    let view = fields.view();
    for p in &query.projection {
        if let Some(expr) = &p.expr {
            select(expr, view.columns, view.alias, &mut selected);
        } else {
            for (i, v) in selected.iter_mut().enumerate() {
                if p.name.is_empty() || equal_fold(view.alias.owner(i), &p.name) {
                    *v = true;
                }
            }
        }
    }
    for expr in query
        .filter
        .iter()
        .chain(query.order.iter().map(|(e, _, _)| e))
        .chain(query.groups.iter())
        .chain(query.having.iter())
    {
        select(expr, view.columns, view.alias, &mut selected);
    }
    for right in &rights {
        if let Some(condition) = &query.joins[right.index].condition {
            select(
                condition,
                &view.columns[..right.end],
                view.alias,
                &mut selected[..right.end],
            );
        }
    }
    let first_selected = selected[..first.columns.len()]
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.then_some(i))
        .collect();
    for right in &mut rights {
        right.selected = selected[right.start..right.end]
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.then_some(i))
            .collect();
    }
    let projected = provider::can_project(&query, view.columns, view.alias)
        && rights.iter().all(|right| {
            query.joins[right.index]
                .condition
                .as_ref()
                .is_none_or(|c| provider::projectable(c, &view.columns[..right.end], view.alias))
        });
    Ok(Plan {
        fields,
        query,
        first,
        first_selected,
        projected,
        rights,
        selected,
    })
}
struct Position<'a> {
    key: Option<Shared<'a, String>>,
    count: usize,
    next: usize,
    matched: bool,
    fallback: bool,
}
pub(super) struct Cursor<'c, 'a, 'w: 'a> {
    plan: Plan<'a>,
    materialized: Vec<(Vec<Vec<Scalar>>, Option<KeyIndex<'w>>)>,
    positions: Vec<Position<'a>>,
    values: Shared<'a, RefCell<Vec<Scalar>>>,
    strings: Vec<String>,
    first: Scan<'c, 'a, 'w>,
    enter: bool,
    active: bool,
    _allocation: Option<crate::resource::RetainedReservation<'w>>,
}
impl<'c, 'a, 'w: 'a> Cursor<'c, 'a, 'w> {
    pub fn new(plan: Plan<'a>, control: &'c Control<'a, 'w>) -> Result<Self> {
        let mut materialized = Vec::with_capacity(plan.rights.len());
        for right in &plan.rights {
            let mut rows = Vec::new();
            control.work(|work| {
                provider::visit(
                    &right.source,
                    plan.projected.then_some(right.selected.as_slice()),
                    work,
                    &mut |row, work| {
                        work.spend()?;
                        if row.len() != right.source.columns.len() {
                            return Err(binding());
                        }
                        let bytes = row_cost(row)?.checked_add(40).ok_or_else(budget)?;
                        work.retain(bytes)?;
                        rows.push(row.to_vec());
                        Ok(())
                    },
                )
            })?;
            let fields = plan.fields.view();
            let keys = control.work(|work| {
                index(
                    plan.query.joins[right.index].condition.as_ref(),
                    &fields.columns[..right.end],
                    match fields.alias {
                        ColumnOwners::PerColumn(o) => o,
                        _ => return Err(binding()),
                    },
                    right.start,
                    &rows,
                    work,
                )
            })?;
            materialized.push((rows, keys));
        }
        let bytes = plan
            .rights
            .len()
            .checked_mul(std::mem::size_of::<Position<'a>>())
            .ok_or_else(budget)?;
        let allocation = control
            .shared_budget()
            .map(|b| b.reserve_retained(bytes))
            .transpose()?;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(plan.rights.len())
            .map_err(|_| budget())?;
        let values = control.share(RefCell::new(vec![Scalar::Null; plan.selected.len()]))?;
        let strings = vec![String::new(); plan.selected.len()];
        let first = Scan::open(
            plan.first.clone(),
            plan.projected.then_some(plan.first_selected.as_slice()),
            control,
        )?;
        Ok(Self {
            plan,
            materialized,
            positions,
            values,
            strings,
            first,
            enter: false,
            active: false,
            _allocation: allocation,
        })
    }
    pub async fn next(
        &mut self,
        context: &Context<'a>,
        control: &Control<'a, 'w>,
    ) -> Result<Option<Row<'a>>> {
        loop {
            if !self.active {
                let Some(row) = self.first.next()? else {
                    return Ok(None);
                };
                row.with(|row| {
                    control.work(|w| {
                        if row.len() != self.plan.first.columns.len() {
                            return Err(binding());
                        }
                        assign(
                            &mut self.values.borrow_mut(),
                            &mut self.strings,
                            0,
                            row,
                            w,
                            &self.plan.selected,
                        )
                    })
                })?;
                self.positions.clear();
                self.enter = true;
                self.active = true;
            }
            if self.enter {
                control.work(|w| w.spend())?;
                let at = self.positions.len();
                self.enter = false;
                if at == self.plan.rights.len() {
                    return Ok(Some(Row::Mutable(self.values.clone())));
                }
                let (rows, index) = &self.materialized[at];
                let mut key = None;
                let count = if let Some(index) = index {
                    let values = self.values.borrow();
                    let value = &values[index.left];
                    if value == &Scalar::Null || index.example.is_none() {
                        0
                    } else {
                        compare(value, &rows[index.example.unwrap()][index.right])?;
                        let payload = if let Scalar::String(s) = value {
                            s.len()
                        } else {
                            0
                        };
                        let temporary = control
                            .shared_budget()
                            .map(|b| b.reserve_retained(payload + 8192))
                            .transpose()?;
                        let text = results::equal_key(value)?;
                        let count = index.buckets.get(&text).map_or(0, Vec::len);
                        if count > 0 {
                            key = Some(control.own(text.len(), || text)?);
                        }
                        drop(temporary);
                        count
                    }
                } else {
                    rows.len()
                };
                self.positions.push(Position {
                    key,
                    count,
                    next: 0,
                    matched: false,
                    fallback: false,
                });
            }
            let Some(at) = self.positions.len().checked_sub(1) else {
                self.active = false;
                continue;
            };
            let position = &mut self.positions[at];
            let right = &self.plan.rights[at];
            let link = &self.plan.query.joins[right.index];
            if position.next < position.count {
                control.work(|w| w.spend())?;
                let ordinal = if let Some(key) = &position.key {
                    self.materialized[at]
                        .1
                        .as_ref()
                        .ok_or_else(binding)?
                        .buckets
                        .get(key.as_str())
                        .ok_or_else(binding)?[position.next]
                } else {
                    position.next
                };
                position.next += 1;
                control.work(|w| {
                    assign(
                        &mut self.values.borrow_mut(),
                        &mut self.strings,
                        right.start,
                        &self.materialized[at].0[ordinal],
                        w,
                        &self.plan.selected,
                    )
                })?;
                let yes = if let Some(condition) = &link.condition {
                    // ON owns a temporary scope even if it rejects this
                    // candidate and no output row ever reaches executeRows.
                    // Child query suspension keeps it live only until truth
                    // conversion completes; errors/cancellation drop it too.
                    let _temporary = control.temporary_scope();
                    let fields = self.plan.fields.prefix(right.end);
                    let mut context = context.clone();
                    context.scope.fields = fields.clone();
                    truth(
                        &evaluation::eval_async(
                            condition,
                            &Row::Mutable(self.values.clone()),
                            &fields,
                            &context,
                            control,
                        )
                        .await?,
                    )? == Some(true)
                } else {
                    true
                };
                if yes {
                    self.positions[at].matched = true;
                    self.enter = true;
                }
            } else if link.left && !position.matched && !position.fallback {
                position.fallback = true;
                let mut values = self.values.borrow_mut();
                for i in right.start..right.end {
                    if let Scalar::String(text) = std::mem::replace(&mut values[i], Scalar::Null) {
                        self.strings[i] = text;
                    }
                }
                self.enter = true;
            } else {
                self.positions.pop();
            }
        }
    }
}
