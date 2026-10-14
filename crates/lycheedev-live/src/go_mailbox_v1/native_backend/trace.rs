//! Bounded metadata only. Never serialize the read record/payload or perform a
//! second read to improve diagnostics. Failed writes retain the exact suffix.
use super::*;
use crate::{mailbox::ReadFailure, runtime_root::RootEvidence};
use serde::Serialize;
use std::{collections::VecDeque, path::Path};
const LIMIT: usize = 256;
const TRACE_JSON_LIMIT: usize = 64 << 20;

#[derive(Clone, Default)]
pub(super) struct ReaderEvidence {
    pub usage: [u64; 6],
    pub first_read_failure: Option<ReadFailure>,
    pub root: Option<RootEvidence>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LookupEvidence {
    // Cumulative actual formal-reader ledger, not a fabricated Go scan total.
    usage: [u64; 6],
    #[serde(skip_serializing_if = "Option::is_none")]
    first_read_failure: Option<ReadFailure>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LookupTrace {
    #[serde(rename = "Nonce")]
    nonce: String,
    #[serde(rename = "Runtime")]
    runtime: String,
    #[serde(rename = "Ticket")]
    ticket: String,
    #[serde(rename = "Kind")]
    kind: u8,
    #[serde(rename = "Matches")]
    matches: usize,
    #[serde(rename = "Lookup")]
    lookup: LookupEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_binding: Option<RootEvidence>,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    #[serde(skip)]
    sequence: usize,
}
pub(super) struct TracedReader<R> {
    reader: Option<R>,
    directory: Option<PathBuf>,
    lookups: Vec<LookupEvidence>,
    traces: VecDeque<LookupTrace>,
    sequence: usize,
    omitted: u64,
}
impl<R: Source> TracedReader<R> {
    pub(super) fn new(reader: R, directory: Option<PathBuf>) -> Self {
        Self {
            reader: Some(reader),
            directory,
            lookups: Vec::new(),
            traces: VecDeque::new(),
            sequence: 0,
            omitted: 0,
        }
    }
    #[cfg(test)]
    pub(super) fn reader_for_test(&mut self) -> &mut R {
        self.reader.as_mut().unwrap()
    }
    pub(super) fn require_open(&self) -> ChannelResult<()> {
        if self.reader.is_some() {
            Ok(())
        } else {
            Err(error("live.channel_mailbox_required"))
        }
    }
    pub(super) fn close(&mut self) -> ChannelResult<()> {
        match self.reader.take() {
            Some(reader) => reader.close(),
            None => Ok(()),
        }
    }
    pub(super) fn flush(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.flush_with(control, |control, path, bytes| {
            let parent = path
                .parent()
                .ok_or_else(|| error("live.channel_trace_path_invalid"))?;
            if !parent.is_absolute()
                || parent.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return Err(error("live.channel_trace_path_invalid"));
            }
            let root = parent
                .ancestors()
                .last()
                .ok_or_else(|| error("live.channel_trace_path_invalid"))?;
            check_control(control)?;
            storage::guard_path(root, path).map_err(toolkit_error)?;
            std::fs::create_dir_all(parent).map_err(|e| external(e.to_string()))?;
            check_control(control)?;
            storage::guard_path(root, path).map_err(toolkit_error)?;
            storage::atomic_write_controlled(path, bytes, control).map_err(toolkit_error)
        })
    }
    fn flush_with(
        &mut self,
        control: &OperationControl,
        mut write: impl FnMut(&OperationControl, &Path, &[u8]) -> ChannelResult<()>,
    ) -> ChannelResult<()> {
        let Some(directory) = self.directory.as_ref() else {
            return Ok(());
        };
        let result = (|| {
            while let Some(trace) = self.traces.front() {
                check_control(control)?;
                let bytes = trace_json(control, trace)?;
                check_control(control)?;
                let path = directory.join(format!("{:03}.json", trace.sequence));
                let written = write(control, &path, &bytes);
                // A reported successful durable write is removed even if the
                // original control expires immediately afterward. No replay of
                // already acknowledged writes; remaining records stay pending.
                if written.is_ok() {
                    self.traces.pop_front();
                }
                finish(written, check_control(control))?;
            }
            if self.omitted > 0 {
                check_control(control)?;
                let summary = serde_json::json!({ "observations": (self.sequence as u64).saturating_add(self.omitted),
                    "retained": self.sequence, "omitted": self.omitted, "truncated": true });
                let bytes = serde_json::to_vec(&summary).map_err(|e| external(e.to_string()))?;
                finish(
                    write(control, &directory.join("summary.json"), &bytes),
                    check_control(control),
                )?;
            }
            Ok(())
        })();
        result.map_err(|cause| join(error("live.channel_trace_flush"), cause))
    }
}
impl<R: Source> RecordReader for TracedReader<R> {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        eager: bool,
    ) -> ChannelResult<Vec<Record>> {
        self.require_open()?;
        let reader = self.reader.as_mut().unwrap();
        let result = reader.find(control, selector, accept, eager);
        let mut evidence = reader.evidence();
        let lookup = LookupEvidence {
            usage: evidence.usage,
            first_read_failure: evidence.first_read_failure.take(),
        };
        if self.lookups.len() < LIMIT {
            self.lookups.push(lookup.clone());
        }
        if self.directory.is_some() {
            if self.sequence >= LIMIT {
                self.omitted = self.omitted.saturating_add(1);
            } else {
                self.sequence += 1;
                self.traces.push_back(LookupTrace {
                    nonce: hex::encode(selector.nonce),
                    runtime: hex::encode(selector.runtime),
                    ticket: hex::encode(selector.ticket),
                    kind: selector.kind as u8,
                    matches: result.as_ref().map_or(0, Vec::len),
                    lookup,
                    root_binding: (self.sequence == 1).then_some(evidence.root).flatten(),
                    error: result
                        .as_ref()
                        .err()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    sequence: self.sequence,
                });
            }
        }
        result
    }
}
#[cfg(test)]
#[path = "trace_tests.rs"]
mod tests;

/// JSON serialization shares the flush control, including CPU/buffer work.
/// A serialization failure never discards an independently observed stop.
fn trace_json(control: &OperationControl, value: &impl Serialize) -> ChannelResult<Vec<u8>> {
    struct Buffer<'a> {
        bytes: Vec<u8>,
        control: &'a OperationControl,
        stop: Option<ChannelError>,
    }
    impl std::io::Write for Buffer<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Err(cause) = check_control(self.control) {
                self.stop = Some(cause);
                return Err(std::io::Error::other("trace serialization stopped"));
            }
            if bytes.len() > TRACE_JSON_LIMIT.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("trace exceeds bounded JSON limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        control,
        stop: None,
    };
    let result = serde_json::to_writer(&mut buffer, value).map_err(|e| external(e.to_string()));
    let result = finish(
        result,
        buffer.stop.map_or_else(|| check_control(control), Err),
    );
    result.map(|_| buffer.bytes)
}
