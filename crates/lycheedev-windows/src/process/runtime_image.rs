//! Trusted image metadata for the fixed Go 3.2 runtime recipe. The sole issuer
//! hashes and parses one read-sharing-only, regular EXE handle under the same
//! request ledger as the OS-issued module. Disk bytes never supply a root RVA.
use super::{
    RuntimeBudget, RuntimeCost, RuntimeModule, RuntimeModuleIdentity, RuntimeReadKind,
    confirm_main_image,
};
use crate::Result;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    mem::size_of,
    os::windows::{
        fs::{FileExt, MetadataExt},
        io::FromRawHandle,
    },
};
use windows::{
    Win32::{
        Foundation::GENERIC_READ,
        Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING},
    },
    core::PCWSTR,
};

const EXECUTABLE_LIMIT: u64 = 512 << 20;
const HEADER_LIMIT: u64 = 1 << 20;
const TEXT_LIMIT: u64 = 128 << 20;
const CHUNK: usize = 1 << 20;
const MAX_SECTIONS: usize = 96;
const OPTIONAL64_BYTES: usize = 240;
const READ: u32 = 0x4000_0000;
const EXECUTE: u32 = 0x2000_0000;

/// Immutable descriptive section metadata. A section alone grants no access;
/// only the opaque layout, its issuing module and the original ledger do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeImageSection {
    start: u64,
    end: u64,
    flags: u32,
    text: bool,
}
impl RuntimeImageSection {
    pub fn start(&self) -> u64 {
        self.start
    }
    pub fn end(&self) -> u64 {
        self.end
    }
    pub fn flags(&self) -> u32 {
        self.flags
    }
    pub fn is_text(&self) -> bool {
        self.text
    }
}

/// Only the seven locating fields/groups from the fixed Go contract are guarded.
/// Unrelated runtime COFF/optional fields may differ without granting authority.
pub struct RuntimeImageGuard {
    rva: u64,
    bytes: Vec<u8>,
}
impl RuntimeImageGuard {
    pub fn rva(&self) -> u64 {
        self.rva
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Issued only after complete file hashing, fixed PE parsing, current runtime
/// metadata comparison and checked close. No caller-authored metadata, digest,
/// deserializer, default value or clone constructor can manufacture this proof.
pub struct RuntimeImageLayout {
    identity: RuntimeModuleIdentity,
    image_path: Vec<u16>,
    executable_sha256: String,
    sections: Vec<RuntimeImageSection>,
    text_index: usize,
    guards: Vec<RuntimeImageGuard>,
}
impl RuntimeImageLayout {
    pub fn identity(&self) -> &RuntimeModuleIdentity {
        &self.identity
    }
    pub fn executable_sha256(&self) -> &str {
        &self.executable_sha256
    }
    pub fn sections(&self) -> &[RuntimeImageSection] {
        &self.sections
    }
    pub fn text(&self) -> &RuntimeImageSection {
        &self.sections[self.text_index]
    }
    pub fn guards(&self) -> &[RuntimeImageGuard] {
        &self.guards
    }
    /// Heap capacity retained by this proof, already charged before allocation.
    /// Callers moving this value must not charge it a second time. Borrowing the
    /// descriptive views allocates nothing; copying them needs its own admission.
    pub fn retained_bytes(&self) -> usize {
        self.image_path.capacity() * size_of::<u16>()
            + self.executable_sha256.capacity()
            + self.sections.capacity() * size_of::<RuntimeImageSection>()
            + self.guards.capacity() * size_of::<RuntimeImageGuard>()
            + self
                .guards
                .iter()
                .map(|guard| guard.bytes.capacity())
                .sum::<usize>()
    }

    /// Revalidate the exact issuance, OS path and runtime locating bytes. This
    /// never reopens the EXE or replaces the original cumulative request budget.
    pub fn verify(&self, module: &RuntimeModule, budget: &mut RuntimeBudget) -> Result<()> {
        if self.identity != *module.identity() {
            return Err("memory.runtime_image_issuance_changed".into());
        }
        module.verify(budget)?;
        verify_path(module, &self.image_path, budget)?;
        verify_guards(&self.guards, budget, |rva, into, budget| {
            module.read_exact(rva, into, RuntimeReadKind::Metadata, budget)
        })?;
        verify_path(module, &self.image_path, budget)?;
        module.verify(budget)
    }
}

impl RuntimeModule {
    /// Bind one OS main module to the complete digest and fixed layout of its
    /// own locked EXE. The file is closed explicitly on every normal success or
    /// failure path; cancellation cannot skip the pre-reserved close attempt.
    pub fn verify_image_layout(&self, budget: &mut RuntimeBudget) -> Result<RuntimeImageLayout> {
        self.verify(budget)?;
        let image_path = confirm_main_image(&self.process, self.identity.base(), budget)?;
        let locked = LockedImage::open(&image_path, budget)?;
        let result = (|| {
            verify_path(self, &image_path, budget)?;
            let before = locked.stamp(budget)?;
            let digest = hash_image(locked.file(), before.length, budget)?;
            let parsed = parse_image(locked.file(), self.identity.size(), budget)?;
            if locked.stamp(budget)? != before {
                return Err("memory.module_executable_changed".into());
            }
            let layout = RuntimeImageLayout {
                identity: self.identity.clone(),
                image_path,
                executable_sha256: digest,
                sections: parsed.sections,
                text_index: parsed.text_index,
                guards: parsed.guards,
            };
            layout.verify(self, budget)?;
            Ok(layout)
        })();
        locked.finish(result, budget)
    }
}

fn verify_path(module: &RuntimeModule, expected: &[u16], budget: &mut RuntimeBudget) -> Result<()> {
    let actual = confirm_main_image(&module.process, module.identity.base(), budget)?;
    verify_path_values(expected, &actual, budget)
}
fn verify_path_values(expected: &[u16], actual: &[u16], budget: &mut RuntimeBudget) -> Result<()> {
    let equal = super::equal_fold_image_path(expected, actual, budget)?;
    let result = if equal {
        Ok(())
    } else {
        Err("memory.runtime_image_path_changed".into())
    };
    finish_step(result, budget)
}

fn verify_guards(
    guards: &[RuntimeImageGuard],
    budget: &mut RuntimeBudget,
    mut read: impl FnMut(u64, &mut [u8], &mut RuntimeBudget) -> Result<()>,
) -> Result<()> {
    // Fixed stack storage, not an unmetered heap allocation or caller-sized array.
    let mut current = [0u8; MAX_SECTIONS * 40];
    for guard in guards {
        budget.charge(RuntimeCost::work(guard.bytes.len() as u64))?;
        let into = current
            .get_mut(..guard.bytes.len())
            .ok_or("memory.runtime_image_guard_range")?;
        read(guard.rva, into, budget)?;
        if into != guard.bytes.as_slice() {
            return Err("memory.lua_mailbox_root_unsupported: runtime_image_metadata".into());
        }
        budget.check()?;
    }
    budget.check()
}

/// File::drop supplies panic-unwind RAII only. Every Result path uses finish,
/// consuming ownership into the existing checked Windows close boundary once.
struct LockedImage {
    file: Option<File>,
}
impl LockedImage {
    fn open(path: &[u16], budget: &mut RuntimeBudget) -> Result<Self> {
        if path.is_empty() || path.len() >= 32768 {
            return Err("memory.runtime_image_name".into());
        }
        budget.charge(RuntimeCost {
            allocation_bytes: ((path.len() + 1) * size_of::<u16>()) as u64,
            work_units: (2 * path.len()) as u64,
            ..RuntimeCost::default()
        })?;
        if path.contains(&0) {
            return Err("memory.runtime_image_name".into());
        }
        let mut terminated = Vec::with_capacity(path.len() + 1);
        terminated.extend_from_slice(path);
        terminated.push(0);
        // Reserve open, its error reporting and the eventual checked close.
        // No cleanup path needs a fresh admission after cancellation/deadline.
        budget.charge(RuntimeCost {
            os_call_units: 4,
            ..RuntimeCost::default()
        })?;
        // SAFETY: the owned, bounded UTF-16 path has no interior NUL and one
        // terminator. It outlives this synchronous call, which retains no pointer.
        // The handle is read-only/noninheritable; FILE_SHARE_READ denies both
        // writing and replacement until our sole file owner is explicitly closed.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(terminated.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|error| super::os_error("memory.module_executable_open", error))
        .map(|raw| {
            // SAFETY: successful CreateFileW returned one valid owned file
            // handle. Ownership transfers exactly once to File; no raw copy is
            // retained or independently closed. File supplies panic-unwind RAII,
            // and every normal Result path consumes it through checked close.
            unsafe { File::from_raw_handle(raw.0) }
        });
        match opened {
            Ok(file) => {
                let locked = Self { file: Some(file) };
                match budget.check() {
                    Ok(()) => Ok(locked),
                    Err(error) => locked.finish(Err(error), budget),
                }
            }
            Err(error) => finish_step(Err(error), budget),
        }
    }
    fn file(&self) -> &File {
        self.file.as_ref().expect("owned until checked close")
    }
    fn stamp(&self, budget: &mut RuntimeBudget) -> Result<FileStamp> {
        // std metadata may use two bounded handle-information queries.
        budget.charge(RuntimeCost {
            os_call_units: 4,
            ..RuntimeCost::default()
        })?;
        let metadata = finish_step(
            self.file()
                .metadata()
                .map_err(|error| format!("memory.module_executable_stat: {error}")),
            budget,
        )?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > EXECUTABLE_LIMIT {
            return Err("memory.module_executable_limit".into());
        }
        Ok(FileStamp {
            length: metadata.len(),
            created: metadata.creation_time(),
            written: metadata.last_write_time(),
            attributes: metadata.file_attributes(),
        })
    }
    fn finish<T>(mut self, result: Result<T>, budget: &RuntimeBudget) -> Result<T> {
        let file = self
            .file
            .take()
            .expect("checked close consumes exactly once");
        let close = crate::filesystem::close_file(file)
            .map_err(|error| format!("memory.module_executable_close: {error}"));
        // Include all errors: never convert an I/O/parse failure into cancellation,
        // hide a close failure, or publish a proof after the final control failed.
        finish_with_close(result, close, budget.check())
    }
}
#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    length: u64,
    created: u64,
    written: u64,
    attributes: u32,
}

fn finish_with_close<T>(result: Result<T>, close: Result<()>, control: Result<()>) -> Result<T> {
    let result = join_result(result, close);
    join_result(result, control)
}
fn join_result<T>(result: Result<T>, after: Result<()>) -> Result<T> {
    match (result, after) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(format!("{first}; {second}")),
    }
}
fn finish_step<T>(result: Result<T>, budget: &RuntimeBudget) -> Result<T> {
    join_result(result, budget.check())
}

// Private adapter: fixtures can exercise exact parsing/hash/fault boundaries,
// but no external caller can use arbitrary bytes to issue a RuntimeImageLayout.
trait ImageRead {
    fn read_at(&self, into: &mut [u8], offset: u64) -> Result<usize>;
}
impl ImageRead for File {
    fn read_at(&self, into: &mut [u8], offset: u64) -> Result<usize> {
        self.seek_read(into, offset)
            .map_err(|error| format!("memory.module_executable_read: {error}"))
    }
}
fn read_at(
    reader: &impl ImageRead,
    offset: u64,
    into: &mut [u8],
    budget: &mut RuntimeBudget,
) -> Result<usize> {
    if into.is_empty() || into.len() > CHUNK {
        return Err("memory.module_executable_read_range".into());
    }
    // std's synchronous offset read may use both a read and offset/overlapped
    // bookkeeping. Reserve conservatively rather than claim one kernel call.
    budget.charge(RuntimeCost {
        os_call_units: 8,
        ..RuntimeCost::read(into.len())
    })?;
    let count = finish_step(reader.read_at(into, offset), budget)?;
    if count > into.len() {
        return Err("memory.module_executable_read_count".into());
    }
    Ok(count)
}
fn hash_image(reader: &impl ImageRead, length: u64, budget: &mut RuntimeBudget) -> Result<String> {
    if length == 0 || length > EXECUTABLE_LIMIT {
        return Err("memory.module_executable_limit".into());
    }
    budget.charge(RuntimeCost::allocation(CHUNK))?;
    let mut buffer = vec![0; CHUNK];
    let mut hash = Sha256::new();
    let mut offset = 0;
    while offset < length {
        let wanted = (length - offset).min(CHUNK as u64) as usize;
        budget.charge(RuntimeCost::work(wanted as u64))?;
        let received = read_at(reader, offset, &mut buffer[..wanted], budget)?;
        if received == 0 {
            return Err("memory.module_executable_changed".into());
        }
        hash.update(&buffer[..received]);
        offset += received as u64;
        budget.check()?;
    }
    // Check complete EOF on the very same held handle, including growing files.
    if read_at(reader, length, &mut buffer[..1], budget)? != 0 {
        return Err("memory.module_executable_changed".into());
    }
    budget.charge(RuntimeCost {
        allocation_bytes: 64,
        work_units: 64,
        ..RuntimeCost::default()
    })?;
    let bytes = hash.finalize();
    let mut digest = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        digest.push(HEX[(byte >> 4) as usize] as char);
        digest.push(HEX[(byte & 15) as usize] as char);
    }
    budget.check()?;
    Ok(digest)
}

struct ParsedImage {
    sections: Vec<RuntimeImageSection>,
    text_index: usize,
    guards: Vec<RuntimeImageGuard>,
}
struct HeaderReader<'a, R: ImageRead> {
    reader: &'a R,
    bytes: u64,
}
impl<R: ImageRead> HeaderReader<'_, R> {
    fn read(&mut self, offset: u64, length: usize, budget: &mut RuntimeBudget) -> Result<Vec<u8>> {
        if offset > HEADER_LIMIT
            || length as u64 > HEADER_LIMIT - offset
            || length as u64 > HEADER_LIMIT - self.bytes
        {
            return Err("memory.lua_mailbox_root_unsupported: header_budget".into());
        }
        self.bytes += length as u64;
        budget.charge(RuntimeCost::allocation(length))?;
        let mut bytes = vec![0; length];
        let mut count = 0;
        while count < length {
            let got = read_at(
                self.reader,
                offset + count as u64,
                &mut bytes[count..],
                budget,
            )?;
            if got == 0 {
                return Err("memory.lua_mailbox_root_unsupported: short_read".into());
            }
            count += got;
        }
        budget.charge(RuntimeCost::work(length as u64))?;
        Ok(bytes)
    }
}
fn parse_image(
    reader: &impl ImageRead,
    module_size: u64,
    budget: &mut RuntimeBudget,
) -> Result<ParsedImage> {
    if module_size == 0 || module_size > EXECUTABLE_LIMIT {
        return Err("memory.lua_mailbox_root_unsupported: image_identity".into());
    }
    let mut reader = HeaderReader { reader, bytes: 0 };
    let dos = reader.read(0, 64, budget)?;
    if &dos[..2] != b"MZ" {
        return Err("memory.lua_mailbox_root_unsupported: dos_header".into());
    }
    let nt = u32::from_le_bytes(dos[0x3c..64].try_into().unwrap()) as u64;
    let signature = reader.read(nt, 4, budget)?;
    if signature != b"PE\0\0" {
        return Err("memory.lua_mailbox_root_unsupported: nt_signature".into());
    }
    let coff = reader.read(nt + 4, 20, budget)?;
    let machine = u16::from_le_bytes(coff[..2].try_into().unwrap());
    let count = u16::from_le_bytes(coff[2..4].try_into().unwrap()) as usize;
    let optional_size = u16::from_le_bytes(coff[16..18].try_into().unwrap()) as usize;
    if machine != 0x8664 || count == 0 || count > MAX_SECTIONS || optional_size < OPTIONAL64_BYTES {
        return Err("memory.lua_mailbox_root_unsupported: image_layout".into());
    }
    let optional = reader.read(nt + 24, optional_size, budget)?;
    let magic = u16::from_le_bytes(optional[..2].try_into().unwrap());
    let image_size = u32::from_le_bytes(optional[56..60].try_into().unwrap()) as u64;
    let header_size = u32::from_le_bytes(optional[60..64].try_into().unwrap()) as u64;
    if magic != 0x20b || image_size != module_size || header_size == 0 || header_size > module_size
    {
        return Err("memory.lua_mailbox_root_unsupported: image_layout".into());
    }
    let table = nt + 24 + optional_size as u64;
    if table + (count * 40) as u64 > header_size {
        return Err("memory.lua_mailbox_root_unsupported: section_headers_range".into());
    }
    let table_bytes = reader.read(table, count * 40, budget)?;
    budget.charge(RuntimeCost::allocation(
        count * size_of::<RuntimeImageSection>(),
    ))?;
    let mut sections: Vec<RuntimeImageSection> = Vec::with_capacity(count);
    let mut text_index = None;
    for raw in table_bytes.as_chunks::<40>().0 {
        budget.charge(RuntimeCost::work(sections.len() as u64 + 1))?;
        let length = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as u64;
        let start = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as u64;
        if length == 0 || start > image_size || length > image_size - start {
            return Err("memory.lua_mailbox_root_unsupported: section_range".into());
        }
        let section = RuntimeImageSection {
            start,
            end: start + length,
            flags: u32::from_le_bytes(raw[36..40].try_into().unwrap()),
            text: raw[..8] == *b".text\0\0\0",
        };
        if sections
            .iter()
            .any(|prior| start < prior.end && section.end > prior.start)
        {
            return Err("memory.lua_mailbox_root_unsupported: section_overlap".into());
        }
        if section.text && text_index.replace(sections.len()).is_some() {
            return Err("memory.lua_mailbox_root_unsupported: text_section".into());
        }
        sections.push(section);
    }
    let text_index = text_index.ok_or("memory.lua_mailbox_root_unsupported: text_section")?;
    let text = sections[text_index];
    if text.flags & (READ | EXECUTE) != READ | EXECUTE || text.end - text.start > TEXT_LIMIT {
        return Err("memory.lua_mailbox_root_unsupported: text_section".into());
    }
    // Reuse already-read bytes from the locked file. Never parse or traverse COFF
    // symbols, raw-data offsets, relocations, string tables or executable code.
    budget.charge(RuntimeCost::allocation(
        7 * size_of::<RuntimeImageGuard>() + 12,
    ))?;
    let guards = vec![
        RuntimeImageGuard {
            rva: 0,
            bytes: dos[..2].to_vec(),
        },
        RuntimeImageGuard {
            rva: 0x3c,
            bytes: dos[0x3c..64].to_vec(),
        },
        RuntimeImageGuard {
            rva: nt,
            bytes: signature,
        },
        RuntimeImageGuard {
            rva: nt + 6,
            bytes: coff[2..4].to_vec(),
        },
        RuntimeImageGuard {
            rva: nt + 20,
            bytes: coff[16..18].to_vec(),
        },
        RuntimeImageGuard {
            rva: nt + 24,
            bytes: optional[..2].to_vec(),
        },
        RuntimeImageGuard {
            rva: table,
            bytes: table_bytes,
        },
    ];
    budget.check()?;
    Ok(ParsedImage {
        sections,
        text_index,
        guards,
    })
}

#[cfg(test)]
#[path = "runtime_image/tests.rs"]
mod tests;
