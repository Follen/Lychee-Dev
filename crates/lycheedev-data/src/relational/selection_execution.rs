//! Original optional IN buffer with a pausable output phase. Candidate lookup
//! remains synchronous and cannot enter a query; no callback replay is used.
use super::*;
use crate::relational::execution::{Control, Fields, Row, SourceRef};
use crate::relational::execution_scan::Scan;
fn prepare_collect<'w, P: Equality>(
    filter: Option<&Term>,
    columns: &[String],
    alias: ColumnOwners<'_>,
    parameters: &BTreeMap<String, Scalar>,
    provider: &mut P,
    work: &mut Work<'w>,
) -> Result<Option<(Vec<Vec<Scalar>>, Temporary<'w>)>> {
    let Some(Term::In(input, items, false)) = filter else {
        return Ok(None);
    };
    let Term::Column(name) = input.as_ref() else {
        return Ok(None);
    };
    if items.is_empty() {
        return Ok(None);
    }
    // Fixed Go selection inspects the original unbound filter. An outer
    // column is not an inner lookup key: its fieldIndex failure is terminal,
    // before evaluating candidates or invoking this source's producer.
    let column = column(columns, alias, name)?;
    let mut temporary = Temporary::new(work);
    let result = (|| -> Result<Option<Vec<Vec<Scalar>>>> {
        let mut values = Vec::new();
        let mut seen = BTreeSet::new();
        for term in items {
            if !supported(term) {
                return Ok(None);
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
                return Ok(None);
            }
            let value = match eval(term, &[], columns, alias, parameters, work) {
                Ok(value) => value,
                Err(error) if error.code == "relational.numeric_range" => {
                    // An invalid literal only fails if the original scan
                    // reaches a row. A concurrent stop still takes priority.
                    work.consume(0)?;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            let key = Key::of(&value)?;
            if !seen.contains(&key) {
                let bytes = 96 + key.go_len();
                if !temporary.reserve(bytes, bytes.max(512), work)? {
                    return Ok(None);
                }
                seen.insert(key);
                values.push(value);
            }
        }
        if !provider.prepare(column, &mut temporary, work)? {
            work.consume(0)?;
            return Ok(None);
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
                    return Ok(None);
                }
                Err(error) => return Err(error),
                Ok(false) => {
                    work.consume(0)?;
                    return Ok(None);
                }
                Ok(true) => {}
            }
        }
        Ok(Some(rows))
    })();
    match result {
        Ok(Some(rows)) => Ok(Some((rows, temporary))),
        result => {
            provider.finish();
            work.bytes += temporary.charged;
            result.map(|_| None)
        }
    }
}
pub(in crate::relational) struct Buffer<'c, 'a, 'w: 'a> {
    rows: std::vec::IntoIter<Vec<Scalar>>,
    index: BTreeMap<Key, Vec<usize>>,
    temporary: Temporary<'w>,
    control: &'c Control<'a, 'w>,
}
impl Drop for Buffer<'_, '_, '_> {
    fn drop(&mut self) {
        self.rows = Vec::new().into_iter();
        self.index.clear();
        let charged = std::mem::take(&mut self.temporary.charged);
        let _ = self.control.work(|w| {
            w.bytes += charged;
            Ok(())
        });
    }
}
pub(in crate::relational) enum Cursor<'c, 'a, 'w: 'a> {
    Scan(Scan<'c, 'a, 'w>),
    Buffer(Buffer<'c, 'a, 'w>),
}
impl<'c, 'a, 'w: 'a> Cursor<'c, 'a, 'w> {
    pub(in crate::relational) fn next(&mut self) -> Result<Option<Row<'a>>> {
        match self {
            Self::Scan(scan) => scan.next(),
            Self::Buffer(buffer) => buffer
                .rows
                .next()
                .map(|row| buffer.control.share(row).map(Row::Values))
                .transpose(),
        }
    }
}
pub(in crate::relational) fn open<'c, 'a, 'w: 'a>(
    source: SourceRef<'a>,
    selected: Option<&[usize]>,
    filter: Option<&Term>,
    fields: &Fields<'a>,
    control: &'c Control<'a, 'w>,
) -> Result<Cursor<'c, 'a, 'w>> {
    let provider = control.work(|w| Ok(provider::find(&source, w)))?;
    let fields = fields.view();
    if let Some(provider) = provider {
        if provider.has_equal() {
            if let Some(Term::Binary(left, BinaryOperator::Eq, right)) = filter {
                let (column_term, value) = if matches!(left.as_ref(), Term::Column(_)) {
                    (left.as_ref(), right.as_ref())
                } else {
                    (right.as_ref(), left.as_ref())
                };
                if let Term::Column(name) = column_term
                    && matches!(value, Term::Number(_) | Term::Value(_) | Term::Parameter(_))
                {
                    let index = column(&source.columns, fields.alias, name)?;
                    let value = control.work(|w| {
                        eval(
                            value,
                            &[],
                            &source.columns,
                            fields.alias,
                            control.parameters(),
                            w,
                        )
                    })?;
                    if let Some(scan) =
                        Scan::equal(&source, provider, &source.columns[index], &value, control)?
                    {
                        return Ok(Cursor::Scan(scan));
                    }
                    control.work(|w| w.consume(0))?;
                }
            }
            let mut direct = Direct {
                provider,
                source: &source,
            };
            if let Some((rows, temporary)) = control.work(|w| {
                prepare_collect(
                    filter,
                    &source.columns,
                    fields.alias,
                    control.parameters(),
                    &mut direct,
                    w,
                )
            })? {
                return Ok(Cursor::Buffer(Buffer {
                    rows: rows.into_iter(),
                    index: BTreeMap::new(),
                    temporary,
                    control,
                }));
            }
        }
    } else {
        // The explicit legacy capability always refers to a stable physical
        // Source in Work, not a query-owned derived/CTE result.
        let identity = control.work(|w| {
            Ok(w.selection_sources
                .iter()
                .find_map(|(s, i)| std::ptr::eq(*s, &*source).then_some((*s, *i))))
        })?;
        if let Some((stable, identity_column)) = identity {
            let mut rows = Rows {
                source: stable,
                identity_column,
                buckets: BTreeMap::new(),
            };
            let prepared = control.work(|w| {
                prepare_collect(
                    filter,
                    &source.columns,
                    fields.alias,
                    control.parameters(),
                    &mut rows,
                    w,
                )
            })?;
            if let Some((buffer, temporary)) = prepared {
                return Ok(Cursor::Buffer(Buffer {
                    rows: buffer.into_iter(),
                    index: std::mem::take(&mut rows.buckets),
                    temporary,
                    control,
                }));
            }
        }
    }
    Scan::open(source, selected, control).map(Cursor::Scan)
}
