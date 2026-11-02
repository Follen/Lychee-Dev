//! Test adapters for the owned ingestion API; no patched engine entry point.
use emmylua_code_analysis::{EmmyLuaAnalysis, FileId};
use lycheedev_emmy_adapter::{IngestionFailure, ingest_public_batch};
use std::{path::PathBuf, sync::Arc};

pub trait PublicIngestionTest {
    fn owned_ingest_shared<E>(
        &mut self,
        files: Vec<(PathBuf, Option<Arc<str>>)>,
        check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<FileId>, IngestionFailure<E>>;
    fn owned_ingest_strings<E>(
        &mut self,
        files: Vec<(PathBuf, Option<String>)>,
        check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<FileId>, IngestionFailure<E>>;
}
impl PublicIngestionTest for EmmyLuaAnalysis {
    fn owned_ingest_shared<E>(
        &mut self,
        files: Vec<(PathBuf, Option<Arc<str>>)>,
        check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<FileId>, IngestionFailure<E>> {
        ingest_public_batch(self, files, 512 << 20, check).map(|batch| batch.file_ids)
    }
    fn owned_ingest_strings<E>(
        &mut self,
        files: Vec<(PathBuf, Option<String>)>,
        check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<FileId>, IngestionFailure<E>> {
        self.owned_ingest_shared(
            files
                .into_iter()
                .map(|(path, text)| (path, text.map(Arc::from)))
                .collect(),
            check,
        )
    }
}
