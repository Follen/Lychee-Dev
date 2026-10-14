use super::{ContractError, Result};

pub const HEADER_BYTES: usize = 80;
pub const TRAILER_BYTES: usize = 40;
pub const MAX_PAYLOAD: usize = 512 << 10;
pub const MAGIC: &[u8; 8] = b"LYCMEM06";
pub const TRAILER: &[u8; 8] = b"LYCEND06";
pub type Token = [u8; 16];
pub const ZERO_TOKEN: Token = [0; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Identity = 1,
    Receipt = 2,
    Body = 3,
    Confirmation = 4,
    Input = 5,
    Bootstrap = 6,
}
impl TryFrom<u8> for Kind {
    type Error = ContractError;
    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Identity),
            2 => Ok(Self::Receipt),
            3 => Ok(Self::Body),
            4 => Ok(Self::Confirmation),
            5 => Ok(Self::Input),
            6 => Ok(Self::Bootstrap),
            _ => Err(ContractError("bridge.memory_header_bounds")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub nonce: Token,
    pub runtime: Token,
    pub ticket: Token,
    pub kind: Kind,
    pub state: u8,
    pub sequence: u32,
    pub length: u32,
    pub checksum: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record<'a> {
    pub header: Header,
    pub payload: &'a [u8],
}

pub fn parse_token(text: &str) -> Result<Token> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(ContractError("live.channel_token_invalid"));
    }
    let mut token = ZERO_TOKEN;
    hex::decode_to_slice(text, &mut token)
        .map_err(|_| ContractError("live.channel_token_invalid"))?;
    Ok(token)
}

pub fn checksum(bytes: &[u8]) -> u32 {
    let (mut lo, mut hi) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            lo += u32::from(byte);
            hi += lo;
        }
        lo %= 65521;
        hi %= 65521;
    }
    (hi << 16) | lo
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("checked header size"))
}

/// Decodes only the fixed header, so a reader can qualify allocation first.
pub fn decode_header(bytes: &[u8]) -> Result<Header> {
    if bytes.len() < HEADER_BYTES
        || &bytes[..8] != MAGIC
        || bytes[58] != 0
        || bytes[59] != 0
        || u32_at(bytes, 72) != HEADER_BYTES as u32
        || u32_at(bytes, 76) != checksum(&bytes[..76])
    {
        return Err(ContractError("bridge.memory_header_invalid"));
    }
    let kind = Kind::try_from(bytes[56])?;
    let length = u32_at(bytes, 64);
    if length as usize > MAX_PAYLOAD || bytes[57] == 0 {
        return Err(ContractError("bridge.memory_header_bounds"));
    }
    Ok(Header {
        nonce: bytes[8..24].try_into().expect("checked header size"),
        runtime: bytes[24..40].try_into().expect("checked header size"),
        ticket: bytes[40..56].try_into().expect("checked header size"),
        kind,
        state: bytes[57],
        sequence: u32_at(bytes, 60),
        length,
        checksum: u32_at(bytes, 68),
    })
}
pub fn decode_record(bytes: &[u8]) -> Result<Record<'_>> {
    let header = decode_header(bytes)?;
    let at = HEADER_BYTES + header.length as usize;
    if bytes.len() != at + TRAILER_BYTES
        || checksum(&bytes[HEADER_BYTES..at]) != header.checksum
        || &bytes[at..at + 8] != TRAILER
        || bytes[at + 8..at + 24] != header.nonce
        || bytes[at + 24..] != header.runtime
    {
        return Err(ContractError("bridge.memory_body_invalid"));
    }
    Ok(Record {
        header,
        payload: &bytes[HEADER_BYTES..at],
    })
}

/// Host-side encoding for offline fixtures and parity checks. Does not publish.
pub fn encode_record(header: Header, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD || header.state == 0 {
        return Err(ContractError("bridge.memory_record_invalid"));
    }
    let mut bytes = Vec::with_capacity(HEADER_BYTES + payload.len() + TRAILER_BYTES);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&header.nonce);
    bytes.extend_from_slice(&header.runtime);
    bytes.extend_from_slice(&header.ticket);
    bytes.extend_from_slice(&[header.kind as u8, header.state, 0, 0]);
    bytes.extend_from_slice(&header.sequence.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&checksum(payload).to_le_bytes());
    bytes.extend_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    bytes.extend_from_slice(&checksum(&bytes).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(TRAILER);
    bytes.extend_from_slice(&header.nonce);
    bytes.extend_from_slice(&header.runtime);
    Ok(bytes)
}

/// Selection predicates from Go memory/records.go. No predicate grants input.
#[derive(Debug, Clone, Copy)]
pub struct Selector {
    pub kind: Kind,
    pub nonce: Token,
    pub runtime: Token,
    pub ticket: Token,
    pub body: Option<BodyQualification>,
}
/// Length and checksum must come from the caller's verified exact HEAD.
/// This type records that evidence; constructing it does not verify a HEAD.
#[derive(Debug, Clone, Copy)]
pub struct BodyQualification {
    pub length: u32,
    pub checksum: u32,
}
impl Selector {
    pub fn matches(&self, header: &Header) -> bool {
        if self.kind != header.kind
            || (self.nonce != ZERO_TOKEN && self.nonce != header.nonce)
            || (self.runtime != ZERO_TOKEN && self.runtime != header.runtime)
            || (self.ticket != ZERO_TOKEN && self.ticket != header.ticket)
        {
            return false;
        }
        if header.kind == Kind::Body {
            return self.body.is_some_and(|body| {
                header.state == 3
                    && header.sequence != 0
                    && header.length as usize <= MAX_PAYLOAD
                    && header.length == body.length
                    && header.checksum == body.checksum
                    && self.nonce != ZERO_TOKEN
                    && self.runtime != ZERO_TOKEN
                    && self.ticket != ZERO_TOKEN
            });
        }
        header.length <= 16 << 10
    }
}

/// Bounded two-read contract reused by the next native named-field adapter.
/// No process is opened here. A caller supplies an already selected safe reader.
/// Returned bytes still require path/identity guard rechecks before consumption.
pub fn read_record<M: crate::mailbox::Memory>(
    source: &M,
    address: u64,
    selector: &Selector,
) -> Result<Vec<u8>> {
    let mut head = [0; HEADER_BYTES];
    source
        .read(address, &mut head, false)
        .map_err(|_| ContractError("memory.header_unavailable"))?;
    let header = decode_header(&head)?;
    if !selector.matches(&header) {
        return Err(ContractError("memory.record_mismatch"));
    }
    let size = HEADER_BYTES + header.length as usize + TRAILER_BYTES;
    source
        .charge(size as u64, 1)
        .map_err(|_| ContractError("memory.body_unavailable"))?;
    let mut bytes = vec![0; size];
    source
        .read(address, &mut bytes, false)
        .map_err(|_| ContractError("memory.body_unavailable"))?;
    if bytes[..HEADER_BYTES] != head {
        return Err(ContractError("memory.record_changed"));
    }
    decode_record(&bytes)?;
    Ok(bytes)
}
