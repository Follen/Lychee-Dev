//! DD 100130 protocol_v2: little-endian application packets inside WebSocket
//! binary messages. Response reasons and raw authentication payloads stay here.
use crate::input;
use flate2::{Decompress, FlushDecompress, Status};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde_json::{Value, json};
pub(super) const SID: u16 = 44204;
pub(super) const LIMIT: usize = 8 * 1024 * 1024;
pub(super) struct Response {
    pub(super) sid: u16,
    pub(super) command: u16,
    pub(super) code: u32,
    pub(super) sequence: Option<String>,
    pub(super) data: Value,
}
pub(super) fn encode(command: u16, sequence: &str, data: &Value) -> Result<Vec<u8>> {
    let ext = serde_json::to_string(&json!({"seq":sequence}))?;
    let prop = serde_json::to_vec(&json!({"extInfo":ext}))?;
    let data = serde_json::to_vec(data)?;
    let prop_size = u16::try_from(prop.len())
        .map_err(|_| invalid("CC request properties exceed the protocol length"))?;
    let data_size = u16::try_from(data.len())
        .map_err(|_| invalid("CC request body exceeds the protocol length"))?;
    let mut out = Vec::with_capacity(20 + prop.len() + data.len());
    out.extend_from_slice(&SID.to_le_bytes());
    out.extend_from_slice(&command.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(&prop_size.to_le_bytes());
    out.extend_from_slice(&prop);
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&data);
    Ok(out)
}
impl Response {
    pub(super) fn decode(bytes: &[u8], control: &OperationControl) -> Result<Self> {
        control.check()?;
        if bytes.len() < 16 || bytes.len() > LIMIT {
            return Err(invalid("CC response has an invalid packet size"));
        }
        let sid = u16::from_le_bytes(bytes[..2].try_into().unwrap());
        let command = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
        let flags = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let body;
        let bytes = if flags & 0x20000 != 0 {
            // The fixed client skips this four-byte compression prefix, then
            // uses the zlib wrapper. It does not interpret it as a field count.
            if bytes.len() < 20 {
                return Err(invalid("CC compressed response is truncated"));
            }
            body = inflate(&bytes[20..], control)?;
            body.as_slice()
        } else {
            &bytes[16..]
        };
        let mut reader = Reader { bytes, position: 0 };
        let prop = reader.string()?;
        let code = reader.u32()?;
        let _reason = reader.string()?;
        let data = reader.string()?;
        let prop = input::value(prop.as_bytes())
            .map_err(|_| invalid("CC response properties are not unambiguous JSON"))?;
        let data = input::value(data.as_bytes())
            .map_err(|_| invalid("CC response body is not unambiguous JSON"))?;
        let sequence = match prop.get("extInfo") {
            None | Some(Value::Null) => None,
            Some(Value::String(ext)) if ext.is_empty() => None,
            Some(Value::String(ext)) => {
                let value = input::value(ext.as_bytes())
                    .map_err(|_| invalid("CC response correlation is malformed"))?;
                value.get("seq").and_then(Value::as_str).map(str::to_owned)
            }
            _ => return Err(invalid("CC response correlation is not a JSON string")),
        };
        Ok(Self {
            sid,
            command,
            code,
            sequence,
            data,
        })
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(size)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("CC response field is truncated"))?;
        let out = &self.bytes[self.position..end];
        self.position = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<&'a str> {
        let size = u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as usize;
        std::str::from_utf8(self.take(size)?).map_err(|_| invalid("CC response field is not UTF-8"))
    }
}
fn inflate(input: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    let mut decoder = Decompress::new(true);
    let mut out = Vec::new();
    let mut buffer = [0; 65536];
    loop {
        control.check()?;
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let state = decoder
            .decompress(
                &input[before_in as usize..],
                &mut buffer,
                FlushDecompress::None,
            )
            .map_err(|_| invalid("CC response compression is invalid"))?;
        let count = (decoder.total_out() - before_out) as usize;
        if out.len().saturating_add(count) > LIMIT {
            return Err(invalid("CC decoded response exceeds 8 MiB"));
        }
        out.extend_from_slice(&buffer[..count]);
        if state == Status::StreamEnd {
            return Ok(out);
        }
        if decoder.total_in() == before_in && decoder.total_out() == before_out {
            return Err(invalid("CC compressed response is incomplete"));
        }
    }
}
pub(super) fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_cc_protocol", message, 5)
}
