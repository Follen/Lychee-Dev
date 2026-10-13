//! Selected content extents retain a read-only archive handle, not a whole
//! encoded body. The caller admits actual encoded/decoded lengths separately
//! from the shared index/metadata limits.
use super::*;

pub(crate) struct Selected<'a> {
    file: fs::File,
    _directory: storage::Directory,
    span: Span,
    position: u64,
    budget: &'a Budget,
}
impl Selected<'_> {
    pub(crate) fn rewind(&mut self) -> Result<()> {
        self.budget.check()?;
        self.file.seek(SeekFrom::Start(self.span.offset + 30))?;
        self.position = 0;
        Ok(())
    }
}
impl Read for Selected<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.budget.check().map_err(std::io::Error::other)?;
        let remaining = self.span.bytes - 30 - self.position;
        let requested = out.len().min(65536).min(remaining as usize);
        if requested == 0 {
            return Ok(0);
        }
        self.budget.charge(Cost::work(1)).map_err(std::io::Error::other)?;
        let n = self.file.read(&mut out[..requested])?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "CASC extent ended early"));
        }
        self.position += n as u64;
        self.budget.check().map_err(std::io::Error::other)?;
        Ok(n)
    }
}

pub(crate) fn open_selected<'a>(
    root: &Path,
    encoding_key: &str,
    encoded_bytes: usize,
    limits: Limits,
    budget: &'a Budget,
) -> Result<Selected<'a>> {
    budget.check()?;
    let expected = key(encoding_key)?;
    if encoded_bytes < 8 {
        return Err(invalid());
    }
    let directory_path = root.join("Data/data");
    let mut collision = false;
    for (bucket, (_, name)) in select_indices(root, limits, budget.token())? {
        let raw = bounded_read_budgeted(
            &directory_path.join(&name), root, limits.index_bytes, limits,
            budget.token(), Some(budget),
        )?;
        let index = Index::open(&raw, bucket, limits, budget.token())?;
        for span in index.find(&expected, limits, budget.token())? {
            if span.bytes != encoded_bytes as u64 + 30 {
                collision = true;
                continue;
            }
            let directory = storage::Directory::open(&directory_path)?;
            let mut file = directory.open_file(std::ffi::OsStr::new(&span.filename()))?;
            let meta = file.metadata()?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 { return Err(invalid()); }
            }
            if !meta.is_file() || span.offset > meta.len() || span.bytes > meta.len() - span.offset {
                return Err(invalid());
            }
            file.seek(SeekFrom::Start(span.offset))?;
            let mut prefix = [0; 38];
            file.read_exact(&mut prefix)?;
            if prefix[..30].iter().any(|b| *b != 0) && (0..9).any(|i| prefix[15-i] != expected[i]) {
                collision = true;
                continue;
            }
            if &prefix[30..34] != b"BLTE" {
                return Err(invalid());
            }
            let header = u32::from_be_bytes(prefix[34..38].try_into().unwrap()) as usize;
            if header != 0 && (header < 12 || header > encoded_bytes || header > 12 + 65536 * 24) {
                return Err(invalid());
            }
            // Framed EKeys cover framing; headerless EKeys cover all encoded
            // bytes. Both use the same held deny-write/delete archive handle.
            let hashed_bytes = if header == 0 { encoded_bytes } else { header };
            let _scratch = budget.reserve_retained(65536)?;
            let mut selected = Selected { file, _directory: directory, span, position: 0, budget };
            selected.rewind()?;
            let mut hash = Md5::new();
            let mut buffer = [0; 65536];
            let mut remaining = hashed_bytes;
            while remaining != 0 {
                let requested = remaining.min(buffer.len());
                let n = selected.read(&mut buffer[..requested]).map_err(crate::blte::stream_io_error)?;
                if n == 0 {
                    return Err(invalid());
                }
                hash.update(&buffer[..n]);
                remaining -= n;
            }
            if hash.finalize().as_slice() != expected {
                collision = true;
                continue;
            }
            selected.rewind()?;
            return Ok(selected);
        }
    }
    Err(if collision { integrity() } else {
        fault("records.local_object_unavailable", 3, "Full encoding key not available in newest local CASC indices")
    })
}
