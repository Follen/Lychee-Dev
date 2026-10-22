//! Demand-only pinned originals. One existing owned Git process serves the
//! request; no document is fetched before its consumer asks for it.
use super::{MAX_SOURCE_BYTES, mirror, source_path, valid_digest, validate_pin};
use crate::process::GitBatchProcess;
use lycheedev_core::{Result, ToolkitError, control::OperationControl, selection::SourcePin};
use sha1::{Digest, Sha1};
use std::{
    io::{BufRead, Read},
    path::Path,
};

const MAX_TREE_BYTES: usize = 4 << 20;
const MAX_ANCESTOR_BYTES: usize = 16 << 20;
// Fixed-width keys and a flat sorted Vec make the complete memo allocation
// charge visible: payload capacities plus record-vector capacity, not an
// estimate of private map-node allocation. This is not an all-process RSS cap.
struct TreeMemo {
    object: [u8; 40],
    raw: Vec<u8>,
}

fn integrity(message: &str) -> ToolkitError {
    ToolkitError::new("codebase.source_integrity", message, 4)
}
fn budget(message: &str) -> ToolkitError {
    ToolkitError::new("codebase.source_budget", message, 3)
}

/// Used only by the fixed Git duplex worker. Each frame is admitted before its
/// allocation; errors terminate that stream rather than guessing a new boundary.
pub(crate) fn read_object_frame(
    reader: &mut impl BufRead,
    object: &str,
    kind: &str,
    maximum: usize,
) -> Result<Vec<u8>> {
    if maximum > MAX_SOURCE_BYTES {
        return Err(budget("Git object budget exceeded"));
    }
    let mut header = Vec::with_capacity(128);
    (&mut *reader).take(129).read_until(b'\n', &mut header)?;
    if header.len() > 128 || header.last() != Some(&b'\n') {
        return Err(integrity("Missing or oversized Git batch header"));
    }
    let header = std::str::from_utf8(&header[..header.len() - 1])
        .map_err(|_| integrity("Invalid Git batch header"))?;
    let fields: Vec<_> = header.split(' ').collect();
    if fields.len() != 3 || fields[0] != object || fields[1] != kind {
        return Err(integrity("Pinned object type or identity differs"));
    }
    let size = fields[2]
        .parse::<usize>()
        .map_err(|_| integrity("Invalid Git batch size"))?;
    if size > maximum {
        return Err(budget("Git object budget exceeded"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| budget("Git object allocation was not admitted"))?;
    bytes.resize(size, 0);
    reader.read_exact(&mut bytes).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            integrity("Truncated Git batch bytes")
        } else {
            error.into()
        }
    })?;
    let mut separator = [0];
    reader.read_exact(&mut separator).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            integrity("Truncated Git batch bytes")
        } else {
            error.into()
        }
    })?;
    if separator != [b'\n'] {
        return Err(integrity("Invalid Git batch separator"));
    }
    let mut hash = Sha1::new();
    hash.update(format!("{kind} {size}\0"));
    hash.update(&bytes);
    if format!("{:x}", hash.finalize()) != object {
        return Err(integrity("Pinned Git object hash differs"));
    }
    Ok(bytes)
}

pub(crate) struct Originals<'a> {
    home: &'a Path,
    pin: SourcePin,
    process: Option<GitBatchProcess>,
    root: Option<String>,
    trees: Vec<TreeMemo>,
    tree_bytes: usize,
    memo_limit: usize,
    original_room: usize,
}
impl<'a> Originals<'a> {
    /// memo_room is the caller's remaining original-text admission, not a new
    /// independent 16 MiB allowance. The original frame/path bounds still apply.
    pub(crate) fn new(home: &'a Path, pin: SourcePin, memo_room: usize) -> Self {
        Self {
            home,
            pin,
            process: None,
            root: None,
            trees: Vec::new(),
            tree_bytes: 0,
            memo_limit: memo_room.min(MAX_ANCESTOR_BYTES),
            original_room: memo_room,
        }
    }
    fn process(&mut self, control: &OperationControl) -> Result<&mut GitBatchProcess> {
        if self.process.is_none() {
            self.process = Some(GitBatchProcess::spawn(
                &mirror(self.home, &self.pin.repository),
                control,
            )?);
        }
        Ok(self.process.as_mut().expect("opened fixed Git reader"))
    }
    pub(crate) fn read(
        &mut self,
        path: &str,
        max: usize,
        control: &OperationControl,
    ) -> Result<Vec<u8>> {
        control.check()?;
        validate_pin(&self.pin)?;
        if !source_path(path) {
            return Err(ToolkitError::new(
                "codebase.invalid_span",
                "Invalid source path",
                2,
            ));
        }
        let components: Vec<_> = path.split('/').collect();
        if components.len() > 64 || max > MAX_SOURCE_BYTES {
            return Err(budget("Source path/decode budget exceeded"));
        }
        if self.root.is_none() {
            let commit = self.pin.exact_commit.clone();
            let raw = self
                .process(control)?
                .request(&commit, "commit", 1 << 20, control)?;
            let text = std::str::from_utf8(&raw)
                .map_err(|_| ToolkitError::invalid("Invalid commit text"))?;
            self.root = Some(
                text.lines()
                    .next()
                    .and_then(|line| line.strip_prefix("tree "))
                    .filter(|hash| valid_digest(hash, 40))
                    .ok_or_else(|| ToolkitError::invalid("Commit omitted tree identity"))?
                    .to_owned(),
            );
        }
        let mut expected = self.root.as_ref().expect("authenticated root").clone();
        let mut ancestor_bytes = 0usize;
        for (i, component) in components.iter().enumerate() {
            control.check()?;
            let leaf = i + 1 == components.len();
            if let Ok(at) = self
                .trees
                .binary_search_by(|entry| entry.object.as_slice().cmp(expected.as_bytes()))
            {
                let tree = &self.trees[at].raw;
                ancestor_bytes += tree.len();
                if ancestor_bytes > MAX_ANCESTOR_BYTES {
                    return Err(budget("Parent tree byte budget exceeded"));
                }
                expected = tree_edge(tree, component.as_bytes(), leaf)?;
            } else {
                let tree =
                    self.process(control)?
                        .request(&expected, "tree", MAX_TREE_BYTES, control)?;
                ancestor_bytes += tree.len();
                if ancestor_bytes > MAX_ANCESTOR_BYTES {
                    return Err(budget("Parent tree byte budget exceeded"));
                }
                let next = tree_edge(&tree, component.as_bytes(), leaf)?;
                self.memoize(&expected, tree);
                expected = next;
            }
        }
        control.check()?;
        self.process(control)?
            .request(&expected, "blob", max, control)
    }
    /// Retained warm originals may have more capacity than their authenticated
    /// length (for example, a cache file that grew during its bounded read).
    /// Charge that excess before handing the original to its consumer. Repeated
    /// reads may conservatively charge twice; memoization is optional, not text.
    pub(crate) fn charge_original_capacity(&mut self, excess: usize) -> Result<()> {
        self.original_room = self
            .original_room
            .checked_sub(excess)
            .ok_or_else(|| budget("Retained original byte budget exceeded"))?;
        self.memo_limit = self.original_room.min(MAX_ANCESTOR_BYTES);
        if self.memo_used() > self.memo_limit {
            self.trees = Vec::new();
            self.tree_bytes = 0;
        }
        Ok(())
    }
    fn memo_used(&self) -> usize {
        self.tree_bytes.saturating_add(
            self.trees
                .capacity()
                .saturating_mul(std::mem::size_of::<TreeMemo>()),
        )
    }
    fn memoize(&mut self, object: &str, tree: Vec<u8>) {
        let record_bytes = std::mem::size_of::<TreeMemo>();
        if tree.capacity().saturating_add(record_bytes) > self.memo_limit {
            return;
        }
        let capacity = self.trees.capacity().max(self.trees.len() + 1);
        let needed = self
            .tree_bytes
            .saturating_add(tree.capacity())
            .saturating_add(capacity.saturating_mul(record_bytes));
        if needed > self.memo_limit {
            self.trees = Vec::new();
            self.tree_bytes = 0;
        }
        // A cache is optional; allocation failure leaves the same authenticated
        // process available to re-read the tree on its next demand.
        if self.trees.try_reserve_exact(1).is_err() {
            return;
        }
        let used = self
            .tree_bytes
            .saturating_add(tree.capacity())
            .saturating_add(self.trees.capacity().saturating_mul(record_bytes));
        if used > self.memo_limit {
            self.trees = Vec::new();
            self.tree_bytes = 0;
            return;
        }
        let object: [u8; 40] = object.as_bytes().try_into().expect("authenticated SHA-1");
        let at = self
            .trees
            .binary_search_by_key(&object, |entry| entry.object)
            .unwrap_err();
        self.tree_bytes += tree.capacity();
        self.trees.insert(at, TreeMemo { object, raw: tree });
    }
    pub(crate) fn finish<T>(mut self, result: Result<T>, control: &OperationControl) -> Result<T> {
        match self.process.take() {
            Some(process) => process.finish(result, control),
            None => crate::persistence::finish_control(result, control.check()),
        }
    }
    #[cfg(test)]
    pub(crate) fn started(&self) -> bool {
        self.process.is_some()
    }
    #[cfg(test)]
    pub(crate) fn process_id(&self) -> Option<u32> {
        self.process.as_ref().map(GitBatchProcess::id)
    }
}
fn tree_edge(tree: &[u8], component: &[u8], leaf: bool) -> Result<String> {
    let mut cursor = 0;
    let mut found = None;
    while cursor < tree.len() {
        let end = tree[cursor..]
            .iter()
            .position(|b| *b == 0)
            .map(|n| cursor + n)
            .ok_or_else(|| ToolkitError::invalid("Truncated tree entry"))?;
        let hash_end = end
            .checked_add(21)
            .filter(|n| *n <= tree.len())
            .ok_or_else(|| ToolkitError::invalid("Truncated tree object identity"))?;
        let entry = &tree[cursor..end];
        let split = entry
            .iter()
            .position(|b| *b == b' ')
            .ok_or_else(|| ToolkitError::invalid("Invalid tree mode"))?;
        if &entry[split + 1..] == component {
            if found.is_some() {
                return Err(ToolkitError::invalid("Duplicate tree path"));
            }
            let mode = &entry[..split];
            let admitted = if leaf {
                mode == b"100644" || mode == b"100755"
            } else {
                mode == b"40000"
            };
            if !admitted {
                return Err(integrity("Pinned path is not a regular source file"));
            }
            found = Some(
                tree[end + 1..hash_end]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
            );
        }
        cursor = hash_end;
    }
    found.ok_or_else(|| ToolkitError::new("codebase.source_not_ready", "Pinned file is absent", 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn frame(kind: &str, raw: &[u8]) -> (String, Vec<u8>) {
        let mut hash = Sha1::new();
        hash.update(format!("{kind} {}\0", raw.len()));
        hash.update(raw);
        let object = format!("{:x}", hash.finalize());
        let mut frame = format!("{object} {kind} {}\n", raw.len()).into_bytes();
        frame.extend_from_slice(raw);
        frame.push(b'\n');
        (object, frame)
    }
    #[test]
    fn demand_frame_matches_existing_authenticated_decoder_without_reading_later_frames() {
        for raw in [b"".as_slice(), b"first\r\n\xff\0", b"hello\n"] {
            let (object, mut bytes) = frame("blob", raw);
            let first_end = bytes.len();
            bytes.extend_from_slice(b"bad later object\n");
            let mut old_at = 0;
            let expected = super::super::take_batch_object(
                &bytes,
                &mut old_at,
                &object,
                "blob",
                MAX_SOURCE_BYTES,
            )
            .unwrap();
            let mut reader = Cursor::new(bytes);
            let actual = read_object_frame(&mut reader, &object, "blob", MAX_SOURCE_BYTES).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(actual.capacity(), raw.len(), "separator is stack scratch");
            assert_eq!(reader.position() as usize, first_end);
            assert_eq!(old_at, first_end);
        }
    }
    #[test]
    fn demand_header_body_hash_type_and_allocation_limits_are_checked() {
        let (empty, _) = frame("blob", b"");
        let header = format!("{empty} blob {}\n\n", "0".repeat(81));
        assert_eq!(header.len(), 129); // A 128-byte header plus the empty body's separator.
        assert!(
            read_object_frame(&mut Cursor::new(header), &empty, "blob", 0)
                .unwrap()
                .is_empty()
        );
        let header = format!("{empty} blob {}\n\n", "0".repeat(82));
        let mut oversized = Cursor::new(header);
        assert_eq!(
            read_object_frame(&mut oversized, &empty, "blob", 0)
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        assert_eq!(oversized.position(), 129);
        let (object, good) = frame("blob", b"abc");
        for bytes in [&good[..0], &good[..good.len() - 2]] {
            assert_eq!(
                read_object_frame(&mut Cursor::new(bytes), &object, "blob", 3)
                    .unwrap_err()
                    .code,
                "codebase.source_integrity"
            );
        }
        assert_eq!(
            read_object_frame(&mut Cursor::new(&good), &object, "tree", 3)
                .unwrap_err()
                .message,
            "Pinned object type or identity differs"
        );
        assert_eq!(
            read_object_frame(&mut Cursor::new(&good), &object, "blob", 2)
                .unwrap_err()
                .code,
            "codebase.source_budget"
        );
        let mut changed = good.clone();
        *changed.last_mut().unwrap() = b'!';
        assert_eq!(
            read_object_frame(&mut Cursor::new(changed), &object, "blob", 3)
                .unwrap_err()
                .message,
            "Invalid Git batch separator"
        );
        let mut changed = good;
        let at = changed.iter().position(|b| *b == b'\n').unwrap() + 1;
        changed[at] = b'X';
        assert_eq!(
            read_object_frame(&mut Cursor::new(changed), &object, "blob", 3)
                .unwrap_err()
                .message,
            "Pinned Git object hash differs"
        );
    }
    fn edge(mode: &str, path: &str) -> Vec<u8> {
        let mut raw = format!("{mode} {path}\0").into_bytes();
        raw.extend_from_slice(&[17; 20]);
        raw
    }
    #[test]
    fn authenticated_tree_edges_keep_regular_file_ancestry_and_duplicate_checks() {
        for mode in ["100644", "100755"] {
            assert_eq!(
                tree_edge(&edge(mode, "File.lua"), b"File.lua", true).unwrap(),
                "11".repeat(20)
            );
            assert_eq!(
                tree_edge(&edge(mode, "Dir"), b"Dir", false)
                    .unwrap_err()
                    .code,
                "codebase.source_integrity"
            );
        }
        assert!(tree_edge(&edge("40000", "Dir"), b"Dir", false).is_ok());
        for mode in ["40000", "120000", "160000"] {
            assert_eq!(
                tree_edge(&edge(mode, "File.lua"), b"File.lua", true)
                    .unwrap_err()
                    .code,
                "codebase.source_integrity"
            );
        }
        assert_eq!(
            tree_edge(&edge("100644", "Other.lua"), b"File.lua", true)
                .unwrap_err()
                .code,
            "codebase.source_not_ready"
        );
        let mut duplicate = edge("100644", "File.lua");
        duplicate.extend(edge("100755", "File.lua"));
        assert_eq!(
            tree_edge(&duplicate, b"File.lua", true)
                .unwrap_err()
                .message,
            "Duplicate tree path"
        );
        duplicate.truncate(10);
        assert!(tree_edge(&duplicate, b"File.lua", true).is_err());
    }
    #[test]
    fn retained_original_capacity_reduces_the_same_memo_allowance() {
        let source_limit = 256usize << 20;
        let reserved_originals = source_limit - 512;
        let mut reader = Originals::new(Path::new("unused"), SourcePin::default(), 512);
        reader.memoize(&"11".repeat(20), vec![7; 200]);
        assert!(reader.memo_used() > 0);
        // A valid warm original's capacity, rather than just its length, owns
        // this additional room. Existing standalone frame/cache scratch is not
        // retained text and keeps its separate pre-existing read bounds.
        reader.charge_original_capacity(300).unwrap();
        assert!(reserved_originals + 300 + reader.memo_used() <= source_limit);
        reader.memoize(&"22".repeat(20), vec![8; 100]);
        assert!(reserved_originals + 300 + reader.memo_used() <= source_limit);
        reader.charge_original_capacity(212).unwrap();
        assert_eq!(reader.memo_used(), 0);
        assert_eq!(reader.memo_limit, 0);
        assert_eq!(
            reader.charge_original_capacity(1).unwrap_err().code,
            "codebase.source_budget"
        );
    }
    #[test]
    fn tree_memo_uses_shared_room_and_actual_payload_and_record_capacities() {
        for room in [0, 64, 512, 1024] {
            let mut reader = Originals::new(Path::new("unused"), SourcePin::default(), room);
            for i in 0..32 {
                reader.memoize(&format!("{i:040x}"), vec![i as u8; i]);
                let actual = reader
                    .trees
                    .iter()
                    .map(|tree| tree.raw.capacity())
                    .sum::<usize>()
                    + reader.trees.capacity() * std::mem::size_of::<TreeMemo>();
                assert!(actual <= room, "{actual} > {room}");
                assert_eq!(
                    reader.tree_bytes,
                    reader
                        .trees
                        .iter()
                        .map(|tree| tree.raw.capacity())
                        .sum::<usize>()
                );
                assert!(
                    reader
                        .trees
                        .windows(2)
                        .all(|pair| pair[0].object < pair[1].object)
                );
            }
            let before = reader.tree_bytes;
            let mut excessive_capacity = Vec::with_capacity(2048);
            excessive_capacity.push(1);
            reader.memoize(&"f".repeat(40), excessive_capacity);
            assert_eq!(reader.tree_bytes, before);
            assert!(!reader.started());
        }
    }
}
