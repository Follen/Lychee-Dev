//! Selected named-field Lua lookup only. Public Mailbox supplies observations, never authority.
use crate::{go_mailbox_v1 as v1, locator::Layout};
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
/// Existing consumers share the one fixed Go v3.2.0 wire codec. No 07 fallback.
pub fn checksum(bytes: &[u8]) -> u32 {
    v1::checksum(bytes)
}
pub fn encode(record: &Record) -> Result<Vec<u8>> {
    let header = v1::Header {
        nonce: v1::parse_token(&record.nonce).map_err(|e| e.to_string())?,
        runtime: v1::parse_token(&record.runtime).map_err(|e| e.to_string())?,
        ticket: v1::parse_token(&record.ticket).map_err(|e| e.to_string())?,
        kind: v1::Kind::try_from(record.kind).map_err(|e| e.to_string())?,
        state: record.state,
        sequence: record.sequence,
        length: 0,
        checksum: 0,
    };
    v1::encode_record(header, &record.payload).map_err(|e| e.to_string())
}
pub fn decode(bytes: &[u8]) -> Result<Record> {
    let record = v1::decode_record(bytes).map_err(|e| e.to_string())?;
    Ok(Record {
        address: 0,
        nonce: hex::encode(record.header.nonce),
        runtime: hex::encode(record.header.runtime),
        ticket: hex::encode(record.header.ticket),
        kind: record.header.kind as u8,
        state: record.header.state,
        sequence: record.header.sequence,
        checksum: record.header.checksum,
        payload: record.payload.to_vec(),
    })
}
impl Record {
    /// Borrow a previously decoded record for the fixed-release correlators.
    /// Rechecks bounded payload integrity, never publication or input authority.
    pub fn contract(&self) -> Result<v1::Record<'_>> {
        if self.payload.len() > v1::MAX_PAYLOAD
            || self.state == 0
            || self.checksum != checksum(&self.payload)
        {
            return Err("bridge.memory_body_invalid".into());
        }
        Ok(v1::Record {
            header: v1::Header {
                nonce: v1::parse_token(&self.nonce).map_err(|e| e.to_string())?,
                runtime: v1::parse_token(&self.runtime).map_err(|e| e.to_string())?,
                ticket: v1::parse_token(&self.ticket).map_err(|e| e.to_string())?,
                kind: v1::Kind::try_from(self.kind).map_err(|e| e.to_string())?,
                state: self.state,
                sequence: self.sequence,
                length: self.payload.len() as u32,
                checksum: self.checksum,
            },
            payload: &self.payload,
        })
    }
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
    read_failed: bool,
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
        if let Err(error) = self.memory.read(address, &mut bytes, module) {
            self.read_failed = true;
            return Err(error);
        }
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
        if h[16] != 4
            || n > max
            || address
                .checked_add(32)
                .and_then(|p| p.checked_add(n as u64))
                .is_none()
        {
            return Err("mailbox.string_layout".into());
        }
        self.guard(address + 16, &h[16..17])?;
        self.guard(address + 20, &h[20..32])?;
        if n == 0 {
            return Ok(Vec::new());
        }
        self.read(address + 32, n, false, true)
    }
    fn record_string(&mut self, address: u64, selector: &v1::Selector) -> Result<Option<Record>> {
        let h = self.read(address, 32, false, false)?;
        let n = ptr(&h, 24);
        let max = selector.body.map_or(16 << 10, |b| b.length as usize) + 120;
        if h[16] != 4
            || n < 120
            || n > max as u64
            || address
                .checked_add(32)
                .and_then(|p| p.checked_add(n))
                .is_none()
        {
            return Err("mailbox.record_length".into());
        }
        self.guard(address + 16, &h[16..17])?;
        self.guard(address + 24, &h[24..32])?;
        let first = self.read(address + 32, v1::HEADER_BYTES, false, false)?;
        let header =
            v1::decode_header(&first).map_err(|e| format!("mailbox.record_invalid: {e}"))?;
        // Match exact static HEAD predicates before allocating/reading BODY.
        if !selector.matches(&header) {
            return Ok(None);
        }
        if v1::HEADER_BYTES + header.length as usize + v1::TRAILER_BYTES != n as usize {
            return Err("mailbox.record_identity".into());
        }
        let bytes = self.read(address + 32, n as usize, false, false)?;
        if bytes[..v1::HEADER_BYTES] != first {
            return Err("mailbox.record_changed".into());
        }
        self.memory.charge(header.length as u64 + 256, n * 4)?;
        let mut record = decode(&bytes).map_err(|e| format!("mailbox.record_invalid: {e}"))?;
        record.address = address + 32;
        Ok(Some(record))
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
        if at == 0 {
            Ok((0, 0))
        } else {
            Err("mailbox.collision_budget".into())
        }
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
    /// Read-only startup diagnostics; never authorizes input.
    Bootstrap,
    Receipt {
        nonce: String,
    },
    Confirmation {
        nonce: String,
    },
    Body {
        head: Record,
    },
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
    /// Additional caller predicates run only after the full Lua path/identity
    /// guard. They cannot authorize old bytes or suppress a changed publication.
    pub fn lookup_with_accept(
        &self,
        selector: &Selector,
        accept: impl FnOnce(&Record) -> bool,
    ) -> Result<Option<Record>> {
        let record = self.lookup(selector)?;
        let accepted = record.filter(accept);
        // An Accept callback cannot suppress cancellation/selected-identity loss.
        self.memory.verify()?;
        Ok(accepted)
    }
    fn lookup_admitted(&self, selector: &Selector) -> Result<Option<Record>> {
        self.memory.verify()?;
        let body = match selector {
            Selector::Receipt { nonce } | Selector::Confirmation { nonce } => {
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
                let contract = head.contract().map_err(|_| "mailbox.body_head")?;
                let receipt =
                    v1::decode_publication_receipt(&contract).map_err(|_| "mailbox.body_head")?;
                if receipt.state != "reported" || receipt.report_bytes as usize > v1::MAX_PAYLOAD {
                    return Err("mailbox.body_head".into());
                }
                Some(v1::BodyQualification {
                    length: receipt.report_bytes,
                    checksum: receipt.report_checksum,
                })
            }
            _ => None,
        };
        let mut access = Access {
            memory: self.memory,
            layout: &self.layout,
            guards: Vec::new(),
            calls: 0,
            bytes: 0,
            read_failed: false,
        };
        let mut guards_checked = false;
        let result = self
            .lookup_access(selector, body, &mut access)
            .and_then(|record| {
                guards_checked = true;
                access.verify()?;
                Ok(record)
            });
        match result {
            Ok(record) => Ok(record),
            Err(error) => {
                // Recheck only the captured prefix. A changed path can explain
                // a retired object, but cannot hide cancellation or identity
                // loss found during that recheck or at the error-return boundary.
                let recheck = if !guards_checked
                    && (access.read_failed
                        || error.starts_with("mailbox.")
                        || error.starts_with("memory.mailbox_incompatible:"))
                    && error != "mailbox.path_changed"
                {
                    access.verify().err()
                } else {
                    None
                };
                let final_failure = self.memory.verify().err();
                if recheck.as_deref() == Some("mailbox.path_changed") && final_failure.is_none() {
                    return Err("mailbox.path_changed".into());
                }
                let mut cause = error;
                for secondary in [recheck, final_failure].into_iter().flatten() {
                    if !cause.lines().any(|line| line == secondary) {
                        cause.push('\n');
                        cause.push_str(&secondary);
                    }
                }
                Err(cause)
            }
        }
    }
    fn lookup_access(
        &self,
        selector: &Selector,
        body: Option<v1::BodyQualification>,
        a: &mut Access<'_, M>,
    ) -> Result<Option<Record>> {
        let root = a.read(self.root, 8, true, true)?;
        let state = ptr(&root, 0);
        if state == 0 {
            return Err("mailbox.lua_state_null".into());
        }
        if state.checked_add(self.layout.state_globals + 24).is_none() {
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
            return Ok(None);
        }
        if ns_tag != 5 {
            return Err("mailbox.namespace_type".into());
        }
        let mailbox_field = if matches!(selector, Selector::Bootstrap) {
            "BootstrapMailbox"
        } else {
            "Mailbox"
        };
        let (mb, mb_tag) = a.lookup(ns, mailbox_field)?;
        if mb_tag == 0 {
            return Ok(None);
        }
        if mb_tag != 5 {
            return Err("mailbox.publication_type".into());
        }
        for (name, want) in [("schema", v1::SCHEMA), ("release", self.release.as_str())] {
            let (v, t) = a.lookup(mb, name)?;
            if t != 4 {
                return Err(format!("mailbox.identity:{name}"));
            }
            let actual = a.string(v, 64)?;
            if actual != want.as_bytes() {
                if name == "schema"
                    && actual == v1::PREVIOUS_SCHEMA.as_bytes()
                    && matches!(selector, Selector::Identity | Selector::Input)
                {
                    continue; // Read-only activation evidence, never old business records.
                }
                return Err(format!(
                    "memory.mailbox_incompatible: mailbox_{name}_mismatch"
                ));
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
        let runtime_bytes = v1::parse_token(&runtime).map_err(|_| "mailbox.runtime")?;
        let mut wire = v1::Selector {
            kind: v1::Kind::Identity,
            nonce: v1::ZERO_TOKEN,
            runtime: v1::ZERO_TOKEN,
            ticket: v1::ZERO_TOKEN,
            body,
        };
        let (field, key) = match selector {
            Selector::Identity => ("identity", None),
            Selector::Input => {
                wire.kind = v1::Kind::Input;
                ("input", None)
            }
            Selector::Bootstrap => {
                wire.kind = v1::Kind::Bootstrap;
                ("bootstrap", None)
            }
            Selector::Receipt { nonce } | Selector::Confirmation { nonce } => {
                wire.kind = if matches!(selector, Selector::Confirmation { .. }) {
                    v1::Kind::Confirmation
                } else {
                    v1::Kind::Receipt
                };
                wire.nonce = v1::parse_token(nonce).map_err(|_| "mailbox.receipt_nonce")?;
                ("receipts", Some(nonce.as_str()))
            }
            Selector::Body { head } => {
                if head.runtime != runtime {
                    return Ok(None);
                }
                wire.kind = v1::Kind::Body;
                wire.runtime = runtime_bytes;
                wire.nonce = v1::parse_token(&head.nonce).map_err(|_| "mailbox.body_head")?;
                wire.ticket = v1::parse_token(&head.ticket).map_err(|_| "mailbox.body_head")?;
                ("bodies", Some(head.ticket.as_str()))
            }
        };
        let (v, t) = if let Some(key) = key {
            let map = a.table(mb, field)?;
            a.lookup(map, key)?
        } else {
            a.lookup(mb, field)?
        };
        if t == 0 {
            return Ok(None);
        }
        if t != 4 {
            return Err("mailbox.record_type".into());
        }
        let record = a.record_string(v, &wire)?;
        if record.as_ref().is_some_and(|r| r.runtime != runtime) {
            return Err("mailbox.record_identity".into());
        }
        Ok(record)
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
        b[7] = b'7';
        assert!(decode(&b).is_err());
    }
}
