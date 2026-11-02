//! Data-only WeakAuras identity extraction. Wire formats are the LibDeflate,
//! LibSerialize and AceSerializer formats used by DD 100130. No Lua is evaluated
//! and arbitrary action/code fields remain opaque strings.
use flate2::{Decompress, FlushDecompress, Status};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde::Serialize;
use serde_json::json;
use std::borrow::Cow;
mod ace;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_VALUES: usize = 1_000_000;
const MAX_DEPTH: usize = 128;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct WaIdentity {
    pub parse_wa_uid: String,
    pub parse_wa_id: String,
}
pub fn parse(input: &str, control: &OperationControl) -> Result<WaIdentity> {
    control.check()?;
    if input.len() > MAX_BYTES {
        return Err(invalid("input", "WA export exceeds 32 MiB"));
    }
    let input = input.trim();
    let (version, encoded) = if let Some(rest) = input.strip_prefix("!WA:") {
        let (version, encoded) = rest
            .split_once('!')
            .ok_or_else(|| invalid("prefix", "invalid WA export prefix"))?;
        if version.is_empty() || !version.bytes().all(|c| c.is_ascii_digit()) {
            return Err(invalid("prefix", "invalid WA export version"));
        }
        let version: u32 = version
            .parse()
            .map_err(|_| invalid("prefix", "unsupported WA export version"))?;
        if version > 2 {
            return Err(invalid("prefix", "unsupported WA export version"));
        }
        (version, encoded)
    } else if let Some(encoded) = input.strip_prefix('!') {
        (1, encoded)
    } else {
        return Err(invalid("prefix", "WA export requires ! or !WA:version!"));
    };
    let compressed = decode_print(encoded, control)?;
    let bytes = inflate(&compressed, control)?;
    if version < 2 {
        ace::parse(&bytes, control)
    } else {
        Serialized::parse(&bytes, control)
    }
}
fn invalid(stage: &str, message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_wa", message, 4).with_details(json!({"stage":stage}))
}
fn decode_print(input: &str, control: &OperationControl) -> Result<Vec<u8>> {
    let input = input.trim_matches(|c: char| c.is_ascii_control() || c == ' ');
    if input.len() == 1 {
        return Err(invalid("encoding", "invalid WA print encoding"));
    }
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0u32);
    for (index, byte) in input.bytes().enumerate() {
        if index % 65536 == 0 {
            control.check()?;
        }
        let value = match byte {
            b'a'..=b'z' => byte - b'a',
            b'A'..=b'Z' => byte - b'A' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'(' => 62,
            b')' => 63,
            _ => return Err(invalid("encoding", "invalid WA print encoding")),
        };
        bits |= u32::from(value) << count;
        count += 6;
        if count >= 8 {
            output.push(bits as u8);
            bits >>= 8;
            count -= 8;
        }
    }
    Ok(output)
}
fn inflate(input: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    let mut decoder = Decompress::new(false);
    let mut output = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        control.check()?;
        let (before_in, before_out) = (decoder.total_in(), decoder.total_out());
        let status = decoder
            .decompress(
                &input[before_in as usize..],
                &mut buffer,
                FlushDecompress::None,
            )
            .map_err(|_| invalid("deflate", "WA export contains invalid raw deflate data"))?;
        let produced = (decoder.total_out() - before_out) as usize;
        if output.len().saturating_add(produced) > MAX_BYTES {
            return Err(invalid("deflate", "decoded WA data exceeds 32 MiB"));
        }
        output.extend_from_slice(&buffer[..produced]);
        // LibDeflate returns unused trailing bytes separately; DD consumes the
        // first completed deflate stream. Preserve that existing format rule.
        if status == Status::StreamEnd {
            return Ok(output);
        }
        if decoder.total_in() == before_in && decoder.total_out() == before_out {
            return Err(invalid("deflate", "WA deflate stream is incomplete"));
        }
    }
}

#[derive(Clone, Copy)]
enum Value {
    Nil,
    Bool,
    Number(f64),
    String(usize),
    Table(usize),
}
struct Arena<'a> {
    strings: Vec<Cow<'a, [u8]>>,
    // Only d/uid/id are retained for each table. All other values are parsed
    // and validated, without materializing user action/code trees.
    tables: Vec<[Value; 3]>,
    values: usize,
    control: &'a OperationControl,
}
impl<'a> Arena<'a> {
    fn new(control: &'a OperationControl) -> Self {
        Self {
            strings: Vec::new(),
            tables: Vec::new(),
            values: 0,
            control,
        }
    }
    fn tick(&mut self, depth: usize) -> Result<()> {
        self.values += 1;
        if self.values > MAX_VALUES || depth > MAX_DEPTH {
            return Err(invalid(
                "serialization",
                "WA data exceeds one million values or 128 nesting levels",
            ));
        }
        if self.values % 256 == 0 {
            self.control.check()?;
        }
        Ok(())
    }
    fn string(&mut self, bytes: Cow<'a, [u8]>) -> Value {
        let index = self.strings.len();
        self.strings.push(bytes);
        Value::String(index)
    }
    fn table(&mut self) -> usize {
        let index = self.tables.len();
        self.tables.push([Value::Nil; 3]);
        index
    }
    fn assign(&mut self, table: usize, key: Value, value: Value) -> Result<()> {
        match key {
            Value::Nil => return Err(invalid("serialization", "WA table has a nil key")),
            Value::Number(value) if value.is_nan() => {
                return Err(invalid("serialization", "WA table has a NaN key"));
            }
            Value::String(index) => {
                let field = match self.strings[index].as_ref() {
                    b"d" => Some(0),
                    b"uid" => Some(1),
                    b"id" => Some(2),
                    _ => None,
                };
                if let Some(field) = field {
                    self.tables[table][field] = value;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn finish(&self, root: Value) -> Result<WaIdentity> {
        self.control.check()?;
        let Value::Table(root) = root else {
            return Err(invalid("identity", "WA export root is not a table"));
        };
        let Value::Table(data) = self.tables[root][0] else {
            return Err(invalid("identity", "WA export has no d table"));
        };
        let text = |field: usize| -> Result<String> {
            let Value::String(index) = self.tables[data][field] else {
                return Err(invalid(
                    "identity",
                    "WA d.uid and d.id must be nonempty strings",
                ));
            };
            let bytes = &self.strings[index];
            if bytes.is_empty() {
                return Err(invalid(
                    "identity",
                    "WA d.uid and d.id must be nonempty strings",
                ));
            }
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| invalid("identity", "WA identity is not UTF-8"))
        };
        Ok(WaIdentity {
            parse_wa_uid: text(1)?,
            parse_wa_id: text(2)?,
        })
    }
}

struct Serialized<'a> {
    bytes: &'a [u8],
    position: usize,
    arena: Arena<'a>,
    string_refs: Vec<usize>,
}
impl<'a> Serialized<'a> {
    fn parse(bytes: &'a [u8], control: &'a OperationControl) -> Result<WaIdentity> {
        let mut parser = Self {
            bytes,
            position: 0,
            arena: Arena::new(control),
            string_refs: Vec::new(),
        };
        if parser.byte()? > 2 {
            return Err(invalid(
                "serialization",
                "unsupported LibSerialize data version",
            ));
        }
        let root = parser.value(0)?;
        while parser.position < bytes.len() {
            parser.value(0)?;
        }
        parser.arena.finish(root)
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(size)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("serialization", "LibSerialize data is truncated"))?;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn integer(&mut self, size: usize) -> Result<u64> {
        Ok(self
            .take(size)?
            .iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte)))
    }
    fn string(&mut self, size: usize) -> Result<Value> {
        let bytes = self.take(size)?;
        let index = self.arena.strings.len();
        let value = self.arena.string(Cow::Borrowed(bytes));
        if size > 2 {
            self.string_refs.push(index);
        }
        Ok(value)
    }
    fn table(&mut self, array: usize, map: usize, depth: usize) -> Result<Value> {
        if array.saturating_add(map.saturating_mul(2)) > MAX_VALUES {
            return Err(invalid("serialization", "WA table exceeds the value limit"));
        }
        let index = self.arena.table();
        for _ in 0..array {
            self.value(depth + 1)?;
        }
        for _ in 0..map {
            let key = self.value(depth + 1)?;
            let value = self.value(depth + 1)?;
            self.arena.assign(index, key, value)?;
        }
        Ok(Value::Table(index))
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        self.arena.tick(depth)?;
        let byte = self.byte()?;
        if byte & 1 != 0 {
            return Ok(Value::Number(f64::from(byte >> 1)));
        }
        if byte & 3 == 2 {
            let count = (byte >> 4) as usize;
            return match (byte >> 2) & 3 {
                0 => self.string(count),
                1 => self.table(0, count, depth),
                2 => self.table(count, 0, depth),
                _ => self.table(count % 4 + 1, count / 4 + 1, depth),
            };
        }
        if byte & 7 == 4 {
            let packed = u16::from(self.byte()?) * 256 + u16::from(byte);
            let number = if byte & 15 == 12 {
                -f64::from((packed - 12) / 16)
            } else {
                f64::from((packed - 4) / 16)
            };
            return Ok(Value::Number(number));
        }
        let typ = byte >> 3;
        match typ {
            0 => Ok(Value::Nil),
            1..=8 => {
                let size = [2, 2, 3, 3, 4, 4, 7, 7][(typ - 1) as usize];
                let value = self.integer(size)? as f64;
                Ok(Value::Number(if typ % 2 == 0 { -value } else { value }))
            }
            9 => Ok(Value::Number(f64::from_be_bytes(
                self.take(8)?.try_into().expect("eight bytes"),
            ))),
            10 | 11 => {
                let size = self.byte()? as usize;
                let number = number(self.take(size)?)?;
                Ok(Value::Number(if typ == 11 { -number } else { number }))
            }
            12 | 13 => Ok(Value::Bool),
            14..=16 => {
                let size = self.integer((typ - 13) as usize)? as usize;
                self.string(size)
            }
            17..=19 => {
                let size = self.integer((typ - 16) as usize)? as usize;
                self.table(0, size, depth)
            }
            20..=22 => {
                let size = self.integer((typ - 19) as usize)? as usize;
                self.table(size, 0, depth)
            }
            23..=25 => {
                let size = (typ - 22) as usize;
                let array = self.integer(size)? as usize;
                let map = self.integer(size)? as usize;
                self.table(array, map, depth)
            }
            26..=28 => {
                let index = self.integer((typ - 25) as usize)? as usize;
                self.string_refs
                    .get(index.wrapping_sub(1))
                    .copied()
                    .map(Value::String)
                    .ok_or_else(|| {
                        invalid("serialization", "invalid LibSerialize string reference")
                    })
            }
            _ => {
                let index = self.integer((typ - 28) as usize)? as usize;
                if index == 0 || index > self.arena.tables.len() {
                    return Err(invalid(
                        "serialization",
                        "invalid LibSerialize table reference",
                    ));
                }
                Ok(Value::Table(index - 1))
            }
        }
    }
}
fn number(bytes: &[u8]) -> Result<f64> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| invalid("serialization", "invalid serialized number"))?
        .trim();
    match value {
        "1.#INF" | "+1.#INF" => Ok(f64::INFINITY),
        "-1.#INF" => Ok(f64::NEG_INFINITY),
        _ => value
            .parse()
            .map_err(|_| invalid("serialization", "invalid serialized number")),
    }
}
#[cfg(test)]
mod tests;
