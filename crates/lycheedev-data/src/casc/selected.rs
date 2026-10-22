//! A selected extent keeps the authenticated archive and directory alive.
//! Every read/rewind observes the original invocation and independent budget.
use super::*;

pub(crate) struct Selected<'a> {
    file: fs::File,
    directory: storage::Directory,
    span: Span,
    position: u64,
    scope: Scope<'a>,
}
impl Selected<'_> {
    pub(crate) fn rewind(&mut self) -> Result<()> {
        self.scope.check()?;
        self.file.seek(SeekFrom::Start(self.span.offset + 30))?;
        self.position = 0;
        self.scope.check()
    }
    pub(crate) fn close_after<T>(self, result: Result<T>) -> Result<T> {
        let mut cleanup = Vec::new();
        if let Err(error) = storage::close_file(self.file) {
            cleanup.push(error.into());
        }
        if let Err(errors) = self.directory.close() {
            cleanup.extend(errors.into_iter().map(ToolkitError::from));
        }
        self.scope.finish(result, cleanup)
    }
}
impl Read for Selected<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.scope.check().map_err(std::io::Error::other)?;
        let remaining = self.span.bytes - 30 - self.position;
        let requested = out.len().min(65536).min(remaining as usize);
        if requested == 0 {
            return Ok(0);
        }
        self.scope
            .budget
            .expect("selected reader budget")
            .charge(Cost::work(1))
            .map_err(std::io::Error::other)?;
        let n = self.file.read(&mut out[..requested])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "CASC extent ended early",
            ));
        }
        // Bytes already consumed are not undone when the post-read check stops.
        self.position += n as u64;
        self.scope.check().map_err(std::io::Error::other)?;
        Ok(n)
    }
}

pub(crate) fn open_selected<'a>(
    root: &Path,
    encoding_key: &str,
    encoded_bytes: usize,
    limits: Limits,
    budget: &'a Budget,
    control: &'a OperationControl,
) -> Result<Selected<'a>> {
    open_selected_source(
        root,
        encoding_key,
        encoded_bytes,
        limits,
        budget,
        control,
        None,
    )
}
pub(crate) fn open_selected_from_source<'a>(
    source: &QuerySource<'_>,
    encoding_key: &str,
    encoded_bytes: usize,
    limits: Limits,
    budget: &'a Budget,
    control: &'a OperationControl,
) -> Result<Selected<'a>> {
    source.check_consumer(budget, control)?;
    open_selected_source(
        source.root(),
        encoding_key,
        encoded_bytes,
        limits,
        budget,
        control,
        Some(source),
    )
}
fn open_selected_source<'a>(
    root: &Path,
    encoding_key: &str,
    encoded_bytes: usize,
    limits: Limits,
    budget: &'a Budget,
    control: &'a OperationControl,
    source: Option<&QuerySource<'_>>,
) -> Result<Selected<'a>> {
    let mut scope = Scope {
        limits,
        cancel: None,
        control: Some(control),
        budget: Some(budget),
    };
    scope.check()?;
    let expected = key(encoding_key)?;
    if encoded_bytes < 8 {
        return Err(invalid());
    }
    let directory_path = root.join("Data/data");
    let mut collision = false;
    let selected = if let Some(source) = source {
        source.indices(&expected)?
    } else {
        select_indices_with_check(root, limits, None, || scope.check())?
            .into_iter()
            .map(|(bucket, (_, name))| (bucket, name))
            .collect()
    };
    for (bucket, name) in selected {
        let spans =
            if let Some(source) = source {
                source.find(bucket, &name, &expected)?
            } else {
                let raw = bounded_read_with_check(
                    &directory_path.join(&name),
                    root,
                    limits.index_bytes,
                    limits,
                    None,
                    Some(budget),
                    || scope.check(),
                )?;
                Index::open_with_check(&raw, bucket, limits, None, || scope.check())?
                    .find_with_check(&expected, limits, None, || scope.check())?
            };
        for span in spans {
            if span.bytes != encoded_bytes as u64 + 30 {
                collision = true;
                continue;
            }
            let directory = storage::Directory::open_controlled(&directory_path, &mut scope)?;
            let opened = if let Some(source) = source {
                source.open_file(&span.filename())
            } else {
                directory
                    .open_shared_file_controlled(std::ffi::OsStr::new(&span.filename()), &mut scope)
                    .map_err(|mut error| {
                        error.message = format!(
                            "Open local CASC file '{}': {}",
                            directory.path().join(span.filename()).display(),
                            error.message
                        );
                        error
                    })
            };
            let file = match opened {
                Ok(file) => file,
                Err(error) => {
                    return scope.finish(
                        Err(error),
                        directory
                            .close()
                            .err()
                            .into_iter()
                            .flatten()
                            .map(Into::into)
                            .collect(),
                    );
                }
            };
            let mut selected = Selected {
                file,
                directory,
                span,
                position: 0,
                scope,
            };
            let result = (|| {
                scope.check()?;
                let meta = selected.file.metadata()?;
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(invalid());
                    }
                }
                if !meta.is_file()
                    || span.offset > meta.len()
                    || span.bytes > meta.len() - span.offset
                {
                    return Err(invalid());
                }
                scope.check()?;
                selected.file.seek(SeekFrom::Start(span.offset))?;
                let mut prefix = [0; 38];
                scope.check()?;
                selected.file.read_exact(&mut prefix)?;
                scope.check()?;
                if prefix[..30].iter().any(|b| *b != 0)
                    && (0..9).any(|i| prefix[15 - i] != expected[i])
                {
                    return Ok(false);
                }
                if &prefix[30..34] != b"BLTE" {
                    return Err(invalid());
                }
                let header = u32::from_be_bytes(prefix[34..38].try_into().unwrap()) as usize;
                if header != 0
                    && (header < 12 || header > encoded_bytes || header > 12 + 65536 * 24)
                {
                    return Err(invalid());
                }
                let hashed_bytes = if header == 0 { encoded_bytes } else { header };
                let _scratch = budget.reserve_retained(65536)?;
                selected.rewind()?;
                let mut hash = Md5::new();
                let mut buffer = [0; 65536];
                let mut remaining = hashed_bytes;
                while remaining != 0 {
                    let requested = remaining.min(buffer.len());
                    let n = selected
                        .read(&mut buffer[..requested])
                        .map_err(crate::blte::stream_io_error)?;
                    if n == 0 {
                        return Err(invalid());
                    }
                    hash.update(&buffer[..n]);
                    remaining -= n;
                }
                if hash.finalize().as_slice() != expected {
                    return Ok(false);
                }
                selected.rewind()?;
                selected.directory.verify_controlled(&mut scope)?;
                scope.check()?;
                Ok(true)
            })();
            match result {
                Ok(true) => return Ok(selected),
                result => {
                    selected.close_after(result)?;
                    collision = true;
                }
            }
        }
    }
    Err(if collision {
        integrity()
    } else {
        fault(
            "records.local_object_unavailable",
            3,
            "Full encoding key not available in newest local CASC indices",
        )
    })
}
