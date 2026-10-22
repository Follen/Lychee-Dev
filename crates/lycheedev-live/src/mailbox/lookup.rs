//! Selected named-field Lua lookup only. Public Mailbox supplies observations, never authority.
use crate::{lua_layout::Layout, mailbox as v1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub type Result<T> = std::result::Result<T, String>;
pub trait Memory {
    fn verify(&self) -> Result<()>;
    fn read(&self, address: u64, bytes: &mut [u8], module: bool) -> Result<()>;
    /// Actual source facts, captured by the original read only. A generic exact
    /// reader cannot invent a native returned-byte count or OS error code.
    fn read_diagnostic(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
    ) -> std::result::Result<(), ReadError> {
        self.read(address, bytes, module)
            .map_err(ReadError::unknown)
    }
    /// Runtime adapters charge the same request ledger used by root discovery
    /// and OS reads. Historical in-memory fixtures retain their local limits.
    fn charge(&self, _allocation_bytes: u64, _work_units: u64) -> Result<()> {
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct ReadError {
    pub cause: String,
    pub returned_bytes: Option<usize>,
    pub reason: &'static str,
    pub io_attempted: Option<bool>,
    pub native_error_code: Option<u32>,
    pub region: Option<ReadRegion>,
}
impl ReadError {
    pub fn unknown(cause: String) -> Self {
        Self {
            cause,
            returned_bytes: None,
            reason: "short_read",
            io_attempted: None,
            native_error_code: None,
            region: None,
        }
    }
}
impl From<lycheedev_windows::process::RuntimeReadFailure> for ReadError {
    fn from(failure: lycheedev_windows::process::RuntimeReadFailure) -> Self {
        Self {
            cause: failure.cause,
            returned_bytes: Some(failure.returned_bytes),
            reason: failure.reason,
            io_attempted: Some(failure.io_attempted),
            native_error_code: failure.native_error_code,
            region: failure.region.map(|(kind, state, protect)| ReadRegion {
                kind,
                state,
                protect,
            }),
        }
    }
}
/// Bounded logical diagnostics. No address, payload, nonce, or extra read.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadFailure {
    pub path: String,
    pub requested_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_bytes: Option<usize>,
    pub source: &'static str,
    pub reason: &'static str,
    pub guard_recheck: &'static str,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ioAttempted")]
    pub io_attempted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_error_code: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<ReadRegion>,
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ReadRegion {
    #[serde(rename = "type")]
    pub kind: u32,
    pub state: u32,
    pub protect: u32,
}
#[derive(Debug, Clone)]
pub struct LookupFailure {
    pub cause: String,
    pub first_read_failure: Option<ReadFailure>,
}
impl From<String> for LookupFailure {
    fn from(cause: String) -> Self {
        Self {
            cause,
            first_read_failure: None,
        }
    }
}
pub type LookupResult = std::result::Result<Option<Record>, LookupFailure>;

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
    first_failure: Option<ReadFailure>,
}
impl<M: Memory> Access<'_, M> {
    fn note_failure(&mut self, path: &str, size: usize, module: bool, details: &ReadError) {
        if self.first_failure.is_none() {
            self.first_failure = Some(ReadFailure {
                path: path.into(),
                requested_bytes: size,
                returned_bytes: details.returned_bytes,
                source: if module { "module" } else { "private" },
                reason: details.reason,
                guard_recheck: "not_run",
                io_attempted: details.io_attempted,
                native_error_code: details.native_error_code,
                region: details.region,
            });
        }
    }
    fn note_guard(&mut self, state: &'static str) {
        if let Some(f) = &mut self.first_failure {
            f.guard_recheck = state;
        }
    }
    fn read(
        &mut self,
        address: u64,
        size: usize,
        module: bool,
        guard: bool,
        path: &str,
    ) -> Result<Vec<u8>> {
        if address == 0 || size == 0 || address.checked_add(size as u64).is_none() {
            self.note_failure(
                path,
                size,
                module,
                &ReadError {
                    cause: String::new(),
                    returned_bytes: Some(0),
                    reason: "address",
                    io_attempted: Some(false),
                    native_error_code: None,
                    region: None,
                },
            );
            return Err("mailbox.address".into());
        }
        if self.calls >= 512 || self.bytes + size > 1 << 20 {
            self.note_failure(
                path,
                size,
                module,
                &ReadError {
                    cause: String::new(),
                    returned_bytes: Some(0),
                    reason: "read_budget",
                    io_attempted: Some(false),
                    native_error_code: None,
                    region: None,
                },
            );
            return Err("mailbox.budget".into());
        }
        self.calls += 1;
        self.bytes += size;
        if let Err(cause) = self.memory.charge(size as u64 + 256, size as u64) {
            self.note_failure(
                path,
                size,
                module,
                &ReadError {
                    cause: String::new(),
                    returned_bytes: Some(0),
                    reason: "read_budget",
                    io_attempted: Some(false),
                    native_error_code: None,
                    region: None,
                },
            );
            return Err(cause);
        }
        let mut bytes = vec![0; size];
        if let Err(error) = self.memory.read_diagnostic(address, &mut bytes, module) {
            self.read_failed = true;
            self.note_failure(path, size, module, &error);
            return Err(error.cause);
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
        let guards = std::mem::take(&mut self.guards);
        let empty = guards.is_empty();
        for (address, bytes, module) in guards {
            let current = match self.read(address, bytes.len(), module, false, "guard_recheck") {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.note_guard("read_failed");
                    return Err(error);
                }
            };
            if current != bytes {
                self.note_guard("path_changed");
                return Err("mailbox.path_changed".into());
            }
        }
        if let Err(error) = self.memory.verify() {
            self.note_guard(
                if error.lines().any(|line| {
                    matches!(
                        line,
                        "memory.runtime_cancelled"
                            | "memory.runtime_deadline"
                            | "command.cancelled"
                            | "command.deadline"
                    )
                }) {
                    "cancelled"
                } else {
                    "identity_failed"
                },
            );
            return Err(error);
        }
        self.note_guard(if empty { "no_guards" } else { "stable" });
        Ok(())
    }
    fn string(&mut self, address: u64, max: usize, path: &str) -> Result<Vec<u8>> {
        let h = self.read(address, 32, false, false, &format!("{path}.header"))?;
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
        self.read(address + 32, n, false, true, &format!("{path}.bytes"))
    }
    fn record_string(&mut self, address: u64, selector: &v1::Selector) -> Result<Option<Record>> {
        let h = self.read(address, 32, false, false, "record.string_header")?;
        let n = ptr(&h, 24);
        let max = if selector.kind == v1::Kind::Body {
            selector.body.map_or(0, |body| body.length as usize)
        } else {
            16 << 10
        } + 120;
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
        let first = self.read(
            address + 32,
            v1::HEADER_BYTES,
            false,
            false,
            "record.wire_header",
        )?;
        let header =
            v1::decode_header(&first).map_err(|e| format!("mailbox.record_invalid: {e}"))?;
        // Match exact static HEAD predicates before allocating/reading BODY.
        if !selector.matches(&header) {
            return Ok(None);
        }
        if v1::HEADER_BYTES + header.length as usize + v1::TRAILER_BYTES != n as usize {
            return Err("mailbox.record_identity".into());
        }
        let bytes = self.read(address + 32, n as usize, false, false, "record.wire_bytes")?;
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
    fn lookup(&mut self, table: u64, key: &str, path: &str) -> Result<(u64, u8)> {
        let h = self.read(table, 72, false, false, &format!("{path}.table"))?;
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
            let b = self.read(at, 56, false, false, &format!("{path}.node"))?;
            self.guard(at + 24, &b[24..34])?;
            if b[32] == 4 {
                if b[33] != 0 {
                    return Err("mailbox.secret_key".into());
                }
                if self.string(ptr(&b, 24), 256, &format!("{path}.key"))? == key.as_bytes() {
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
        let (v, t) = self.lookup(parent, name, &format!("record.{name}"))?;
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
    pub release: String,
}
impl<M: Memory> Reader<'_, M> {
    pub(crate) fn lookup_runtime(
        &self,
        selector: &Selector,
        admission: &crate::runtime_binding::RuntimeLayoutAdmission,
    ) -> Result<Option<Record>> {
        admission.admits_layout(&self.layout)?;
        self.lookup_admitted(selector)
    }
    pub(crate) fn lookup_runtime_formal(
        &self,
        selector: &v1::Selector,
        admission: &crate::runtime_binding::RuntimeLayoutAdmission,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
    ) -> LookupResult {
        admission
            .admits_layout(&self.layout)
            .map_err(LookupFailure::from)?;
        self.lookup_formal_admitted(selector, accept)
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
        let mut formal = v1::Selector {
            kind: v1::Kind::Identity,
            nonce: v1::ZERO_TOKEN,
            runtime: v1::ZERO_TOKEN,
            ticket: v1::ZERO_TOKEN,
            body,
        };
        match selector {
            Selector::Identity => {}
            Selector::Input => formal.kind = v1::Kind::Input,
            Selector::Bootstrap => formal.kind = v1::Kind::Bootstrap,
            Selector::Receipt { nonce } | Selector::Confirmation { nonce } => {
                formal.kind = if matches!(selector, Selector::Confirmation { .. }) {
                    v1::Kind::Confirmation
                } else {
                    v1::Kind::Receipt
                };
                formal.nonce = v1::parse_token(nonce).map_err(|_| "mailbox.receipt_nonce")?;
            }
            Selector::Body { head } => {
                formal.kind = v1::Kind::Body;
                formal.nonce = v1::parse_token(&head.nonce).map_err(|_| "mailbox.body_head")?;
                formal.runtime = v1::parse_token(&head.runtime).map_err(|_| "mailbox.body_head")?;
                formal.ticket = v1::parse_token(&head.ticket).map_err(|_| "mailbox.body_head")?;
            }
        }
        self.lookup_formal_admitted(&formal, None)
            .map_err(|failure| failure.cause)
    }
    fn lookup_formal_admitted(
        &self,
        selector: &v1::Selector,
        mut accept: Option<&mut dyn FnMut(&Record) -> bool>,
    ) -> LookupResult {
        self.memory.verify().map_err(LookupFailure::from)?;
        // Reject incomplete BODY qualification and empty receipt nonces before
        // any Lua/private read. Tokens are bounded binary values by construction.
        if matches!(selector.kind, v1::Kind::Receipt | v1::Kind::Confirmation)
            && selector.nonce == v1::ZERO_TOKEN
        {
            return Err("mailbox.receipt_nonce".to_string().into());
        }
        if selector.kind == v1::Kind::Body
            && (selector
                .body
                .is_none_or(|b| b.length as usize > v1::MAX_PAYLOAD)
                || selector.nonce == v1::ZERO_TOKEN
                || selector.runtime == v1::ZERO_TOKEN
                || selector.ticket == v1::ZERO_TOKEN)
        {
            return Err("mailbox.body_authorization".to_string().into());
        }
        let mut access = Access {
            memory: self.memory,
            layout: &self.layout,
            guards: Vec::new(),
            calls: 0,
            bytes: 0,
            read_failed: false,
            first_failure: None,
        };
        let mut guards_checked = false;
        let result = self
            .lookup_access(selector, &mut access)
            .and_then(|record| {
                guards_checked = true;
                access.verify()?;
                Ok(record)
            });
        match result {
            Ok(record) => {
                let accepted = record
                    .filter(|record| accept.as_mut().is_none_or(|predicate| predicate(record)));
                self.memory.verify().map_err(|cause| LookupFailure {
                    cause,
                    first_read_failure: access.first_failure,
                })?;
                Ok(accepted)
            }
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
                    return Err(LookupFailure {
                        cause: "mailbox.path_changed".into(),
                        first_read_failure: access.first_failure,
                    });
                }
                let mut cause = error;
                for secondary in [recheck, final_failure].into_iter().flatten() {
                    if !cause.lines().any(|line| line == secondary) {
                        cause.push('\n');
                        cause.push_str(&secondary);
                    }
                }
                Err(LookupFailure {
                    cause,
                    first_read_failure: access.first_failure,
                })
            }
        }
    }
    fn lookup_access(
        &self,
        selector: &v1::Selector,
        a: &mut Access<'_, M>,
    ) -> Result<Option<Record>> {
        let root = a.read(self.root, 8, true, true, "root.state")?;
        let state = ptr(&root, 0);
        if state == 0 {
            return Err("mailbox.lua_state_null".into());
        }
        if state.checked_add(self.layout.state_globals + 24).is_none() {
            return Err("mailbox.state_address".into());
        }
        if a.read(state + 16, 1, false, true, "state.tag")? != [8] {
            return Err("mailbox.thread_tag".into());
        }
        let globals = a.read(
            state + self.layout.state_globals,
            24,
            false,
            false,
            "state.globals",
        )?;
        if globals[8] != 5 || globals[9] != 0 {
            return Err("mailbox.globals".into());
        }
        a.guard(state + self.layout.state_globals, &globals[..10])?;
        let (ns, ns_tag) = a.lookup(ptr(&globals, 0), "LycheeDevInternal", "namespace")?;
        if ns_tag == 0 {
            return Ok(None);
        }
        if ns_tag != 5 {
            return Err("mailbox.namespace_type".into());
        }
        let mailbox_field = if selector.kind == v1::Kind::Bootstrap {
            "BootstrapMailbox"
        } else {
            "Mailbox"
        };
        let (mb, mb_tag) = a.lookup(ns, mailbox_field, "mailbox")?;
        if mb_tag == 0 {
            return Ok(None);
        }
        if mb_tag != 5 {
            return Err("mailbox.publication_type".into());
        }
        for (name, want) in [("schema", v1::SCHEMA), ("release", self.release.as_str())] {
            let (v, t) = a.lookup(mb, name, &format!("mailbox.{name}"))?;
            if t != 4 {
                return Err(format!("mailbox.identity:{name}"));
            }
            let actual = a.string(v, 64, &format!("mailbox.{name}.value"))?;
            if actual != want.as_bytes() {
                return Err(format!(
                    "memory.mailbox_incompatible: mailbox_{name}_mismatch"
                ));
            }
        }
        let (rt, t) = a.lookup(mb, "runtime", "mailbox.runtime")?;
        if t != 4 {
            return Err("mailbox.runtime".into());
        }
        let runtime = String::from_utf8(a.string(rt, 32, "mailbox.runtime.value")?)
            .map_err(|_| "mailbox.runtime")?;
        if !token(&runtime) {
            return Err("mailbox.runtime".into());
        }
        let runtime_bytes = v1::parse_token(&runtime).map_err(|_| "mailbox.runtime")?;
        if selector.runtime != v1::ZERO_TOKEN && selector.runtime != runtime_bytes {
            return Ok(None);
        }
        let key;
        let (field, lookup_key) = match selector.kind {
            v1::Kind::Identity => ("identity", None),
            v1::Kind::Input => ("input", None),
            v1::Kind::Bootstrap => ("bootstrap", None),
            v1::Kind::Receipt | v1::Kind::Confirmation => {
                key = hex::encode(selector.nonce);
                ("receipts", Some(key.as_str()))
            }
            v1::Kind::Body => {
                key = hex::encode(selector.ticket);
                ("bodies", Some(key.as_str()))
            }
        };
        let (v, t) = if let Some(key) = lookup_key {
            let map = a.table(mb, field)?;
            a.lookup(map, key, &format!("record.{field}.entry"))?
        } else {
            a.lookup(mb, field, &format!("record.{field}"))?
        };
        if t == 0 {
            return Ok(None);
        }
        if t != 4 {
            return Err("mailbox.record_type".into());
        }
        let record = a.record_string(v, selector)?;
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
mod tests;
