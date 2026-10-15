//! Read-only durable adapter. Each SQLite statement returns one atomic document;
//! fixed Go ReadWorkspace does not impose a cross-document SQL transaction.
use super::*;
pub(super) use lycheedev_core::metadata::RawDocument;
use lycheedev_core::{
    evidence::{self, CaptureRef},
    selection::PinnedSet,
};
pub(super) struct Captured {
    pub reference: CaptureRef,
    pub bytes: Vec<u8>,
}
pub(super) trait ReadStore {
    fn document(&self, key: &str, control: &OperationControl) -> Result<RawDocument>;
    fn capture(&self, id: &str, maximum: usize, control: &OperationControl) -> Result<Captured>;
    fn pin(&self, id: &str, control: &OperationControl) -> Result<PinnedSet>;
}
pub(super) struct DiskRead<'a>(pub &'a Path);
impl ReadStore for DiskRead<'_> {
    fn document(&self, key: &str, control: &OperationControl) -> Result<RawDocument> {
        Metadata::read_document_raw_controlled(self.0, key, control)
    }
    fn capture(&self, id: &str, maximum: usize, control: &OperationControl) -> Result<Captured> {
        let (reference, bytes) = evidence::fetch_capture_controlled(self.0, id, maximum, control)?;
        Ok(Captured { reference, bytes })
    }
    fn pin(&self, id: &str, control: &OperationControl) -> Result<PinnedSet> {
        let document = self.document(&format!("pin/{id}"), control)?;
        super::pins::read_pin(&document.bytes, id, control)
    }
}
pub(super) fn provenance_matches(
    a: &lycheedev_core::evidence::Provenance,
    b: &lycheedev_core::evidence::Provenance,
) -> bool {
    a.kind == b.kind
        && a.locator == b.locator
        && a.snapshot == b.snapshot
        && a.operation_id == b.operation_id
        && a.source_commit == b.source_commit
        && a.base_snapshot == b.base_snapshot
        && a.base_source_commit == b.base_source_commit
        && a.data_build == b.data_build
        && a.session == b.session
}
pub(super) fn provenance(
    kind: &str,
    locator: &str,
    snapshot: &str,
    operation: &str,
    build: &str,
    session: &str,
) -> lycheedev_core::evidence::Provenance {
    lycheedev_core::evidence::Provenance {
        kind: kind.into(),
        locator: locator.into(),
        snapshot: snapshot.into(),
        operation_id: operation.into(),
        source_commit: String::new(),
        base_snapshot: String::new(),
        base_source_commit: String::new(),
        data_build: build.into(),
        session: session.into(),
    }
}
