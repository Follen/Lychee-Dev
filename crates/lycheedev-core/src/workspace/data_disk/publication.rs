//! The first real consumer sink. Reuses PrivateSpool + immutable no-replace
//! StagedFile publication; no custom rename, writable File escape or eviction.
use super::*;
use lycheedev_windows::filesystem::destination::{PrivateSpool, StagedFile, StagedPublication};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    io::{Read, Write},
};

#[cfg(test)]
thread_local! {
    // Witness actual digest progress, not an ordinal shared with later I/O.
    static MANIFEST_HASHED_BYTES: Cell<u64> = const { Cell::new(0) };
    // Witness successful writes to the original final stage, before visibility.
    static OBJECT_COPIED_BYTES: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DerivedObject {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Copy)]
enum Kind {
    Objects,
    Generations,
    Staging,
}
impl Kind {
    fn get(self, peak: &mut AdditionalPeak) -> &mut u64 {
        match self {
            Self::Objects => &mut peak.objects,
            Self::Generations => &mut peak.generations,
            Self::Staging => &mut peak.staging,
        }
    }
}
impl DerivedReservation<'_> {
    pub(super) fn prepare_publication_receipt(&self) -> Result<()> {
        self.control.check()?;
        let mut receipts = self.publication_receipts.borrow_mut();
        if receipts.len() >= 1024 {
            return Err(ToolkitError::new(
                "vault.data_cache_publication_limit",
                "Data owner exceeds 1024 publication attempts",
                3,
            ));
        }
        receipts.try_reserve(1).map_err(|_| {
            ToolkitError::new(
                "vault.data_cache_publication_limit",
                "Cannot allocate Data publication receipt",
                3,
            )
        })
    }
    pub(super) fn record_publication(
        &self,
        state: StagedPublication,
        kind: &str,
        path: &Path,
        sha256: &str,
        bytes: u64,
    ) {
        if state != StagedPublication::NotPublished {
            self.publication_receipts
                .borrow_mut()
                .push(serde_json::json!({
                    "kind":kind,"path":path,"sha256":sha256,"bytes":bytes,
                    "state":format!("{state:?}"),
                }));
        }
    }
    fn charge(&self, kind: Kind, bytes: u64) -> Result<()> {
        self.control.check()?;
        let mut current = self.charged.get();
        let mut limits = self.report.additional_peak;
        let next = sum([*kind.get(&mut current), bytes])?;
        if next > *kind.get(&mut limits) {
            return Err(ToolkitError::new(
                "vault.data_cache_reservation_exceeded", "Data write exceeds its reserved category; no additional bytes written", 3,
            ).with_details(serde_json::json!({"schema":"lycheedev.data-derived-admission.v1","reservation":self.report,"charged":self.charged.get(),"requestedBytes":bytes})));
        }
        *kind.get(&mut current) = next;
        self.charged.set(current);
        Ok(())
    }
    fn refund(&self, kind: Kind, bytes: u64) {
        let mut current = self.charged.get();
        let amount = kind.get(&mut current);
        *amount = amount
            .checked_sub(bytes)
            .expect("refund only this owner's admitted bytes");
        self.charged.set(current);
    }
    fn directory(&self, path: &Path) -> &Directory {
        self.directories
            .iter()
            .find(|directory| directory.path() == path)
            .expect("reservation retains every fixed publication directory")
    }
    /// Stream to a same-command private spool. The declared bound must fit both
    /// staging and object peak reservations. No filesystem allocation occurs
    /// until after the reservation has already admitted the simultaneous peak.
    pub fn begin_object(&self, max_bytes: u64) -> Result<DerivedObjectWriter<'_>> {
        self.control.check()?;
        if max_bytes > self.report.additional_peak.staging
            || max_bytes > self.report.additional_peak.objects
        {
            return Err(ToolkitError::new(
                "vault.data_cache_reservation_exceeded",
                "Object bound requires both staging and final-object reservations",
                3,
            ));
        }
        let stage = PrivateSpool::create(
            self.directory(&self.paths.staging),
            &mut Controlled(&self.control),
        )?;
        let writer = DerivedObjectWriter {
            owner: self,
            stage: Some(stage),
            maximum: max_bytes,
            bytes: 0,
            digest: Sha256::new(),
            failure: None,
        };
        match self.control.check() {
            Ok(()) => Ok(writer),
            Err(error) => writer.abort(Err(error)),
        }
    }
    /// Immutable manifest publication. The Data layer owns generation semantics
    /// and must verify its source and publish referenced objects first. A key
    /// collision with different bytes is an error, never replacement or eviction.
    pub fn publish_generation(&self, key: &str, bytes: &[u8]) -> Result<()> {
        self.control.check()?;
        if !valid_digest(key, 64) || key.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(ToolkitError::invalid(
                "Data generation requires lowercase SHA-256 key",
            ));
        }
        let parent = self.directory(&self.paths.generations);
        let name = format!("{key}.json");
        let mut digest = Sha256::new();
        for chunk in bytes.chunks(65536) {
            self.control.check()?;
            digest.update(chunk);
            #[cfg(test)]
            MANIFEST_HASHED_BYTES
                .with(|progress| progress.set(progress.get() + chunk.len() as u64));
        }
        self.control.check()?;
        let digest = hex::encode(digest.finalize());
        if verify_existing(
            parent,
            OsStr::new(&name),
            bytes.len() as u64,
            &digest,
            &self.control,
        )? {
            return Ok(());
        }
        self.prepare_publication_receipt()?;
        self.charge(Kind::Generations, bytes.len() as u64)?;
        let mut stage = match StagedFile::create(parent, &mut Controlled(&self.control)) {
            Ok(stage) => stage,
            Err(error) => {
                self.refund(Kind::Generations, bytes.len() as u64);
                return Err(error);
            }
        };
        let result = (|| {
            for chunk in bytes.chunks(65536) {
                self.control.check()?;
                stage.write_all(chunk)?;
            }
            self.control.check()?;
            stage.seal()?; // Generation visibility keeps the durable manifest contract.
            self.control.check()?;
            let state = stage.publish_name(OsStr::new(&name), &mut Controlled(&self.control))?;
            if state == StagedPublication::NotPublished
                && !verify_existing(
                    parent,
                    OsStr::new(&name),
                    bytes.len() as u64,
                    &digest,
                    &self.control,
                )?
            {
                return Err(ToolkitError::invalid(
                    "Data generation publication winner disappeared",
                ));
            }
            Ok(())
        })();
        let state = stage.publication();
        self.record_publication(
            state,
            "generation",
            &parent.path().join(&name),
            &digest,
            bytes.len() as u64,
        );
        let temporary = stage.name().to_owned();
        let cleanup: Vec<_> = stage.close().into_iter().map(ToolkitError::io).collect();
        if state == StagedPublication::NotPublished && cleanup.is_empty() {
            self.refund(Kind::Generations, bytes.len() as u64);
        }
        let cleanup_failed = !cleanup.is_empty();
        publication_result(
            join(result, cleanup, &self.control),
            state,
            parent.path().join(name),
            temporary,
            cleanup_failed,
        )
    }
}

/// Bounded append-only Write adapter for Arrow IPC. Call abort on codec errors;
/// finish the codec before commit. No seek/file escape can bypass byte admission.
/// error() preserves the original typed budget/cancellation error through codecs
/// that erase an io::Error's source. A failed writer cannot be committed/reused.
pub struct DerivedObjectWriter<'a> {
    owner: &'a DerivedReservation<'a>,
    stage: Option<PrivateSpool<'a>>,
    maximum: u64,
    bytes: u64,
    digest: Sha256,
    failure: Option<ToolkitError>,
}
impl<'a> DerivedObjectWriter<'a> {
    pub fn len(&self) -> u64 {
        self.bytes
    }
    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }
    pub fn error(&self) -> Option<&ToolkitError> {
        self.failure.as_ref()
    }
    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = (|| {
            self.owner.control.check()?;
            let next = sum([self.bytes, bytes.len() as u64])?;
            if next > self.maximum {
                return Err(ToolkitError::new(
                    "vault.data_cache_reservation_exceeded",
                    "Data object exceeds declared maximum; no additional bytes written",
                    3,
                ));
            }
            // Charge before the first write. On a partial failing syscall the
            // full attempted slice remains charged until checked discard.
            self.owner.charge(Kind::Staging, bytes.len() as u64)?;
            self.bytes = next;
            for chunk in bytes.chunks(65536) {
                self.owner.control.check()?;
                self.stage
                    .as_mut()
                    .expect("owned private spool")
                    .write_all(chunk)?;
                self.digest.update(chunk);
            }
            self.owner.control.check()
        })();
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }
    pub fn commit(mut self) -> Result<DerivedObject> {
        let result = (|| {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.owner.control.check()?;
            self.stage
                .as_mut()
                .expect("owned private spool")
                .finish_writes()?;
            self.owner.control.check()?;
            let object = DerivedObject {
                sha256: hex::encode(self.digest.clone().finalize()),
                bytes: self.bytes,
            };
            let path = self.owner.paths.object(&object.sha256)?;
            let parent = prepare_root(path.parent().expect("content shard"), &self.owner.control)?;
            let result = self.publish_object(&parent, &object);
            join(result, close_directory(parent), &self.owner.control)
        })();
        self.abort(result)
    }
    fn publish_object(
        &mut self,
        parent: &Directory,
        object: &DerivedObject,
    ) -> Result<DerivedObject> {
        let name = OsStr::new(&object.sha256[2..]);
        if verify_existing(
            parent,
            name,
            object.bytes,
            &object.sha256,
            &self.owner.control,
        )? {
            self.owner.touch_object(&object.sha256)?;
            return Ok(object.clone());
        }
        self.owner.prepare_publication_receipt()?;
        self.owner.charge(Kind::Objects, object.bytes)?;
        let mut final_stage = match StagedFile::create(parent, &mut Controlled(&self.owner.control))
        {
            Ok(stage) => stage,
            Err(error) => {
                self.owner.refund(Kind::Objects, object.bytes);
                return Err(error);
            }
        };
        let result = (|| {
            let mut remaining = object.bytes;
            let mut buffer = [0u8; 65536];
            let mut digest = Sha256::new();
            while remaining != 0 {
                self.owner.control.check()?;
                let count = (remaining.min(buffer.len() as u64)) as usize;
                self.stage
                    .as_mut()
                    .expect("owned private spool")
                    .read_exact(&mut buffer[..count])?;
                digest.update(&buffer[..count]);
                final_stage.write_all(&buffer[..count])?;
                remaining -= count as u64;
                #[cfg(test)]
                OBJECT_COPIED_BYTES.with(|progress| progress.set(object.bytes - remaining));
            }
            self.owner.control.check()?;
            let mut tail = [0];
            if self
                .stage
                .as_mut()
                .expect("owned private spool")
                .read(&mut tail)?
                != 0
                || hex::encode(digest.finalize()) != object.sha256
            {
                return Err(ToolkitError::new(
                    "vault.data_cache_integrity",
                    "Private Data spool does not match its measured content",
                    4,
                ));
            }
            final_stage.seal_rebuildable_cache()?;
            self.owner.control.check()?;
            let state = final_stage.publish_name(name, &mut Controlled(&self.owner.control))?;
            if state == StagedPublication::NotPublished
                && !verify_existing(
                    parent,
                    name,
                    object.bytes,
                    &object.sha256,
                    &self.owner.control,
                )?
            {
                return Err(ToolkitError::invalid(
                    "Data object publication winner disappeared",
                ));
            }
            Ok(object.clone())
        })();
        let state = final_stage.publication();
        self.owner.record_publication(
            state,
            "object",
            &parent.path().join(name),
            &object.sha256,
            object.bytes,
        );
        let temporary = final_stage.name().to_owned();
        let cleanup: Vec<_> = final_stage
            .close()
            .into_iter()
            .map(ToolkitError::io)
            .collect();
        if state == StagedPublication::NotPublished && cleanup.is_empty() {
            self.owner.refund(Kind::Objects, object.bytes);
        }
        let cleanup_failed = !cleanup.is_empty();
        publication_result(
            join(result, cleanup, &self.owner.control),
            state,
            parent.path().join(name),
            temporary,
            cleanup_failed,
        ).and_then(|object| {
            self.owner.touch_object(&object.sha256)?;
            Ok(object)
        })
    }
    pub fn abort<T>(self, result: Result<T>) -> Result<T> {
        self.abort_with(result, |stage| stage.close())
    }
    fn abort_with<T>(
        mut self,
        result: Result<T>,
        close: impl FnOnce(PrivateSpool<'a>) -> Vec<std::io::Error>,
    ) -> Result<T> {
        let result = match (result, self.failure.take()) {
            (Ok(_), Some(error)) => Err(error),
            (result, _) => result,
        };
        let stage = self.stage.take().expect("owned private spool");
        let temporary = self.owner.paths.staging.join(stage.name());
        let cleanup: Vec<_> = close(stage).into_iter().map(ToolkitError::io).collect();
        let cleanup_failed = !cleanup.is_empty();
        if !cleanup_failed {
            self.owner.refund(Kind::Staging, self.bytes);
        }
        let result = join(result, cleanup, &self.owner.control).map_err(|mut error| {
            if cleanup_failed {
                error.details = serde_json::json!({
                    "schema":"lycheedev.data-derived-staging-cleanup.v1",
                    "temporary":temporary,"cleanupComplete":false,
                    "reservedBytesRetained":self.bytes,"cause":error.clone(),
                });
            }
            error
        });
        self.owner.with_publications(result)
    }
}
impl Write for DerivedObjectWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.write_bytes(bytes)
            .map(|()| bytes.len())
            .map_err(std::io::Error::other)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(error) = &self.failure {
            return Err(std::io::Error::other(error.clone()));
        }
        self.owner.control.check().map_err(|error| {
            self.failure = Some(error.clone());
            std::io::Error::other(error)
        })
    }
}

pub(super) fn verify_existing(
    parent: &Directory,
    name: &OsStr,
    expected_bytes: u64,
    expected_digest: &str,
    control: &Checks<'_>,
) -> Result<bool> {
    let Some(mut file) = parent.open_optional_file_controlled(name, &mut Controlled(control))?
    else {
        control.check()?;
        return Ok(false);
    };
    let result = (|| {
        let info = file.metadata()?;
        if !info.is_file() || info.len() != expected_bytes {
            return Err(ToolkitError::new(
                "vault.data_cache_integrity",
                "Existing Data object/manifest length or kind differs",
                4,
            ));
        }
        let mut buffer = [0; 65536];
        let mut remaining = expected_bytes;
        let mut digest = Sha256::new();
        while remaining != 0 {
            control.check()?;
            let count = remaining.min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..count])?;
            digest.update(&buffer[..count]);
            remaining -= count as u64;
        }
        control.check()?;
        let mut tail = [0];
        if file.read(&mut tail)? != 0 || hex::encode(digest.finalize()) != expected_digest {
            return Err(ToolkitError::new(
                "vault.data_cache_integrity",
                "Existing Data object/manifest content differs",
                4,
            ));
        }
        control.check()?;
        Ok(true)
    })();
    finish_file(file, result, control)
}
pub(super) fn publication_result<T>(
    result: Result<T>,
    state: StagedPublication,
    destination: PathBuf,
    temporary: String,
    cleanup_failed: bool,
) -> Result<T> {
    result.map_err(|mut error| {
        if state != StagedPublication::NotPublished || cleanup_failed {
            let temporary_path = destination.parent().map(|parent| parent.join(&temporary));
            error.details = serde_json::json!({"schema":"lycheedev.data-derived-publication.v1","path":destination,"temporary":temporary_path,"state":format!("{state:?}"),"cleanupComplete":!cleanup_failed,"cause":error.clone()});
        }
        error
    })
}

#[cfg(test)]
mod tests;
