//! Exact SUM/AVG accumulators inside DataFusion's grouping/merge execution.
//! The state is rational; partial groups never merge already-rounded answers.
use super::{QueryOwner, external, unsupported, values};
use crate::relational::{Scalar, engine_compat};
use arrow_array::{Array, ArrayRef, BooleanArray, Int64Array, StringArray};
use arrow_schema::{DataType, Field, FieldRef};
use datafusion::{
    common::{Result, ScalarValue},
    logical_expr::{
        Accumulator, AggregateUDF, AggregateUDFImpl, Signature, Volatility,
        function::{AccumulatorArgs, StateFieldsArgs},
    },
};
use num_rational::BigRational;
use num_traits::Zero;
use std::{
    hash::{Hash, Hasher},
    str::FromStr,
    sync::Arc,
};

#[derive(Debug)]
struct ExactAggregate {
    average: bool,
    signature: Signature,
    owner: Arc<QueryOwner>,
}
impl PartialEq for ExactAggregate {
    fn eq(&self, other: &Self) -> bool {
        self.average == other.average && Arc::ptr_eq(&self.owner, &other.owner)
    }
}
impl Eq for ExactAggregate {}
impl Hash for ExactAggregate {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.average.hash(h);
        Arc::as_ptr(&self.owner).hash(h);
    }
}
pub(super) fn functions(owner: &Arc<QueryOwner>) -> Vec<Arc<AggregateUDF>> {
    [false, true]
        .into_iter()
        .map(|average| {
            Arc::new(AggregateUDF::from(ExactAggregate {
                average,
                signature: Signature::any(1, Volatility::Immutable),
                owner: owner.clone(),
            }))
        })
        .collect()
}
impl AggregateUDFImpl for ExactAggregate {
    fn name(&self) -> &str {
        if self.average { "avg" } else { "sum" }
    }
    fn signature(&self) -> &Signature {
        &self.signature
    }
    fn return_type(&self, kinds: &[DataType]) -> Result<DataType> {
        let [kind] = kinds else {
            return Err(unsupported("SUM/AVG require one input"));
        };
        if !values::numeric(kind) {
            return Err(unsupported("SUM/AVG require a finite numeric input"));
        }
        Ok(if !self.average && values::integer(kind) {
            DataType::Decimal128(20, 0)
        } else {
            DataType::Float64
        })
    }
    fn accumulator(&self, args: AccumulatorArgs) -> Result<Box<dyn Accumulator>> {
        if args.is_distinct || !args.order_bys.is_empty() {
            return Err(unsupported(
                "Exact DISTINCT/ordered aggregate state has not yet completed engine adaptation",
            ));
        }
        self.owner.retain(512)?;
        Ok(Box::new(ExactAccumulator {
            sum: BigRational::zero(),
            count: 0,
            integer: true,
            average: self.average,
            output: args.return_type().clone(),
            capacity: 512,
            owner: self.owner.clone(),
        }))
    }
    fn state_fields(&self, args: StateFieldsArgs) -> Result<Vec<FieldRef>> {
        Ok(vec![
            Arc::new(Field::new(
                format!("{}:ratio", args.name),
                DataType::Utf8,
                false,
            )),
            Arc::new(Field::new(
                format!("{}:count", args.name),
                DataType::Int64,
                false,
            )),
            Arc::new(Field::new(
                format!("{}:integer", args.name),
                DataType::Boolean,
                false,
            )),
        ])
    }
}
#[derive(Debug)]
struct ExactAccumulator {
    sum: BigRational,
    count: i64,
    integer: bool,
    average: bool,
    output: DataType,
    capacity: usize,
    owner: Arc<QueryOwner>,
}
impl ExactAccumulator {
    fn add(&mut self, ratio: BigRational, count: i64, integer: bool) -> Result<()> {
        self.owner.work(1)?;
        if count < 0 {
            return Err(unsupported("Negative exact aggregate count"));
        }
        let next = self
            .count
            .checked_add(count)
            .ok_or_else(|| external(engine_compat::numeric_error()))?;
        let bits = (self.sum.numer().bits() + ratio.denom().bits())
            .max(ratio.numer().bits() + self.sum.denom().bits())
            + 1;
        let denominator = self.sum.denom().bits() + ratio.denom().bits();
        let needed = usize::try_from((bits + denominator).div_ceil(8))
            .ok()
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(128))
            .ok_or_else(|| external(super::limit()))?;
        if needed > self.capacity {
            self.owner.retain(needed - self.capacity)?;
            self.capacity = needed;
        }
        self.sum += ratio;
        self.count = next;
        self.integer &= integer;
        Ok(())
    }
}
impl Accumulator for ExactAccumulator {
    fn update_batch(&mut self, inputs: &[ArrayRef]) -> Result<()> {
        let [input] = inputs else {
            return Err(unsupported("Aggregate batch arity changed"));
        };
        for row in 0..input.len() {
            self.owner.check()?;
            let value = values::read(ScalarValue::try_from_array(input, row)?)?;
            if matches!(value, Scalar::Null) {
                continue;
            }
            let integer = matches!(value, Scalar::Int(_) | Scalar::UInt(_));
            // Input conversion is temporary; retained exact state grows only
            // through add(). Never accumulate a per-row scratch charge.
            let owner = self.owner.clone();
            let _input = owner.budget.reserve_retained(512).map_err(external)?;
            self.add(engine_compat::exact(&value).map_err(external)?, 1, integer)?;
        }
        Ok(())
    }
    fn evaluate(&mut self) -> Result<ScalarValue> {
        self.owner.check()?;
        if self.count == 0 {
            return ScalarValue::try_new_null(&self.output);
        }
        self.owner.retain(
            self.capacity
                .checked_mul(2)
                .ok_or_else(|| external(super::limit()))?,
        )?;
        let value = if self.average {
            engine_compat::finish_average(&self.sum, self.count)
        } else {
            engine_compat::finish_sum(&self.sum, self.integer)
        }
        .map_err(external)?;
        values::write(value, &self.output)
    }
    fn size(&self) -> usize {
        std::mem::size_of::<Self>() + self.capacity
    }
    fn state(&mut self) -> Result<Vec<ScalarValue>> {
        self.owner.check()?;
        self.owner.retain(8192)?;
        let ratio = self.sum.to_string();
        if ratio.len() > 4096 {
            return Err(external(super::limit()));
        }
        Ok(vec![
            ScalarValue::Utf8(Some(ratio)),
            ScalarValue::Int64(Some(self.count)),
            ScalarValue::Boolean(Some(self.integer)),
        ])
    }
    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        let [ratios, counts, integers] = states else {
            return Err(unsupported("Exact aggregate state arity changed"));
        };
        let ratios = ratios
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| unsupported("Invalid aggregate ratio state"))?;
        let counts = counts
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| unsupported("Invalid aggregate count state"))?;
        let integers = integers
            .as_any()
            .downcast_ref::<BooleanArray>()
            .ok_or_else(|| unsupported("Invalid aggregate kind state"))?;
        if ratios.len() != counts.len() || counts.len() != integers.len() {
            return Err(unsupported("Aggregate state lengths differ"));
        }
        for row in 0..ratios.len() {
            self.owner.check()?;
            if ratios.is_null(row) || counts.is_null(row) || integers.is_null(row) {
                return Err(unsupported("Null aggregate merge state"));
            }
            let text = ratios.value(row);
            if text.len() > 4096
                || !text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'-' || b == b'/')
            {
                return Err(external(super::limit()));
            }
            let owner = self.owner.clone();
            let _parse = owner.budget.reserve_retained(32768).map_err(external)?;
            let ratio = BigRational::from_str(text)
                .map_err(|_| unsupported("Invalid rational aggregate state"))?;
            if ratio.to_string() != text
                || counts.value(row) == 0 && !ratio.is_zero()
                || integers.value(row) && !ratio.is_integer()
            {
                return Err(unsupported("Noncanonical rational aggregate state"));
            }
            self.add(ratio, counts.value(row), integers.value(row))?;
        }
        Ok(())
    }
}
