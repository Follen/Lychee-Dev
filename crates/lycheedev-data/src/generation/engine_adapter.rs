//! Authenticated static/effective sources for the production Arrow engine.
//! Generation reuse is an explicit representation choice, never a source or
//! Hotfix fallback. Each retained buffer keeps its original admission owner.
pub(crate) mod demand;
use super::{
    ImportLimits, ImportedBatchAllocation, ImportedSchema, Reuse, StaticIdentity, StaticSource,
    TableImporter, import,
};
use crate::{
    capture, fault,
    query_source::{effective, provider::Physical},
    query_value::Scalar,
    remote::provider::FileScope,
    resource::{Budget, Cost, OwnedRetainedReservation},
    table, table_source,
};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{
        ArrayBuilder, FixedSizeListBuilder, Float32Builder, Int64Builder, StringBuilder,
        UInt64Builder,
    },
};
use arrow_buffer::{MemoryPool, MemoryReservation};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use crate::request::DataContext;
use lycheedev_core::{Result, ToolkitError};
use serde_json::{Value, json};
use std::{fmt, sync::Arc};

// Batch sizing is a production policy, not an aggregate table limit. All
// retained batches compete in the original session Budget before allocation.
const BATCH_ROWS: usize = 1024;
const BATCH_VALUES: usize = 1 << 20;

struct SchemaOwner {
    schema: SchemaRef,
    _original: Arc<ImportedSchema>,
    _extra: Option<OwnedRetainedReservation>,
}
enum Admission {
    Imported(ImportedBatchAllocation),
    Generation(OwnedRetainedReservation),
    Effective(OwnedRetainedReservation),
}
impl Admission {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Imported(allocation) => allocation.retained_bytes(),
            Self::Generation(allocation) | Self::Effective(allocation) => allocation.bytes(),
        }
    }
}
/// There is deliberately no Array, Buffer or RecordBatch in this owner.
struct BufferOwner {
    _admission: Admission,
    _schema: Arc<SchemaOwner>,
    _hooks: OwnedRetainedReservation,
}
impl fmt::Debug for BufferOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OriginalSourceAdmission")
    }
}
#[derive(Debug)]
struct InputPool(Arc<BufferOwner>);
#[derive(Debug)]
struct InputLease {
    bytes: usize,
    _owner: Arc<BufferOwner>,
}
impl MemoryPool for InputPool {
    fn reserve(&self, bytes: usize) -> Box<dyn MemoryReservation> {
        Box::new(InputLease {
            bytes,
            _owner: self.0.clone(),
        })
    }
    fn available(&self) -> isize {
        0
    }
    fn used(&self) -> usize {
        0
    }
    fn capacity(&self) -> usize {
        0
    }
}
impl MemoryReservation for InputLease {
    fn size(&self) -> usize {
        self.bytes
    }
    fn resize(&mut self, bytes: usize) {
        self.bytes = bytes;
    }
}

fn limit() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Source Arrow allocation exceeds its admitted bound",
    )
}
fn shape() -> ToolkitError {
    fault(
        "records.generation_type",
        4,
        "Source value differs from its exact typed schema",
    )
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(limit)
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(limit)
}

#[derive(Clone, Copy)]
enum Kind {
    Signed,
    Unsigned,
    Float,
    Text,
}
impl Kind {
    fn from_type(kind: &DataType) -> Result<Self> {
        match kind {
            DataType::Int64 => Ok(Self::Signed),
            DataType::UInt64 => Ok(Self::Unsigned),
            DataType::Float32 => Ok(Self::Float),
            DataType::Utf8 => Ok(Self::Text),
            _ => Err(shape()),
        }
    }
    fn width(self) -> usize {
        match self {
            Self::Signed | Self::Unsigned => 8,
            Self::Float | Self::Text => 4,
        }
    }
    fn builder(self, rows: usize) -> Box<dyn ArrayBuilder> {
        match self {
            Self::Signed => Box::new(Int64Builder::with_capacity(rows)),
            Self::Unsigned => Box::new(UInt64Builder::with_capacity(rows)),
            Self::Float => Box::new(Float32Builder::with_capacity(rows)),
            Self::Text => Box::new(StringBuilder::with_capacity(rows, 0)),
        }
    }
    fn inspect(self, value: &Scalar) -> Result<usize> {
        match (self, value) {
            (_, Scalar::Null) => Ok(self.width()),
            (Self::Signed, Scalar::Int(_)) | (Self::Unsigned, Scalar::UInt(_)) => Ok(8),
            // JSON-backed Hotfix fields may use the other integer variant, but
            // conversion must be exact and checked, never via binary float.
            (Self::Signed, Scalar::UInt(value)) if i64::try_from(*value).is_ok() => Ok(8),
            (Self::Unsigned, Scalar::Int(value)) if *value >= 0 => Ok(8),
            (Self::Float, Scalar::Float(value))
                if value.is_finite()
                    && (*value as f32).is_finite()
                    && f64::from(*value as f32) == *value =>
            {
                Ok(4)
            }
            (Self::Float, Scalar::Int(value)) if (*value as f32) as i128 == i128::from(*value) => {
                Ok(4)
            }
            (Self::Float, Scalar::UInt(value)) if (*value as f32) as u128 == u128::from(*value) => {
                Ok(4)
            }
            (Self::Text, Scalar::String(value)) => add(4, value.len()),
            _ => Err(shape()),
        }
    }
    fn append(self, builder: &mut dyn ArrayBuilder, value: &Scalar) {
        // `inspect` validates the complete row before any builder mutation.
        match self {
            Self::Signed => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<Int64Builder>()
                    .expect("signed builder");
                match value {
                    Scalar::Int(v) => out.append_value(*v),
                    Scalar::UInt(v) => out.append_value(*v as i64),
                    _ => out.append_null(),
                }
            }
            Self::Unsigned => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<UInt64Builder>()
                    .expect("unsigned builder");
                match value {
                    Scalar::UInt(v) => out.append_value(*v),
                    Scalar::Int(v) => out.append_value(*v as u64),
                    _ => out.append_null(),
                }
            }
            Self::Float => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<Float32Builder>()
                    .expect("float builder");
                match value {
                    Scalar::Float(v) => out.append_value(*v as f32),
                    Scalar::Int(v) => out.append_value(*v as f32),
                    Scalar::UInt(v) => out.append_value(*v as f32),
                    _ => out.append_null(),
                }
            }
            Self::Text => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<StringBuilder>()
                    .expect("text builder");
                match value {
                    Scalar::String(v) => out.append_value(v),
                    _ => out.append_null(),
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
struct Column {
    kind: Kind,
    width: Option<usize>,
}
impl Column {
    fn from_type(kind: &DataType) -> Result<Self> {
        let (kind, width) = match kind {
            DataType::FixedSizeList(field, width) if *width > 0 => {
                (field.data_type(), Some(*width as usize))
            }
            _ => (kind, None),
        };
        Ok(Self {
            kind: Kind::from_type(kind)?,
            width,
        })
    }
    fn builder(self, rows: usize) -> Result<Builder> {
        match self.width {
            None => Ok(Builder::Scalar(self.kind.builder(rows))),
            Some(width) => Ok(Builder::List(FixedSizeListBuilder::with_capacity(
                self.kind.builder(mul(rows, width)?),
                width as i32,
                rows,
            ))),
        }
    }
}
enum Builder {
    Scalar(Box<dyn ArrayBuilder>),
    List(FixedSizeListBuilder<Box<dyn ArrayBuilder>>),
}
impl Builder {
    fn finish(self) -> ArrayRef {
        match self {
            Self::Scalar(mut builder) => builder.finish(),
            Self::List(mut builder) => Arc::new(builder.finish()),
        }
    }
}
