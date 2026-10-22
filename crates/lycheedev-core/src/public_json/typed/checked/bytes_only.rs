//! The private raw marker admits exactly one serialize_bytes call. No sequence,
//! string, option, map, or nested newtype can arm the raw formatter accidentally.
use serde::{Serialize, Serializer, ser::Impossible};
pub(super) struct BytesOnly<S>(pub S);
fn rejected<E: serde::ser::Error>() -> E {
    E::custom("raw JSON marker requires a byte payload")
}
macro_rules! scalar {
    ($($method:ident ($ty:ty)),* $(,)?) => { $(
        fn $method(self, _: $ty) -> Result<Self::Ok, Self::Error> { Err(rejected()) }
    )*};
}
impl<S: Serializer> Serializer for BytesOnly<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Impossible<S::Ok, S::Error>;
    type SerializeTuple = Impossible<S::Ok, S::Error>;
    type SerializeTupleStruct = Impossible<S::Ok, S::Error>;
    type SerializeTupleVariant = Impossible<S::Ok, S::Error>;
    type SerializeMap = Impossible<S::Ok, S::Error>;
    type SerializeStruct = Impossible<S::Ok, S::Error>;
    type SerializeStructVariant = Impossible<S::Ok, S::Error>;
    scalar! {
        serialize_bool(bool), serialize_i8(i8), serialize_i16(i16), serialize_i32(i32),
        serialize_i64(i64), serialize_i128(i128), serialize_u8(u8), serialize_u16(u16),
        serialize_u32(u32), serialize_u64(u64), serialize_u128(u128),
        serialize_f32(f32), serialize_f64(f64), serialize_char(char), serialize_str(&str),
    }
    fn serialize_bytes(self, bytes: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.0.serialize_bytes(bytes)
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(rejected())
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(rejected())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(rejected())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(rejected())
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(rejected())
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(rejected())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(rejected())
    }
    fn collect_str<T: ?Sized + std::fmt::Display>(self, _: &T) -> Result<Self::Ok, Self::Error> {
        Err(rejected())
    }
    // A local payload adapter signal, not authentication or a general change
    // to typed business values' human-readable serialization mode.
    fn is_human_readable(&self) -> bool {
        false
    }
}
