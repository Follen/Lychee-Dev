//! Private, versioned Postcard Facts wire. No public JSON contract is changed.
//! Postcard owns integer/string framing; these adapters only enforce admission,
//! exact field presence, checked coordinates, and operation cancellation.
use super::Facts;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde::{
    Deserialize, Serialize,
    de::{DeserializeSeed, Error, SeqAccess, Visitor},
    ser::SerializeSeq,
};
use sha2::{Digest, Sha256};
use std::{fmt, marker::PhantomData};

pub(crate) const MAGIC: &[u8; 8] = b"LYCFCT13";
pub(crate) const HEADER: usize = 76;
pub(crate) const MAXIMUM: usize = 32 << 20;
// Per-record decoded ownership admission, not a whole-query or RSS bound.
const MAX_OWNED: usize = 256 << 20;
// Preserve the previous producer's exact logical limit without a JSON DOM.
const JSON_ENVELOPE_BYTES: usize =
    br#"{"facts":,"factsSHA256":"","inputSHA256":"","schema":"lycheedev.source-facts.rust.v11"}"#
        .len()
        + 128;

fn budget() -> ToolkitError {
    ToolkitError::new(
        "codebase.fact_budget",
        "Source fact cache exceeds the 32 MiB limit",
        3,
    )
}

pub(super) trait Wire: Sized {
    const MIN_BYTES: usize;
    fn encode<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>;
    fn decode<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
        context: &mut DecodeContext<'_>,
    ) -> std::result::Result<Self, D::Error>;
}

pub(super) struct WireRef<'a, T>(pub &'a T);
impl<T: Wire> Serialize for WireRef<'_, T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.0.encode(serializer)
    }
}

pub(super) struct Seed<'a, 'b, T> {
    context: &'a mut DecodeContext<'b>,
    kind: PhantomData<T>,
}
impl<'a, 'b, T> Seed<'a, 'b, T> {
    pub(super) fn new(context: &'a mut DecodeContext<'b>) -> Self {
        Self {
            context,
            kind: PhantomData,
        }
    }
}
impl<'de, T: Wire> DeserializeSeed<'de> for Seed<'_, '_, T> {
    type Value = T;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<T, D::Error> {
        T::decode(deserializer, self.context)
    }
}

pub(super) struct DecodeContext<'a> {
    control: &'a OperationControl,
    payload_bytes: usize,
    remaining: usize,
    failure: Option<ToolkitError>,
}
impl DecodeContext<'_> {
    pub(super) fn check<E: Error>(&mut self) -> std::result::Result<(), E> {
        self.control.check().map_err(|error| {
            self.failure = Some(error);
            E::custom("source Facts operation stopped")
        })
    }
    fn admit<E: Error>(&mut self, bytes: usize) -> std::result::Result<(), E> {
        self.check::<E>()?;
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| E::custom("source Facts ownership limit"))?;
        Ok(())
    }
    fn allocation<E: Error>(&mut self) -> E {
        self.failure = Some(budget());
        E::custom("source Facts allocation was not admitted")
    }
}

impl Wire for String {
    const MIN_BYTES: usize = 1;
    fn encode<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self)
    }
    fn decode<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
        context: &mut DecodeContext<'_>,
    ) -> std::result::Result<Self, D::Error> {
        // Slice deserialization validates UTF-8 and the complete byte range
        // before returning this borrow; no untrusted length allocates a String.
        let value = <&str>::deserialize(deserializer)?;
        context.admit::<D::Error>(value.len())?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(value.len())
            .map_err(|_| context.allocation::<D::Error>())?;
        context.admit::<D::Error>(owned.capacity() - value.len())?;
        owned.push_str(value);
        context.check::<D::Error>()?;
        Ok(owned)
    }
}
impl Wire for usize {
    const MIN_BYTES: usize = 1;
    fn encode<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        // The wire coordinate is always u64, independent of host pointer width.
        serializer.serialize_u64(
            u64::try_from(*self)
                .map_err(|_| serde::ser::Error::custom("source Facts coordinate overflow"))?,
        )
    }
    fn decode<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
        _: &mut DecodeContext<'_>,
    ) -> std::result::Result<Self, D::Error> {
        usize::try_from(u64::deserialize(deserializer)?)
            .map_err(|_| D::Error::custom("source Facts coordinate overflow"))
    }
}
impl<T: Wire> Wire for Vec<T> {
    const MIN_BYTES: usize = 1;
    fn encode<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.len()))?;
        for row in self {
            sequence.serialize_element(&WireRef(row))?;
        }
        sequence.end()
    }
    fn decode<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
        context: &mut DecodeContext<'_>,
    ) -> std::result::Result<Self, D::Error> {
        struct Rows<'a, 'b, T>(Seed<'a, 'b, T>);
        impl<'de, T: Wire> Visitor<'de> for Rows<'_, '_, T> {
            type Value = Vec<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded Facts sequence")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Vec<T>, A::Error> {
                let context = self.0.context;
                let count = sequence
                    .size_hint()
                    .ok_or_else(|| A::Error::custom("missing Facts count"))?;
                if count
                    .checked_mul(T::MIN_BYTES)
                    .is_none_or(|n| n > context.payload_bytes)
                {
                    return Err(A::Error::custom("Facts count exceeds payload"));
                }
                let bytes = count
                    .checked_mul(size_of::<T>())
                    .ok_or_else(|| A::Error::custom("Facts count overflow"))?;
                context.admit::<A::Error>(bytes)?;
                let mut rows = Vec::new();
                rows.try_reserve_exact(count)
                    .map_err(|_| context.allocation::<A::Error>())?;
                let capacity_bytes = rows
                    .capacity()
                    .checked_mul(size_of::<T>())
                    .ok_or_else(|| A::Error::custom("Facts capacity overflow"))?;
                context.admit::<A::Error>(capacity_bytes - bytes)?;
                for _ in 0..count {
                    context.check::<A::Error>()?;
                    rows.push(
                        sequence
                            .next_element_seed(Seed::<T>::new(context))?
                            .ok_or_else(|| A::Error::custom("missing Facts row"))?,
                    );
                }
                Ok(rows)
            }
        }
        deserializer.deserialize_seq(Rows(Seed::<T>::new(context)))
    }
}

// A bounded output flavor is the Postcard-supported extension point. It does
// not implement a second codec, varint parser, or generic persistence layer.
struct Output(Vec<u8>);
impl postcard::ser_flavors::Flavor for Output {
    type Output = Vec<u8>;
    fn try_push(&mut self, byte: u8) -> postcard::Result<()> {
        self.try_extend(&[byte])
    }
    fn try_extend(&mut self, bytes: &[u8]) -> postcard::Result<()> {
        let length = self
            .0
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= MAXIMUM)
            .ok_or(postcard::Error::SerializeBufferFull)?;
        if length > self.0.capacity() {
            let capacity = length.max(self.0.capacity().saturating_mul(2)).min(MAXIMUM);
            self.0
                .try_reserve_exact(capacity - self.0.len())
                .map_err(|_| postcard::Error::SerializeBufferFull)?;
            if self.0.capacity() > MAXIMUM {
                return Err(postcard::Error::SerializeBufferFull);
            }
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn finalize(self) -> postcard::Result<Vec<u8>> {
        Ok(self.0)
    }
}

struct JsonLength<'a> {
    bytes: usize,
    next_check: usize,
    control: Option<&'a OperationControl>,
    stopped: Option<ToolkitError>,
}
impl std::io::Write for JsonLength<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= MAXIMUM)
            .ok_or_else(|| std::io::Error::other("Facts JSON admission limit"))?;
        // Escaped NULs cause many tiny serializer writes. Check at bounded byte
        // intervals rather than adding a clock read for every scalar fragment.
        if self.bytes >= self.next_check {
            if let Some(control) = self.control
                && let Err(error) = control.check()
            {
                self.stopped = Some(error);
                return Err(std::io::Error::other("Facts JSON admission stopped"));
            }
            self.next_check = self.bytes.saturating_add(64 << 10);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn admit_public_facts(facts: &Facts, control: Option<&OperationControl>) -> Result<()> {
    let mut counter = JsonLength {
        bytes: JSON_ENVELOPE_BYTES,
        next_check: 0,
        control,
        stopped: None,
    };
    let result = serde_json::to_writer(&mut counter, facts);
    if let Some(error) = counter.stopped {
        return Err(error);
    }
    result.map_err(|_| budget())
}

fn digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0; 32];
    for (at, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let digit = |b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        output[at] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(output)
}

pub(crate) fn encode(facts: &Facts, input: &str) -> Result<Vec<u8>> {
    let input =
        digest(input).ok_or_else(|| ToolkitError::invalid("Invalid Facts source digest"))?;
    // Compact public JSON field ordering cannot change its byte length. This
    // preserves the old complete-JSON budget, including escaped Unicode/control
    // characters and skipped fields, while retaining no JSON bytes or DOM.
    admit_public_facts(facts, None)?;
    let mut record = Vec::new();
    record.try_reserve_exact(HEADER).map_err(|_| budget())?;
    record.resize(HEADER, 0);
    let mut record =
        postcard::serialize_with_flavor(&WireRef(facts), Output(record)).map_err(|_| budget())?;
    let length = u32::try_from(record.len() - HEADER).map_err(|_| budget())?;
    let hash: [u8; 32] = Sha256::digest(&record[HEADER..]).into();
    record[..8].copy_from_slice(MAGIC);
    record[8..12].copy_from_slice(&length.to_le_bytes());
    record[12..44].copy_from_slice(&input);
    record[44..HEADER].copy_from_slice(&hash);
    Ok(record)
}

pub(crate) fn decode(raw: &[u8], input: &str, control: &OperationControl) -> Result<Option<Facts>> {
    control.check()?;
    let result = (|| {
        let Some(input) = digest(input) else {
            return Ok(None);
        };
        if raw.len() < HEADER || raw.len() > MAXIMUM || &raw[..8] != MAGIC {
            return Ok(None);
        }
        let length =
            u32::from_le_bytes(raw[8..12].try_into().expect("fixed Facts header")) as usize;
        if length.checked_add(HEADER) != Some(raw.len()) || raw[12..44] != input {
            return Ok(None);
        }
        let payload = &raw[HEADER..];
        if <[u8; 32]>::from(Sha256::digest(payload)) != raw[44..HEADER] {
            return Ok(None);
        }
        control.check()?;
        let mut context = DecodeContext {
            control,
            payload_bytes: payload.len(),
            remaining: MAX_OWNED,
            failure: None,
        };
        let mut decoder = postcard::Deserializer::from_bytes(payload);
        let decoded = Seed::<Facts>::new(&mut context).deserialize(&mut decoder);
        control.check()?;
        if let Some(error) = context.failure {
            return Err(error);
        }
        Ok(match (decoded, decoder.finalize()) {
            // A compact forged binary could otherwise expand beyond the old
            // accepted complete JSON limit (for example many NUL characters).
            // This extra warm traversal retains no JSON bytes/DOM. It is a
            // compatibility cost to measure, not a zero-conversion claim.
            (Ok(facts), Ok([])) => match admit_public_facts(&facts, Some(control)) {
                Ok(()) => Some(facts),
                Err(error) if error.code == "codebase.fact_budget" => None,
                Err(error) => return Err(error),
            },
            _ => None,
        })
    })();
    control.check()?;
    result
}

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod tests;
