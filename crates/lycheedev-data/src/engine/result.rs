//! Result and page lifetimes retain their admission through serialization.
use super::{api::ResultPage, lifetime::RequestLifetime};
use crate::{
    fault,
    resource::{Budget, OwnedRetainedReservation},
};
use arrow_array::{Array, FixedSizeListArray, LargeListArray, ListArray, RecordBatch};
use datafusion::common::ScalarValue;
use lycheedev_core::Result;
use serde_json::Value;
use std::sync::Arc;

#[derive(serde::Serialize)]
#[serde(transparent)]
pub struct OwnedProvenance {
    value: Value,
    #[serde(skip)]
    pub(super) session: Arc<Budget>,
    #[serde(skip)]
    pub(super) dependencies: Vec<String>,
    #[serde(skip)]
    _allocation: OwnedRetainedReservation,
}
impl std::fmt::Debug for OwnedProvenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedProvenance").finish_non_exhaustive()
    }
}
pub struct ResultOwner {
    pub(super) schema: Option<arrow_schema::SchemaRef>,
    pub(super) dependencies: Vec<String>,
    pub(super) batches: Vec<RecordBatch>,
    pub(super) columns: Vec<String>,
    pub(super) provenance: Value,
    pub(super) provenance_bytes: usize,
    pub(super) complete: bool,
    pub(super) rows: usize,
    pub(super) bytes: usize,
    pub(super) session: Arc<Budget>,
    pub(super) page_bytes: usize,
    pub(super) _lifetimes: Vec<Arc<RequestLifetime>>,
    pub(super) _output: Option<OwnedRetainedReservation>,
}
impl Drop for ResultOwner {
    fn drop(&mut self) {
        // Release output ownership before reconciling the shared session
        // admission. A cached source can keep this request lifetime alive.
        drop(std::mem::take(&mut self.batches));
        drop(std::mem::take(&mut self.columns));
        drop(std::mem::take(&mut self.dependencies));
        drop(std::mem::replace(&mut self.provenance, Value::Null));
        self.schema.take();
        self._output.take();
        for lifetime in &self._lifetimes {
            // Poisoned accounting stays conservatively reserved. Drop cannot
            // report an error or safely guess a refund after corruption.
            let _ = lifetime.settle();
        }
    }
}
impl OwnedProvenance {
    pub(crate) fn value(&self) -> &Value { &self.value }
}
impl ResultOwner {
    pub(crate) fn visit_batches<T>(&self, control: &lycheedev_core::control::OperationControl,
        visitor: impl FnOnce(&arrow_schema::SchemaRef, &[RecordBatch]) -> Result<T>) -> Result<T> {
        control.check()?;
        let schema = self.schema.clone().or_else(||self.batches.first().map(RecordBatch::schema))
            .ok_or_else(|| fault("records.internal", 8, "Typed result schema is missing"))?;
        let result = visitor(&schema, &self.batches)?;
        control.check()?;
        Ok(result)
    }
    pub fn row_count(&self) -> usize {
        self.rows
    }
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn provenance(&self) -> &Value {
        &self.provenance
    }
    pub fn owned_provenance(&self) -> Result<OwnedProvenance> {
        let bytes = self.dependencies.iter().try_fold(self.provenance_bytes, |n, item| {
            n.checked_add(item.len()).and_then(|n| n.checked_add(128)).ok_or_else(bound)
        })?;
        let allocation = self
            .session
            .reserve_retained(bytes)?
            .into_owned(&self.session)?;
        Ok(OwnedProvenance {
            value: self.provenance.clone(),
            session: self.session.clone(),
            dependencies: self.dependencies.clone(),
            _allocation: allocation,
        })
    }
    pub fn page(&self, offset: usize, limit: usize) -> Result<ResultPage> {
        if limit > 1000 {
            return Err(bound());
        }
        let end = offset.checked_add(limit).ok_or_else(bound)?.min(self.rows);
        if offset > self.rows {
            return Err(bound());
        }
        let allocation = self
            .session
            .reserve_retained(self.page_bytes)?
            .into_owned(&self.session)?;
        let mut remaining = self.page_bytes;
        take(
            &mut remaining,
            1024 + self.columns.len().checked_mul(64).ok_or_else(bound)?,
        )?;
        for column in &self.columns {
            take(&mut remaining, column.len())?;
        }
        let mut rows = Vec::new();
        let mut base = 0usize;
        for batch in &self.batches {
            let stop = base.checked_add(batch.num_rows()).ok_or_else(bound)?;
            for row in offset.saturating_sub(base)..end.saturating_sub(base).min(batch.num_rows()) {
                take(
                    &mut remaining,
                    64 + batch.num_columns().checked_mul(64).ok_or_else(bound)?,
                )?;
                let mut values = Vec::with_capacity(batch.num_columns());
                for array in batch.columns() {
                    values.push(value(array.as_ref(), row, &mut remaining, 0)?);
                }
                rows.push(values);
            }
            base = stop;
            if base >= end {
                break;
            }
        }
        Ok(ResultPage {
            columns: self.columns.clone(),
            rows,
            offset,
            total_rows: self.rows,
            complete: self.complete,
            _allocation: allocation,
            _budget: self.session.clone(),
            _byte_limit: self.page_bytes,
        })
    }
}
fn take(remaining: &mut usize, bytes: usize) -> Result<()> {
    *remaining = remaining.checked_sub(bytes).ok_or_else(bound)?;
    Ok(())
}
pub(super) fn value(
    array: &dyn Array,
    row: usize,
    remaining: &mut usize,
    depth: usize,
) -> Result<Value> {
    if row >= array.len() || depth > 64 {
        return Err(bound());
    }
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    macro_rules! list {
        ($ty:ty) => {
            if let Some(list) = array.as_any().downcast_ref::<$ty>() {
                let items = list.value(row);
                take(
                    remaining,
                    64 + items.len().checked_mul(64).ok_or_else(bound)?,
                )?;
                let mut output = Vec::with_capacity(items.len());
                for i in 0..items.len() {
                    output.push(value(items.as_ref(), i, remaining, depth + 1)?);
                }
                return Ok(Value::Array(output));
            }
        };
    }
    list!(FixedSizeListArray);
    list!(ListArray);
    list!(LargeListArray);
    // Check text size before ScalarValue makes an owned string.
    macro_rules! text {
        ($ty:ty) => {
            if let Some(strings) = array.as_any().downcast_ref::<$ty>() {
                let text = strings.value(row);
                take(remaining, text.len().checked_mul(2).ok_or_else(bound)?)?;
                return Ok(Value::String(text.to_owned()));
            }
        };
    }
    text!(arrow_array::StringArray);
    text!(arrow_array::LargeStringArray);
    text!(arrow_array::StringViewArray);
    let scalar = ScalarValue::try_from_array(array, row).map_err(super::error)?;
    Ok(match scalar {
        ScalarValue::Null => Value::Null,
        ScalarValue::Boolean(Some(v)) => Value::Bool(v),
        ScalarValue::Int8(Some(v)) => Value::from(v),
        ScalarValue::Int16(Some(v)) => Value::from(v),
        ScalarValue::Int32(Some(v)) => Value::from(v),
        ScalarValue::Int64(Some(v)) => Value::from(v),
        ScalarValue::UInt8(Some(v)) => Value::from(v),
        ScalarValue::UInt16(Some(v)) => Value::from(v),
        ScalarValue::UInt32(Some(v)) => Value::from(v),
        ScalarValue::UInt64(Some(v)) => Value::from(v),
        ScalarValue::Float32(Some(v)) if v.is_finite() => {
            serde_json::to_value(v).map_err(|_| bound())?
        }
        ScalarValue::Float64(Some(v)) if v.is_finite() => {
            serde_json::to_value(v).map_err(|_| bound())?
        }
        ScalarValue::Decimal128(Some(v), _, 0)
            if v >= i64::MIN as i128 && v <= u64::MAX as i128 =>
        {
            if v < 0 {
                Value::from(v as i64)
            } else {
                Value::from(v as u64)
            }
        }
        _ => {
            return Err(fault(
                "records.result_type",
                3,
                "Result type has no verified exact JSON representation",
            ));
        }
    })
}
fn bound() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Result page exceeds its admitted bound",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Float32Array, Int64Array, NullArray, StringArray, UInt64Array};
    #[test]
    fn releasing_result_refunds_session_output_while_source_cache_survives() {
        let memory = super::super::lifetime::SessionMemory::new(10000).unwrap();
        let lifetime = memory
            .begin(10000, lycheedev_core::control::OperationControl::default())
            .unwrap();
        let source = lifetime
            .budget
            .reserve_retained(1000)
            .unwrap()
            .into_owned(&lifetime.budget)
            .unwrap();
        let output = lifetime
            .budget
            .reserve_retained(2000)
            .unwrap()
            .into_owned(&lifetime.budget)
            .unwrap();
        lifetime.settle().unwrap();
        let result = ResultOwner {
            schema: None,
            dependencies: vec![],
            batches: vec![],
            columns: vec![],
            provenance: serde_json::json!({"complete":true}),
            provenance_bytes: 1024,
            complete: true,
            rows: 0,
            bytes: 2000,
            session: memory.budget.clone(),
            page_bytes: 1000,
            _lifetimes: vec![lifetime.clone()],
            _output: Some(output),
        };
        assert_eq!(result.retained_bytes(), 2000);
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 3000);
        let receipt = result.owned_provenance().unwrap();
        let receipt_bytes = memory.budget.snapshot().unwrap().retained_bytes - 3000;
        assert!(receipt_bytes > 0);
        drop(result);
        assert_eq!(
            memory.budget.snapshot().unwrap().retained_bytes,
            1000 + receipt_bytes
        );
        assert_eq!(
            serde_json::to_string(&receipt).unwrap(),
            "{\"complete\":true}"
        );
        drop(receipt);
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 1000);
        let next = memory
            .begin(10000, lycheedev_core::control::OperationControl::default())
            .unwrap();
        assert!(next.budget.reserve_retained(9000).is_ok());
        drop(next);
        drop(source);
        drop(lifetime);
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 0);
    }
    #[test]
    fn exact_scalar_boundaries_and_missing_values() {
        let mut bytes = 1024;
        assert_eq!(
            value(&UInt64Array::from(vec![u64::MAX]), 0, &mut bytes, 0)
                .unwrap()
                .to_string(),
            "18446744073709551615"
        );
        assert_eq!(
            value(&Int64Array::from(vec![i64::MIN]), 0, &mut bytes, 0)
                .unwrap()
                .to_string(),
            "-9223372036854775808"
        );
        assert_eq!(
            value(&Float32Array::from(vec![0.1f32]), 0, &mut bytes, 0).unwrap(),
            serde_json::to_value(0.1f32).unwrap()
        );
        assert_eq!(
            value(&UInt64Array::from(vec![None]), 0, &mut bytes, 0).unwrap(),
            Value::Null
        );
        let nulls = NullArray::new(1);
        assert_eq!(value(&nulls, 0, &mut bytes, 0).unwrap(), Value::Null);
        assert_eq!(
            value(&nulls, 1, &mut bytes, 0).unwrap_err().code,
            "records.query_resource_budget"
        );
    }
    #[test]
    fn long_text_is_rejected_before_owned_copy() {
        let array = StringArray::from(vec!["abcdefgh"]);
        assert!(value(&array, 0, &mut 4, 0).is_err());
        assert!(value(&array, 1, &mut 100, 0).is_err());
    }
    #[test]
    fn non_finite_is_not_silently_null() {
        assert!(value(&Float32Array::from(vec![f32::INFINITY]), 0, &mut 100, 0).is_err());
    }
}

// Integrate into engine/result.rs. Do not duplicate its already-present value().
// Same-session identity and dependencies remain privately Engine-constructed.
impl OwnedProvenance {
    pub(crate) fn belongs_to(&self, session: &std::sync::Arc<crate::resource::Budget>) -> bool {
        std::sync::Arc::ptr_eq(&self.session, session)
    }
    pub(crate) fn dependencies(&self) -> &[String] {
        &self.dependencies
    }
}
