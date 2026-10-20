//! Scalar SQL projection over one authenticated Arrow batch per live cursor.
//! The private record locator is not a visible SQL column. No raw Arrow value
//! escapes its batch's refundable owner or remains borrowed by an output row.
use super::*;
use crate::{
    generation::{Generation, GenerationBatch},
    resource::RetainedReservation,
};
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int64Array, StringArray, UInt32Array, UInt64Array,
};
use arrow_schema::DataType;
use relational::{RowCursor, Scalar};
use std::collections::BTreeSet;

struct Row<'a> {
    cells: Vec<Scalar>,
    _allocation: RetainedReservation<'a>,
}
pub(super) struct Cursor<'a> {
    generation: &'a Generation,
    columns: &'a [schema::Field],
    budget: &'a Budget,
    selected: Option<BTreeSet<usize>>,
    batch: Option<GenerationBatch>,
    block: usize,
    row: usize,
    equal: Option<u32>,
    done: bool,
    current: Option<Row<'a>>,
    _allocation: RetainedReservation<'a>,
}
impl<'a> Cursor<'a> {
    pub(super) fn scan(
        generation: &'a Generation,
        columns: &'a [schema::Field],
        budget: &'a Budget,
        selected: Option<BTreeSet<usize>>,
    ) -> Result<Box<dyn RowCursor + 'a>> {
        let allocation = budget.reserve_retained(size_of::<Self>())?;
        Ok(Box::new(Self {
            generation,
            columns,
            budget,
            selected,
            batch: None,
            block: 0,
            row: 0,
            equal: None,
            done: false,
            current: None,
            _allocation: allocation,
        }))
    }
    pub(super) fn equal(
        generation: &'a Generation,
        columns: &'a [schema::Field],
        budget: &'a Budget,
        id: u32,
    ) -> Result<Box<dyn RowCursor + 'a>> {
        let allocation = budget.reserve_retained(size_of::<Self>())?;
        Ok(Box::new(Self {
            generation,
            columns,
            budget,
            selected: None,
            batch: None,
            block: 0,
            row: 0,
            equal: Some(id),
            done: false,
            current: None,
            _allocation: allocation,
        }))
    }
    fn advance_inner(&mut self) -> Result<bool> {
        self.budget.check()?;
        if let Some(id) = self.equal.take() {
            self.done = true;
            let Some(block) = self.generation.locate_batch(id) else {
                return Ok(false);
            };
            self.batch = Some(self.generation.read_batch(block)?);
            let batch = self.batch.as_ref().expect("opened batch");
            let ids = batch
                .records
                .columns()
                .last()
                .and_then(|array| array.as_any().downcast_ref::<UInt32Array>())
                .ok_or_else(invalid)?;
            let Ok(row) = ids.values().binary_search(&id) else {
                return Ok(false);
            };
            self.current = Some(flatten(
                &batch.records,
                row,
                self.columns,
                None,
                self.budget,
            )?);
            return Ok(true);
        }
        while self
            .batch
            .as_ref()
            .is_none_or(|batch| self.row == batch.records.num_rows())
        {
            self.batch = None;
            if self.block == self.generation.batch_count() {
                self.done = true;
                return Ok(false);
            }
            self.batch = Some(self.generation.read_batch(self.block)?);
            self.block += 1;
            self.row = 0;
        }
        let batch = self.batch.as_ref().expect("current batch");
        self.current = Some(flatten(
            &batch.records,
            self.row,
            self.columns,
            self.selected.as_ref(),
            self.budget,
        )?);
        self.row += 1;
        Ok(true)
    }
}
impl RowCursor for Cursor<'_> {
    fn advance(&mut self) -> Result<bool> {
        let budget = self.budget;
        let _phase = budget.phase(Phase::GenerationRow);
        self.current = None;
        if self.done {
            self.batch = None;
            return Ok(false);
        }
        let result = self.advance_inner();
        if result.is_err() {
            self.done = true;
            self.batch = None;
        }
        result
    }
    fn row(&self) -> &[Scalar] {
        self.current
            .as_ref()
            .map_or(&[], |row| row.cells.as_slice())
    }
}
fn invalid() -> lycheedev_core::ToolkitError {
    fault(
        "records.generation_invalid",
        4,
        "Typed SQL batch differs from its authenticated schema",
    )
}
fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "Typed SQL row exceeds its admitted bound",
    )
}
fn cell<'a>(
    array: &'a dyn Array,
    row: usize,
    element: Option<usize>,
) -> Result<Option<(&'a dyn Array, usize)>> {
    if row >= array.len() {
        return Err(invalid());
    }
    if array.is_null(row) {
        return Ok(None);
    }
    match element {
        None => Ok(Some((array, row))),
        Some(element) => {
            let list = array
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .ok_or_else(invalid)?;
            let width = usize::try_from(list.value_length()).map_err(|_| invalid())?;
            if element >= width {
                return Err(invalid());
            }
            let index = row
                .checked_mul(width)
                .and_then(|n| n.checked_add(element))
                .ok_or_else(limit)?;
            let values = list.values().as_ref();
            if index >= values.len() {
                return Err(invalid());
            }
            Ok((!values.is_null(index)).then_some((values, index)))
        }
    }
}
fn text_bytes(value: Option<(&dyn Array, usize)>) -> Result<usize> {
    let Some((array, row)) = value else {
        return Ok(0);
    };
    if array.data_type() == &DataType::Utf8 {
        return Ok(array
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(invalid)?
            .value(row)
            .len());
    }
    Ok(0)
}
fn scalar(value: Option<(&dyn Array, usize)>) -> Result<Scalar> {
    let Some((array, row)) = value else {
        return Ok(Scalar::Null);
    };
    Ok(match array.data_type() {
        DataType::Int64 => Scalar::Int(
            array
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(invalid)?
                .value(row),
        ),
        DataType::UInt64 => Scalar::UInt(
            array
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(invalid)?
                .value(row),
        ),
        DataType::Float32 => {
            let value = array
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(invalid)?
                .value(row);
            if !value.is_finite() {
                return Err(invalid());
            }
            Scalar::Float(f64::from(value))
        }
        DataType::Utf8 => Scalar::String(
            array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(invalid)?
                .value(row)
                .to_owned(),
        ),
        _ => return Err(invalid()),
    })
}
fn flatten<'a>(
    batch: &arrow_array::RecordBatch,
    row: usize,
    fields: &[schema::Field],
    selected: Option<&BTreeSet<usize>>,
    budget: &'a Budget,
) -> Result<Row<'a>> {
    if batch.num_columns() != fields.len() + 1 {
        return Err(invalid());
    }
    let mut count = 0usize;
    let mut strings = 0usize;
    for (field, array) in fields.iter().zip(batch.columns()) {
        budget.check()?;
        let width = if field.array {
            field.elements as usize
        } else {
            1
        };
        for element in 0..width {
            if selected.is_none_or(|set| set.contains(&count)) {
                strings = strings
                    .checked_add(text_bytes(cell(
                        array.as_ref(),
                        row,
                        field.array.then_some(element),
                    )?)?)
                    .ok_or_else(limit)?;
            }
            count = count.checked_add(1).ok_or_else(limit)?;
        }
    }
    let bytes = count
        .checked_mul(size_of::<Scalar>())
        .and_then(|n| n.checked_add(strings))
        .and_then(|n| n.checked_add(64))
        .ok_or_else(limit)?;
    let allocation = budget.reserve_retained(bytes)?;
    let mut cells = Vec::new();
    cells.try_reserve_exact(count).map_err(|_| limit())?;
    for (field, array) in fields.iter().zip(batch.columns()) {
        budget.check()?;
        let width = if field.array {
            field.elements as usize
        } else {
            1
        };
        for element in 0..width {
            budget.charge(Cost::work(1))?;
            cells.push(if selected.is_none_or(|set| set.contains(&cells.len())) {
                scalar(cell(array.as_ref(), row, field.array.then_some(element))?)?
            } else {
                Scalar::Null
            });
        }
    }
    Ok(Row {
        cells,
        _allocation: allocation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{ArrayRef, RecordBatch};
    use arrow_buffer::NullBuffer;
    use arrow_schema::{Field, Schema};
    fn fields() -> Vec<schema::Field> {
        [
            ("Signed", false, 1),
            ("Unsigned", false, 1),
            ("Text", false, 1),
            ("Array", true, 2),
        ]
        .into_iter()
        .map(|(name, array, elements)| schema::Field {
            name: name.into(),
            kind: "int".into(),
            foreign_table: String::new(),
            foreign_column: String::new(),
            verified: true,
            bits: 64,
            signed: true,
            elements,
            array,
            identity: false,
            inline: true,
            relation: false,
        })
        .collect()
    }
    fn batch() -> RecordBatch {
        let list = FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Int64, true)),
            2,
            Arc::new(Int64Array::from(vec![Some(-1), None, Some(5), Some(6)])),
            Some(NullBuffer::from(vec![true, false])),
        )
        .unwrap();
        let arrays: Vec<ArrayRef> = vec![
            Arc::new(Int64Array::from(vec![Some(i64::MIN), None])),
            Arc::new(UInt64Array::from(vec![u64::MAX, 9_007_199_254_740_993])),
            Arc::new(StringArray::from(vec![Some("中文🙂"), None])),
            Arc::new(list),
            Arc::new(UInt32Array::from(vec![7, 8])),
        ];
        let schema = Arc::new(Schema::new(
            arrays
                .iter()
                .enumerate()
                .map(|(i, a)| Field::new(i.to_string(), a.data_type().clone(), true))
                .collect::<Vec<_>>(),
        ));
        RecordBatch::try_new(schema, arrays).unwrap()
    }
    #[test]
    fn typed_scalar_row_preserves_null_lists_children_and_exact_integer_variants() {
        let batch = batch();
        let fields = fields();
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(60),
            None,
        );
        let first = flatten(&batch, 0, &fields, None, &budget).unwrap();
        assert_eq!(
            first.cells,
            vec![
                Scalar::Int(i64::MIN),
                Scalar::UInt(u64::MAX),
                Scalar::String("中文🙂".into()),
                Scalar::Int(-1),
                Scalar::Null
            ]
        );
        let second = flatten(&batch, 1, &fields, None, &budget).unwrap();
        assert_eq!(
            second.cells,
            vec![
                Scalar::Null,
                Scalar::UInt(9_007_199_254_740_993),
                Scalar::Null,
                Scalar::Null,
                Scalar::Null
            ]
        );
        assert!(budget.refundable_retained_bytes() > 0);
        drop(first);
        drop(second);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
    #[test]
    fn typed_scalar_projection_and_rejection_keep_row_allocation_owned() {
        let batch = batch();
        let fields = fields();
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(60),
            None,
        );
        let selected = BTreeSet::from([1usize, 4]);
        let row = flatten(&batch, 0, &fields, Some(&selected), &budget).unwrap();
        assert_eq!(
            row.cells,
            vec![
                Scalar::Null,
                Scalar::UInt(u64::MAX),
                Scalar::Null,
                Scalar::Null,
                Scalar::Null
            ]
        );
        drop(row);
        assert_eq!(budget.refundable_retained_bytes(), 0);
        let narrow = Budget::new(
            Cost {
                retained_bytes: 1,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(60),
            None,
        );
        assert!(flatten(&batch, 0, &fields, None, &narrow).is_err());
        assert_eq!(narrow.refundable_retained_bytes(), 0);
    }
}
