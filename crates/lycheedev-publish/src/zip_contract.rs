//! ZIP member identity shared by backup admission and addon packages.
use crate::local_backup;
use lycheedev_core::{Result, ToolkitError};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
};
#[derive(Default)]
pub(crate) struct Paths {
    seen: BTreeSet<Vec<String>>,
    tree: BTreeMap<Vec<String>, (Vec<String>, bool)>,
}
impl Paths {
    pub(crate) fn admit(
        &mut self,
        raw: &mut File,
        name: &[u8],
        local_offset: u64,
        central_offset: u64,
        directory: bool,
        encrypted: bool,
    ) -> Result<Vec<String>> {
        raw.seek(SeekFrom::Start(local_offset))?;
        let mut local = [0u8; 30];
        raw.read_exact(&mut local)
            .map_err(|_| invalid("ZIP local header is incomplete"))?;
        if &local[..4] != b"PK\x03\x04"
            || usize::from(u16::from_le_bytes(local[26..28].try_into().unwrap())) != name.len()
        {
            return Err(invalid(
                "ZIP local member identity differs from its central directory",
            ));
        }
        let mut local_name = vec![0; name.len()];
        raw.read_exact(&mut local_name)
            .map_err(|_| invalid("ZIP local member name is incomplete"))?;
        raw.seek(SeekFrom::Start(central_offset))?;
        let mut central = [0u8; 46];
        raw.read_exact(&mut central)
            .map_err(|_| invalid("ZIP central header is incomplete"))?;
        if &central[..4] != b"PK\x01\x02"
            || usize::from(u16::from_le_bytes(central[28..30].try_into().unwrap())) != name.len()
        {
            return Err(invalid("ZIP central member identity is invalid"));
        }
        let mut central_name = vec![0; name.len()];
        raw.read_exact(&mut central_name)
            .map_err(|_| invalid("ZIP central member name is incomplete"))?;
        if local_name != name || central_name != name || local[6..10] != central[8..12] {
            return Err(invalid("ZIP local and central member headers disagree"));
        }
        let attrs = u32::from_le_bytes(central[38..42].try_into().unwrap());
        let mode = (attrs >> 16) & 0o170000;
        if encrypted
            || u16::from_le_bytes(local[6..8].try_into().unwrap()) & 1 != 0
            || attrs & 0x400 != 0
            || !matches!(mode, 0 | 0o100000 | 0o040000)
            || (mode == 0o040000 && !directory)
            || (mode == 0o100000 && directory)
        {
            return Err(invalid(
                "ZIP contains an encrypted, linked or unsupported member",
            ));
        }
        let name =
            std::str::from_utf8(name).map_err(|_| invalid("ZIP member name is not UTF-8"))?;
        let name = if directory {
            name.strip_suffix('/')
                .ok_or_else(|| invalid("ZIP directory lacks a forward slash"))?
        } else {
            name
        };
        let parts = name
            .split('/')
            .map(|part| local_backup::name(part).map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        let folded = parts
            .iter()
            .map(|part| part.to_lowercase())
            .collect::<Vec<_>>();
        if !self.seen.insert(folded.clone()) {
            return Err(invalid("ZIP repeats a Windows member path"));
        }
        for length in 1..=parts.len() {
            let key = folded[..length].to_vec();
            let entry = (parts[..length].to_vec(), length < parts.len() || directory);
            if self.tree.get(&key).is_some_and(|prior| prior != &entry) {
                return Err(invalid("ZIP has conflicting Windows paths"));
            }
            self.tree.insert(key, entry);
        }
        Ok(parts)
    }
}
pub(crate) fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("publish.zip_invalid", message, 4)
}
