use super::*;
use crate::evidence::bounded::lifecycle::PathControl;
use lycheedev_windows::filesystem::destination::{StagedFile, StagedPublication};
use std::{
    collections::BTreeSet,
    io::{self, Seek, SeekFrom},
};
fn fault(kind: &str) -> ToolkitError {
    ToolkitError::new("command.failed", format!("evidence.bundle_{kind}"), 5)
}
fn io_error(error: io::Error) -> ToolkitError {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ToolkitError>())
        .cloned()
        .unwrap_or_else(|| ToolkitError::from(error))
}
fn zip_error(error: zip::result::ZipError) -> ToolkitError {
    match error {
        zip::result::ZipError::Io(error) => io_error(error),
        error => ToolkitError::new("command.failed", error.to_string(), 5),
    }
}
// The archive borrows the stage. Even a consumed ZipWriter::finish error cannot
// take away the caller's authority to close/discard the original stage owner.
struct Output<'a, 'b> {
    stage: &'a mut StagedFile<'b>,
    control: &'a OperationControl,
    failed: Option<ToolkitError>,
}
impl Output<'_, '_> {
    fn check(&mut self) -> io::Result<()> {
        if let Some(error) = &self.failed {
            return Err(io::Error::other(error.clone()));
        }
        self.control.check().map_err(|error| {
            self.failed = Some(error.clone());
            io::Error::other(error)
        })
    }
    fn observe<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        result.map_err(|error| {
            self.failed = Some(io_error(error));
            io::Error::other(self.failed.as_ref().unwrap().clone())
        })
    }
}
impl Write for Output<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.check()?;
        let n = bytes.len().min(65536);
        let result = self.stage.write_all(&bytes[..n]);
        self.observe(result)?;
        // n is persisted privately before any post-I/O stop; no public output
        // or Data byte accounting is claimed by this archive writer.
        self.check()?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()
    }
}
impl Seek for Output<'_, '_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        let result = self.stage.seek_private(position);
        let offset = self.observe(result)?;
        self.check()?;
        Ok(offset)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema: &'static str,
    output: String,
    captures: Vec<GoCaptureDecode>,
    bytes: i64,
    complete: bool,
}
fn write_entry<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    name: &str,
    bytes: &[u8],
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    zip.start_file(
        name,
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .map_err(zip_error)?;
    for block in bytes.chunks(65536) {
        control.check()?;
        zip.write_all(block).map_err(io_error)?;
        control.check()?;
    }
    Ok(())
}
fn build(
    stage: &mut StagedFile<'_>,
    inv: &Invocation,
    ids: &[&str],
    output: String,
) -> Result<Value> {
    let mut adapter = Output {
        stage,
        control: &inv.control,
        failed: None,
    };
    let mut zip = zip::ZipWriter::new(&mut adapter);
    let mut manifest = Manifest {
        schema: "lycheedev.evidence-bundle.v1",
        output,
        captures: Vec::with_capacity(ids.len()),
        bytes: 0,
        complete: true,
    };
    let result = (|| {
        let metadata = read_commands::read_metadata(&inv.home, &inv.control)?;
        let captures = (|| {
            let mut used = BTreeSet::new();
            for id in ids {
                inv.control.check()?;
                let capture = read_commands::inspect(&metadata, id, &inv.control)?;
                let reference = serde_json::to_value(&capture)?;
                let digest = reference["blob"]["sha256"].as_str().unwrap_or("");
                let signed = reference["blob"]["bytes"]
                    .as_i64()
                    .ok_or_else(|| ToolkitError::invalid("invalid historical blob length"))?;
                if signed > 32 << 20
                    || !used.contains(digest)
                        && i128::from(manifest.bytes) + i128::from(signed) > 128 << 20
                {
                    return Err(fault("limit"));
                }
                // Go revalidates/reads duplicate objects too; only ZIP entries
                // and manifest.Bytes are deduplicated, never input validation.
                if signed < 0 {
                    return Err(ToolkitError::new(
                        "vault.blob_integrity",
                        "vault.blob_integrity",
                        4,
                    ));
                }
                let raw = read_blob_bounded(&inv.home, &reference["blob"], 32 << 20, &inv.control)?;
                if !used.contains(digest) {
                    write_entry(&mut zip, &format!("objects/{digest}"), &raw, &inv.control)?;
                    used.insert(digest.to_owned());
                    manifest.bytes += raw.len() as i64;
                }
                inv.control.check()?;
                let encoded = storage::identity_json(&capture)?;
                inv.control.check()?;
                write_entry(
                    &mut zip,
                    &format!("captures/{id}.json"),
                    &encoded,
                    &inv.control,
                )?;
                manifest.captures.push(capture);
            }
            Ok(())
        })();
        let captures = finish_historical(captures, metadata.close());
        crate::selection::selection_finish(captures, inv.control.check())?;
        let encoded = storage::identity_json(&manifest)?;
        inv.control.check()?;
        write_entry(&mut zip, "manifest.json", &encoded, &inv.control)?;
        Ok(())
    })();
    // Finalize even when body work failed, retaining its first cause and any
    // distinct finalization failure. A sticky failed adapter prevents retries
    // of an already failed underlying I/O from ZipWriter's fallback Drop.
    let finish = zip.finish().map(|_| ()).map_err(zip_error);
    let result = match (&result, &finish) {
        (Err(a), Err(b))
            if a.code == b.code
                && a.exit_code == b.exit_code
                && a.message == b.message
                && a.details == b.details =>
        {
            result
        }
        _ => finish_historical(result, finish),
    };
    result?;
    inv.control.check()?;
    let result = serde_json::to_value(manifest).map_err(Into::into);
    crate::selection::selection_finish(result, inv.control.check())
}
fn close_directory<T>(result: Result<T>, directory: storage::Directory) -> Result<T> {
    let mut result = result;
    if let Err(errors) = directory.close() {
        for error in errors {
            result = finish_historical(result, Err(error.into()));
        }
    }
    result
}
pub(super) fn run(inv: &Invocation) -> Result<Value> {
    let ids: Vec<_> = inv.required("--ids")?.split(',').collect();
    if ids.is_empty() || ids.len() > 100 {
        return Err(fault("selection"));
    }
    let mut seen = BTreeSet::new();
    for id in &ids {
        if !id.starts_with("CAP-") || id.len() != 68 || !seen.insert(*id) {
            return Err(fault("selection"));
        }
    }
    let output = inv.required("--output")?;
    if output.is_empty() {
        return Err(fault("output"));
    }
    inv.control.check()?;
    let path = Path::new(output);
    let absolute = std::path::absolute(if path.is_absolute() {
        path.to_owned()
    } else {
        inv.cwd.join(path)
    })?;
    inv.control.check()?;
    let canonical_parent = fs::canonicalize(absolute.parent().ok_or_else(|| fault("output"))?)
        .map_err(|error| {
            ToolkitError::new(
                "command.failed",
                format!("evidence.bundle_output: {error}"),
                5,
            )
        })?;
    inv.control.check()?;
    let name = absolute.file_name().ok_or_else(|| fault("output"))?;
    let target = canonical_parent.join(name);
    let workspace = fs::canonicalize(&inv.home)?;
    inv.control.check()?;
    if target.starts_with(&workspace) {
        return Err(fault("output"));
    }
    let mut supplemental = || Ok(());
    let mut files = PathControl {
        control: Some(&inv.control),
        supplemental: &mut supplemental,
    };
    let parent = storage::Directory::open_controlled(&canonical_parent, &mut files)?;
    let mut publication = StagedPublication::NotPublished;
    let mut stage_name = String::new();
    let result = (|| {
        if parent
            .optional_metadata_controlled(name, &mut files)?
            .is_some()
        {
            return Err(fault("output"));
        }
        let mut stage = StagedFile::create(&parent, &mut files)?;
        let result = (|| {
            inv.control.check()?;
            let manifest = build(&mut stage, inv, &ids, crate::client::display_path(&target))?;
            inv.control.check()?;
            stage.seal()?;
            inv.control.check()?;
            match stage.publish_name(name, &mut files)? {
                StagedPublication::Published => Ok(manifest),
                StagedPublication::NotPublished => Err(fault("output")),
                StagedPublication::Unknown => Err(ToolkitError::new(
                    "command.failed",
                    "bundle publication is uncertain",
                    5,
                )),
            }
        })();
        publication = stage.publication();
        stage_name = stage.name().to_owned();
        let mut result = result;
        for error in stage.close() {
            result = finish_historical(result, Err(error.into()));
        }
        result
    })();
    let result = close_directory(result, parent);
    let result = crate::selection::selection_finish(result, inv.control.check());
    annotate_publication(result, publication, &target, &stage_name)
}
pub(super) fn annotate_publication<T>(
    result: Result<T>,
    publication: StagedPublication,
    target: &Path,
    stage_name: &str,
) -> Result<T> {
    result.map_err(|mut error| {
        if publication!=StagedPublication::NotPublished {
            let primary=error.clone();
            let state=if publication==StagedPublication::Published{"published"}else{"unknown"};
            error.message=format!("{}\nbundle output {state}: {}",error.message,crate::client::display_path(target));
            error.details=json!({"schema":"lycheedev.bundle-publication-error.v1","primary":primary,"publication":{"state":state,"output":crate::client::display_path(target),"stageName":stage_name}});
        }
        error
    })
}
