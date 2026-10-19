//! A retaining choice for the existing Index schema, not another cache format.
//! Each row is fully decoded as its ordinary business type, then discarded.
//! Authentication, limits, identity and cleanup remain in the shared loader.
//! The ordinary serde_json recursion limit remains in force; no custom deep
//! traversal or parser budget changes the previous acceptance boundary.
use super::{Document, Index, Symbol};
use lycheedev_core::{Result, control::OperationControl, selection::SourcePin};
use serde::{
    Deserialize, Deserializer,
    de::{SeqAccess, Visitor},
};
use serde_json::Value;
use std::{fmt, marker::PhantomData, path::Path};

pub(super) trait CollectionLength {
    fn len(&self) -> usize;
}
impl<T> CollectionLength for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
}

struct Counted<T> {
    count: usize,
    element: PhantomData<T>,
}
impl<T> CollectionLength for Counted<T> {
    fn len(&self) -> usize {
        self.count
    }
}
impl<T> From<Vec<T>> for Counted<T> {
    fn from(rows: Vec<T>) -> Self {
        Self {
            count: rows.len(),
            element: PhantomData,
        }
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Counted<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct CountVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for CountVisitor<T> {
            type Value = Counted<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a sequence")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut count = 0;
                while let Some(row) = seq.next_element::<T>()? {
                    drop(row);
                    count += 1;
                }
                Ok(Counted {
                    count,
                    element: PhantomData,
                })
            }
        }
        // No early count budget or control check: the complete old JSON decode
        // must precede the shared post-decode control, identity and count gate.
        deserializer.deserialize_seq(CountVisitor(PhantomData))
    }
}

type SummaryIndex = Index<Counted<Document>, Counted<Value>, Counted<Symbol>>;

pub(crate) fn build_summary_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &OperationControl,
) -> Result<(Value, Value)> {
    let (index, _): (SummaryIndex, _) =
        super::build_material_projected_with_control(home, pin, control)?;
    let coverage = super::coverage(&index.summary, index.documents.len());
    Ok((index.summary, coverage))
}

#[cfg(test)]
mod tests;
