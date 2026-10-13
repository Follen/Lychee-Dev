//! Selected named-field Lua lookup only. Public Mailbox supplies observations, never authority.
use crate::locator::Layout;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub type Result<T> = std::result::Result<T, String>;
pub trait Memory {
    fn verify(&self) -> Result<()>;
    fn read(&self, address: u64, bytes: &mut [u8], module: bool) -> Result<()>;
    /// Runtime adapters charge the same request ledger used by root discovery
    /// and OS reads. Historical in-memory fixtures retain their local limits.
    fn charge(&self, _allocation_bytes: u64, _work_units: u64) -> Result<()> {
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    #[serde(skip)]
    pub address: u64,
    pub nonce: String,
    pub runtime: String,
    pub ticket: String,
    pub kind: u8,
    pub state: u8,
    pub sequence: u32,
    pub checksum: u32,
    pub payload: Vec<u8>,
}
pub fn checksum(b: &[u8]) -> u32 {
    let (mut lo, mut hi) = (1u32, 0u32);
    for chunk in b.chunks(5552) {
        for v in chunk {
            lo += *v as u32;
            hi += lo;
        }
        lo %= 65521;
        hi %= 65521;
    }
    (hi << 16) | lo
}
pub fn encode(record: &Record) -> Result<Vec<u8>> {
    if record.payload.len() > 524288 {
        return Err("wire.payload_budget".into());
    }
    let mut h = b"LYCMEM07".to_vec();
    for t in [&record.nonce, &record.runtime, &record.ticket] {
        let v = hex::decode(t).map_err(|_| "wire.token")?;
        if v.len() != 16 {
            return Err("wire.token".into());
        }
        h.extend(v);
    }
    h.extend([record.kind, record.state, 0, 0]);
    h.extend(record.sequence.to_le_bytes());
    h.extend((record.payload.len() as u32).to_le_bytes());
    h.extend(checksum(&record.payload).to_le_bytes());
    h.extend(80u32.to_le_bytes());
    h.extend(checksum(&h).to_le_bytes());
    h.extend(&record.payload);
    h.extend(b"LYCEND07");
    h.extend(hex::decode(&record.nonce).unwrap());
    h.extend(hex::decode(&record.runtime).unwrap());
    Ok(h)
}
pub fn decode(bytes: &[u8]) -> Result<Record> {
    if bytes.len() < 120
        || bytes.len() > 524408
        || &bytes[..8] != b"LYCMEM07"
        || u32at(bytes, 72)? != 80
        || checksum(&bytes[..76]) != u32at(bytes, 76)?
    {
        return Err("wire.header".into());
    }
    let len = u32at(bytes, 64)? as usize;
    if len + 120 != bytes.len()
        || &bytes[80 + len..88 + len] != b"LYCEND07"
        || bytes[8..40] != bytes[88 + len..120 + len]
        || checksum(&bytes[80..80 + len]) != u32at(bytes, 68)?
    {
        return Err("wire.integrity".into());
    }
    if !(1..=5).contains(&bytes[56]) || bytes[58] != 0 || bytes[59] != 0 {
        return Err("wire.kind".into());
    }
    Ok(Record {
        address: 0,
        nonce: hex::encode(&bytes[8..24]),
        runtime: hex::encode(&bytes[24..40]),
        ticket: hex::encode(&bytes[40..56]),
        kind: bytes[56],
        state: bytes[57],
        sequence: u32at(bytes, 60)?,
        checksum: u32at(bytes, 68)?,
        payload: bytes[80..80 + len].to_vec(),
    })
}
fn u32at(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4).ok_or("wire.short")?.try_into().unwrap(),
    ))
}
fn ptr(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn hash(text: &str) -> u32 {
    let b = text.as_bytes();
    let mut h = b.len() as u32;
    let step = (b.len() >> 5) + 1;
    let mut n = b.len();
    while n >= step {
        h ^= h
            .wrapping_shl(5)
            .wrapping_add(h >> 2)
            .wrapping_add(b[n - 1] as u32);
        n -= step;
    }
    h
}
struct Access<'a, M: Memory> {
    memory: &'a M,
    layout: &'a Layout,
    guards: Vec<(u64, Vec<u8>, bool)>,
    calls: u16,
    bytes: usize,
}
impl<M: Memory> Access<'_, M> {
    fn read(&mut self, address: u64, size: usize, module: bool, guard: bool) -> Result<Vec<u8>> {
        if address == 0 || address.checked_add(size as u64).is_none() {
            return Err("mailbox.address".into());
        }
        if self.calls >= 512 || self.bytes + size > 1 << 20 {
            return Err("mailbox.budget".into());
        }
        self.calls += 1;
        self.bytes += size;
        self.memory.charge(size as u64, size as u64)?;
        let mut bytes = vec![0; size];
        self.memory.read(address, &mut bytes, module)?;
        if guard {
            self.memory.charge(size as u64 + 128, size as u64)?;
            self.guards.push((address, bytes.clone(), module));
        }
        Ok(bytes)
    }
    fn guard(&mut self, address: u64, b: &[u8]) -> Result<()> {
        self.memory.charge(b.len() as u64 + 128, b.len() as u64)?;
        self.guards.push((address, b.to_vec(), false));
        Ok(())
    }
    fn verify(&mut self) -> Result<()> {
        for (address, bytes, module) in std::mem::take(&mut self.guards) {
            if self.read(address, bytes.len(), module, false)? != bytes {
                return Err("mailbox.path_changed".into());
            }
        }
        self.memory.verify()
    }
    fn string(&mut self, address: u64, max: usize) -> Result<Vec<u8>> {
        let h = self.read(address, 32, false, false)?;
        let n = ptr(&h, 24) as usize;
        if h[16] != 4 || n > max {
            return Err("mailbox.string_layout".into());
        }
        self.guard(address + 16, &h[16..17])?;
        self.guard(address + 20, &h[20..32])?;
        if n == 0 {
            return Ok(Vec::new());
        }
        self.read(address + 32, n, false, true)
    }
    fn record_string(&mut self, address: u64, max: usize) -> Result<Vec<u8>> {
        let h = self.read(address, 32, false, false)?;
        let n = ptr(&h, 24) as usize;
        if h[16] != 4 || n < 120 || n > max {
            return Err("mailbox.record_length".into());
        }
        self.guard(address + 16, &h[16..17])?;
        self.guard(address + 20, &h[20..32])?;
        let bytes = self.read(address + 32, n, false, false)?;
        self.guard(address + 32, &bytes[..80])?;
        self.guard(address + 32 + n as u64 - 40, &bytes[n - 40..])?;
        Ok(bytes)
    }
    #[allow(
        clippy::manual_is_multiple_of,
        reason = "integer is_multiple_of requires Rust newer than the 1.85 MSRV"
    )]
    fn lookup(&mut self, table: u64, key: &str) -> Result<(u64, u8)> {
        let h = self.read(table, 72, false, false)?;
        if h[16] != 5 || h[19] > 20 {
            return Err("mailbox.table_layout".into());
        }
        let nodes = ptr(&h, 40);
        let count = 1u64 << h[19];
        if nodes == 0 || nodes.checked_add(count * 56).is_none() {
            return Err("mailbox.nodes".into());
        }
        self.guard(table + 16, &h[16..17])?;
        self.guard(table + 19, &h[19..20])?;
        self.guard(table + 40, &h[40..48])?;
        self.memory.charge(0, key.len() as u64)?;
        let mut at = nodes + u64::from(hash(key) & (count as u32 - 1)) * self.layout.node_bytes;
        let mut seen = BTreeSet::new();
        for _ in 0..64 {
            if at == 0 {
                return Ok((0, 0));
            }
            if at < nodes || at >= nodes + count * 56 || (at - nodes) % 56 != 0 {
                return Err("mailbox.collision_chain".into());
            }
            self.memory.charge(128, 64)?;
            if !seen.insert(at) {
                return Err("mailbox.collision_chain".into());
            }
            let b = self.read(at, 56, false, false)?;
            self.guard(at + 24, &b[24..34])?;
            if b[32] == 4 {
                if b[33] != 0 {
                    return Err("mailbox.secret_key".into());
                }
                if self.string(ptr(&b, 24), 256)? == key.as_bytes() {
                    if b[9] != 0 {
                        return Err("mailbox.secret_value".into());
                    }
                    self.guard(at, &b[..10])?;
                    return Ok((ptr(&b, 0), b[8]));
                }
            }
            self.guard(at + 48, &b[48..56])?;
            at = ptr(&b, 48);
        }
        Err("mailbox.collision_budget".into())
    }
    fn table(&mut self, parent: u64, name: &str) -> Result<u64> {
        let (v, t) = self.lookup(parent, name)?;
        if t != 5 {
            return Err(format!("mailbox.table_missing:{name}"));
        }
        Ok(v)
    }
}
#[derive(Debug, Clone)]
pub enum Selector {
    Identity,
    Input,
    Receipt { nonce: String },
    Body { head: Record },
}
pub struct Reader<'a, M: Memory> {
    pub memory: &'a M,
    pub root: u64,
    pub layout: Layout,
    pub module_hash: String,
    pub release: String,
}
impl<M: Memory> Reader<'_, M> {
    pub fn lookup(&self, selector: &Selector) -> Result<Option<Record>> {
        self.layout.admit(&self.module_hash)?;
        self.lookup_admitted(selector)
    }
    pub(crate) fn lookup_runtime(
        &self,
        selector: &Selector,
        admission: &crate::runtime_binding::RuntimeLayoutAdmission,
    ) -> Result<Option<Record>> {
        admission.admits_layout(&self.layout)?;
        self.lookup_admitted(selector)
    }
    fn lookup_admitted(&self, selector: &Selector) -> Result<Option<Record>> {
        self.memory.verify()?;
        // Caller-owned selector strings/JSON cannot create unbounded work before
        // the first path read. A head is only an observation; authentication is
        // still the protocol caller's responsibility.
        let head_json = match selector {
            Selector::Receipt { nonce } => {
                if !token(nonce) {
                    return Err("mailbox.receipt_nonce".into());
                }
                None
            }
            Selector::Body { head } => {
                if !token(&head.nonce)
                    || !token(&head.runtime)
                    || !token(&head.ticket)
                    || head.kind != 2
                    || head.payload.len() > 16384
                {
                    return Err("mailbox.body_head".into());
                }
                self.memory.charge(
                    head.payload.len() as u64 * 128 + 4096,
                    head.payload.len() as u64 * 4,
                )?;
                Some(
                    serde_json::from_slice::<serde_json::Value>(&head.payload)
                        .map_err(|_| "mailbox.body_head")?,
                )
            }
            _ => None,
        };
        let mut a = Access {
            memory: self.memory,
            layout: &self.layout,
            guards: Vec::new(),
            calls: 0,
            bytes: 0,
        };
        let root = a.read(self.root, 8, true, true)?;
        let state = ptr(&root, 0);
        if state == 0 || state.checked_add(self.layout.state_globals + 24).is_none() {
            return Err("mailbox.state_address".into());
        }
        if a.read(state + 16, 1, false, true)? != [8] {
            return Err("mailbox.thread_tag".into());
        }
        let globals = a.read(state + self.layout.state_globals, 24, false, false)?;
        if globals[8] != 5 || globals[9] != 0 {
            return Err("mailbox.globals".into());
        }
        a.guard(state + self.layout.state_globals, &globals[..10])?;
        let (ns, ns_tag) = a.lookup(ptr(&globals, 0), "LycheeDevInternal")?;
        if ns_tag == 0 {
            a.verify()?;
            return Ok(None);
        }
        if ns_tag != 5 {
            return Err("mailbox.namespace_type".into());
        }
        let (mb, mb_tag) = a.lookup(ns, "Mailbox")?;
        if mb_tag == 0 {
            a.verify()?;
            return Ok(None);
        }
        if mb_tag != 5 {
            return Err("mailbox.publication_type".into());
        }
        for (name, want) in [
            ("schema", crate::MAILBOX_SCHEMA),
            ("release", self.release.as_str()),
        ] {
            let (v, t) = a.lookup(mb, name)?;
            if t != 4 || a.string(v, 64)? != want.as_bytes() {
                return Err(format!("mailbox.identity:{name}"));
            }
        }
        let (rt, t) = a.lookup(mb, "runtime")?;
        if t != 4 {
            return Err("mailbox.runtime".into());
        }
        let runtime = String::from_utf8(a.string(rt, 32)?).map_err(|_| "mailbox.runtime")?;
        if !token(&runtime) {
            return Err("mailbox.runtime".into());
        }
        let (field, key, maximum) = match selector {
            Selector::Identity => ("identity", None, 16384),
            Selector::Input => ("input", None, 16384),
            Selector::Receipt { nonce } => ("receipts", Some(nonce.as_str()), 16384),
            Selector::Body { head } => {
                if head.runtime != runtime || head.kind != 2 {
                    return Err("mailbox.body_head".into());
                }
                let h = head_json.as_ref().ok_or("mailbox.body_head")?;
                if h["state"] != "reported"
                    || h["ticket"] != head.ticket
                    || h["nonce"] != head.nonce
                {
                    return Err("mailbox.body_head".into());
                }
                let max = h["reportBytes"].as_u64().ok_or("mailbox.body_length")? as usize;
                if max > 524288 {
                    return Err("mailbox.body_length".into());
                }
                ("bodies", Some(head.ticket.as_str()), max)
            }
        };
        let (v, t) = if let Some(key) = key {
            let map = a.table(mb, field)?;
            a.lookup(map, key)?
        } else {
            a.lookup(mb, field)?
        };
        if t == 0 {
            a.verify()?;
            return Ok(None);
        }
        if t != 4 {
            return Err("mailbox.record_type".into());
        }
        let bytes = a.record_string(v, maximum + 120)?;
        self.memory
            .charge(bytes.len() as u64 + 256, bytes.len() as u64 * 4)?;
        let mut r = decode(&bytes)?;
        r.address = v + 32;
        if r.runtime != runtime {
            return Err("mailbox.stale_record".into());
        }
        match selector {
            Selector::Identity if r.kind != 1 => return Err("mailbox.record_kind".into()),
            Selector::Input if r.kind != 4 => return Err("mailbox.record_kind".into()),
            Selector::Receipt { .. } if r.kind != 2 => return Err("mailbox.record_kind".into()),
            Selector::Receipt { nonce } if &r.nonce != nonce => {
                return Err("mailbox.receipt_nonce".into());
            }
            Selector::Body { head } => {
                let h = head_json.as_ref().ok_or("mailbox.body_head")?;
                if r.kind != 3
                    || r.nonce != head.nonce
                    || r.ticket != head.ticket
                    || r.payload.len() != maximum
                    || h["reportChecksum"].as_u64() != Some(r.checksum as u64)
                {
                    return Err("mailbox.body_identity".into());
                }
            }
            _ => {}
        }
        a.verify()?;
        Ok(Some(r))
    }
}
fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && value.bytes().any(|c| c != b'0')
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_roundtrip_and_tearing() {
        let r = Record {
            address: 0,
            nonce: "1".repeat(32),
            runtime: "2".repeat(32),
            ticket: "3".repeat(32),
            kind: 2,
            state: 3,
            sequence: 9,
            checksum: 0,
            payload: b"result".to_vec(),
        };
        let mut b = encode(&r).unwrap();
        assert_eq!(decode(&b).unwrap().payload, r.payload);
        b[82] ^= 1;
        assert!(decode(&b).is_err());
    }
    #[test]
    fn old_wire_rejected() {
        let r = Record {
            address: 0,
            nonce: "1".repeat(32),
            runtime: "2".repeat(32),
            ticket: "3".repeat(32),
            kind: 2,
            state: 3,
            sequence: 0,
            checksum: 0,
            payload: vec![],
        };
        let mut b = encode(&r).unwrap();
        b[7] = b'6';
        assert!(decode(&b).is_err());
    }
}
