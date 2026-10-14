//! Fixed-Go channel persistence under an existing shared window claim.
//!
//! Oracle: Go 3.2.0 `3057a9666ab078270b63c7f06de748069b0d1f4b`,
//! channel/{artifacts,history,driver,project_windows}.go and
//! journal/{memory_log,window_run,window_owner,installation_admission}.go.
//! This adapter does not create/retire claims, implement Book, or authorize
//! native input. Its caller must already have admitted the selected connection.
//! Locks coordinate cooperating processes; they are not hostile-filesystem
//! isolation. Every uncertain write poisons this instance until lease release
//! and a fresh load. Torn tails are retained and never repaired implicitly.
#[cfg(test)]
use super::journal::read_log_bytes;
use super::journal::{
    CHECKPOINT_BYTES, CHECKPOINT_EVENTS, MAX_EVENT_BYTES, MAX_EVENTS, MAX_HISTORY_SEGMENTS,
    MAX_LOG_BYTES, MemoryEvent, StateJournal, event_hash, go_json_bytes, hydrate, prepare_event,
    snapshot,
};
use super::observations::{ChannelError, ChannelResult, check_control};
use super::state::{
    Operation, OperationStage, RecoveryPhase, ReloadPhase, State, validate_request,
};
use super::{ContractError, MAX_PAYLOAD, checksum, may_prepare_again, parse_token};
use lycheedev_core::{ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const OWNER_SCHEMA: &str = "lycheedev.window-owner.v1";
const OWNER_BYTES: usize = 4096;

fn error(code: &'static str) -> ChannelError {
    ContractError(code).into()
}
fn io_error(e: std::io::Error) -> ChannelError {
    ChannelError::External {
        causes: vec![e.to_string()],
    }
}
fn storage_error(e: ToolkitError) -> ChannelError {
    match e.code.as_str() {
        "command.cancelled" => ChannelError::Cancelled,
        "command.deadline" => ChannelError::DeadlineExceeded,
        _ => ChannelError::External {
            causes: vec![e.code, e.message],
        },
    }
}
fn joined(results: impl IntoIterator<Item = ChannelResult<()>>) -> ChannelResult<()> {
    let mut causes: Vec<_> = results.into_iter().filter_map(Result::err).collect();
    match causes.len() {
        0 => Ok(()),
        1 => Err(causes.remove(0)),
        _ => Err(ChannelError::Joined { causes }),
    }
}
fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn json(value: &impl Serialize) -> ChannelResult<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| error("live.channel_journal_invalid"))?;
    Ok(go_json_bytes(&bytes)?)
}
fn close_file(file: File) -> ChannelResult<()> {
    // This consuming safe platform boundary reports CloseHandle failures. Drop
    // is only the fallback for early unwinding, never a successful write check.
    lycheedev_windows::filesystem::close_file(file).map_err(io_error)
}

/// The exact shared claim record. This adapter reads it but never publishes or
/// removes it. An OS-released execution lease does not release this claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowOwner {
    pub schema: String,
    pub workspace_id: String,
    pub resource: String,
    pub operation_id: String,
    #[serde(
        rename = "intentSha256",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub intent_sha256: String,
}

/// Project-local channel paths, distinct from the global vault Book records
/// and the historical Rust lifecycle-v2 layout. Construction is read-only.
#[derive(Debug, Clone)]
pub struct ChannelPaths {
    project: PathBuf,
    connection_id: String,
    log: PathBuf,
    artifacts: PathBuf,
    history: PathBuf,
    results: PathBuf,
}
impl ChannelPaths {
    pub fn project_root(&self) -> &Path {
        &self.project
    }
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub fn log_path(&self) -> &Path {
        &self.log
    }
    pub fn artifact_directory(&self) -> &Path {
        &self.artifacts
    }
    pub fn history_directory(&self) -> &Path {
        &self.history
    }
    pub fn result_directory(&self) -> &Path {
        &self.results
    }
    pub fn new(project: &Path, connection_id: &str) -> ChannelResult<Self> {
        let owner = connection_id
            .strip_prefix("CON-")
            .ok_or_else(|| error("live.channel_connection_id_required"))?;
        parse_token(owner)?;
        let project = fs::canonicalize(project).map_err(io_error)?;
        if !fs::metadata(&project).map_err(io_error)?.is_dir() {
            return Err(error("live.channel_project_invalid"));
        }
        let live = project.join(".lycheedev").join("live");
        let connections = live.join("connections");
        let log = connections.join(format!("{connection_id}.jsonl"));
        let history = connections.join(format!("{connection_id}.jsonl.history"));
        Ok(Self {
            project,
            connection_id: connection_id.into(),
            log,
            artifacts: connections.join("artifacts"),
            history,
            results: live.join("results"),
        })
    }
    /// Go hashes the lower-case canonical project path. Windows Rust canonical
    /// paths add a verbatim prefix; strip only that spelling-only prefix.
    pub fn workspace_id(&self) -> ChannelResult<String> {
        let path = self
            .project
            .to_str()
            .ok_or_else(|| error("live.channel_project_invalid"))?;
        let ordinary = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
        };
        // Go strings.ToLower applies simple per-rune casing, without Rust's
        // contextual final-sigma or multi-scalar U+0130 expansion.
        let lower: String = ordinary
            .chars()
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect();
        Ok(storage::sha256(lower.as_bytes())[..32].to_owned())
    }
    fn check_state(&self, state: &State) -> ChannelResult<()> {
        state.validate()?;
        if state.id != self.connection_id {
            return Err(error("live.channel_project_owner_mismatch"));
        }
        Ok(())
    }
    fn artifact(&self, hash: &str) -> ChannelResult<PathBuf> {
        if !digest(hash) {
            return Err(error("live.channel_artifact_invalid"));
        }
        Ok(self.artifacts.join(hash))
    }
}

/// Retained lock file: never unlink it, including after a process crash.
struct Lease(File);
impl Lease {
    fn acquire(
        scope: &Path,
        resource: &str,
        wait: bool,
        control: &OperationControl,
    ) -> ChannelResult<Self> {
        check_control(control)?;
        let path = scope.join(format!("{}.lock", storage::sha256(resource.as_bytes())));
        ordinary_path(&path, false, true)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(io_error)?;
        let acquire = (|| {
            loop {
                check_control(control)?;
                match file.try_lock() {
                    Ok(()) => break,
                    Err(TryLockError::WouldBlock) if wait => {
                        let remaining = control.remaining().map_err(storage_error)?;
                        std::thread::sleep(Duration::from_millis(25).min(remaining));
                    }
                    Err(TryLockError::WouldBlock) => return Err(error("journal.resource_busy")),
                    Err(TryLockError::Error(e)) => return Err(io_error(e)),
                }
            }
            check_control(control)?;
            ordinary_path(scope, true, false)?;
            ordinary_path(&path, false, false)
        })();
        if let Err(e) = acquire {
            return joined([Err(e), close_file(file)]).map(|_| unreachable!());
        }
        Ok(Self(file))
    }
    fn close(self) -> ChannelResult<()> {
        close_file(self.0)
    }
}

fn ordinary_path(path: &Path, directory: bool, missing: bool) -> ChannelResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if missing && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_error(e)),
    };
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(error("journal.redirected_window_owner"));
    }
    let resolved = fs::canonicalize(path).map_err(io_error)?;
    // Both inputs are rooted in canonical parents, including their Windows
    // verbatim spelling. Go's ordinaryOwnerPath comparison is case-insensitive.
    if path.to_string_lossy().to_lowercase() != resolved.to_string_lossy().to_lowercase() {
        return Err(error("journal.redirected_window_owner"));
    }
    Ok(())
}

/// Bounded reads preserve missing-file separately and check cancellation every
/// 64 KiB. File size and regular-file checks are repeated on the opened handle.
fn read_file(
    path: &Path,
    maximum: usize,
    invalid: &'static str,
    control: &OperationControl,
) -> ChannelResult<Option<Vec<u8>>> {
    check_control(control)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e)),
    };
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(error(invalid));
    }
    let mut file = File::open(path).map_err(io_error)?;
    let read = (|| {
        let metadata = file.metadata().map_err(io_error)?;
        if !metadata.is_file() || metadata.len() > maximum as u64 {
            return Err(error(invalid));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        let mut buffer = [0u8; 65536];
        loop {
            check_control(control)?;
            let remaining = maximum.saturating_sub(bytes.len());
            let take = buffer.len().min(remaining.saturating_add(1));
            let count = file.read(&mut buffer[..take]).map_err(io_error)?;
            if count == 0 {
                break;
            }
            if count > remaining {
                return Err(error(invalid));
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        check_control(control)?;
        Ok(bytes)
    })();
    let close = close_file(file);
    match read {
        Ok(bytes) => {
            close?;
            Ok(Some(bytes))
        }
        Err(e) => joined([Err(e), close]).map(|_| unreachable!()),
    }
}
fn read_owner(scope: &Path, want: &WindowOwner, control: &OperationControl) -> ChannelResult<()> {
    ordinary_path(scope, true, false)?;
    let marker = scope.join(format!(
        "{}.json",
        storage::sha256(want.resource.as_bytes())
    ));
    ordinary_path(&marker, false, false)?;
    let bytes = read_file(
        &marker,
        OWNER_BYTES,
        "journal.invalid_window_owner",
        control,
    )?
    .ok_or_else(|| error("journal.resource_busy"))?;
    let owner: WindowOwner =
        serde_json::from_slice(&bytes).map_err(|_| error("journal.invalid_window_owner"))?;
    if json(&owner)? != bytes || owner.schema != OWNER_SCHEMA {
        return Err(error("journal.invalid_window_owner"));
    }
    if owner != *want {
        return Err(error("journal.resource_busy"));
    }
    Ok(())
}
struct Segment {
    bytes: Vec<u8>,
    events: Vec<MemoryEvent>,
}
fn read_segment(path: &Path, control: &OperationControl) -> ChannelResult<Segment> {
    // Preserve Go's distinct directory/link error rather than calling it an
    // empty or missing journal.
    if let Ok(m) = fs::symlink_metadata(path) {
        if !m.is_file() {
            return Err(error("journal.memory_log_not_regular"));
        }
    }
    let bytes =
        read_file(path, MAX_LOG_BYTES, "journal.memory_log_limit", control)?.unwrap_or_default();
    let mut events: Vec<MemoryEvent> = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        check_control(control)?;
        let Some(relative) = bytes[offset..].iter().position(|byte| *byte == b'\n') else {
            return Err(error(if bytes.len() - offset > MAX_EVENT_BYTES {
                "journal.memory_log_limit"
            } else {
                "journal.memory_torn_tail"
            }));
        };
        let end = offset + relative + 1;
        if end - offset > MAX_EVENT_BYTES || events.len() >= MAX_EVENTS {
            return Err(error("journal.memory_log_limit"));
        }
        let event: MemoryEvent = serde_json::from_slice(&bytes[offset..end])
            .map_err(|_| error("journal.memory_log_corrupt"))?;
        let previous = events.last().map_or("", |e| e.checksum.as_str());
        if event.sequence != events.len() as i64 + 1
            || event.previous != previous
            || event_hash(&event)? != event.checksum
        {
            return Err(error("journal.memory_log_corrupt"));
        }
        events.push(event);
        offset = end;
    }
    check_control(control)?;
    Ok(Segment { bytes, events })
}
fn read_state(
    paths: &ChannelPaths,
    event: &MemoryEvent,
    control: &OperationControl,
) -> ChannelResult<State> {
    check_control(control)?;
    let state: State = serde_json::from_str(event.data.get())
        .map_err(|_| error("live.channel_journal_invalid"))?;
    // hydrate's callback uses ContractError; read I/O and cancellation separately
    // first so neither is accidentally relabeled as artifact corruption.
    if state.artifacts.len() > 4
        || state
            .artifacts
            .keys()
            .any(|key| !matches!(key.as_str(), "code" | "result" | "transaction" | "recovery"))
    {
        return Err(error("live.channel_artifact_invalid"));
    }
    let mut artifacts = BTreeMap::new();
    for hash in state.artifacts.values() {
        if artifacts.contains_key(hash) {
            continue;
        }
        let bytes = read_file(
            &paths.artifact(hash)?,
            MAX_PAYLOAD,
            "live.channel_artifact_invalid",
            control,
        )?
        .ok_or_else(|| io_error(std::io::Error::from(std::io::ErrorKind::NotFound)))?;
        artifacts.insert(hash.clone(), bytes);
    }
    let state = hydrate(state, |hash| {
        artifacts
            .get(hash)
            .cloned()
            .ok_or(ContractError("live.channel_artifact_invalid"))
    })?;
    paths.check_state(&state)?;
    check_control(control)?;
    Ok(state)
}

/// Read-only status load: no lease, directories, repair, or filesystem writes.
/// A concurrent writer can make this read fail; retry under the owning lease
/// before any effect. Missing/empty and truncated logs never create new state.
pub fn load(paths: &ChannelPaths, control: &OperationControl) -> ChannelResult<State> {
    let segment = read_segment(&paths.log, control)?;
    let last = segment
        .events
        .last()
        .ok_or_else(|| error("live.channel_journal_missing"))?;
    read_state(paths, last, control)
}

fn history(
    paths: &ChannelPaths,
    request: &str,
    control: &OperationControl,
) -> ChannelResult<Option<State>> {
    let mut current = paths.log.clone();
    let mut expected = String::new();
    let mut seen = BTreeSet::new();
    for _ in 0..MAX_HISTORY_SEGMENTS {
        check_control(control)?;
        let segment = read_segment(&current, control)?;
        if segment.events.is_empty() {
            if !expected.is_empty() {
                return Err(error("live.channel_journal_missing"));
            }
            return Ok(None);
        }
        let last = segment.events.last().unwrap();
        if !expected.is_empty() && last.checksum != expected {
            return Err(error("live.channel_history_corrupt"));
        }
        let mut archive = String::new();
        for (i, event) in segment.events.iter().enumerate().rev() {
            let state = read_state(paths, event, control)?;
            if i == segment.events.len() - 1 {
                archive = state.archive.clone();
            }
            // Fixed Go returns the newest matching state immediately. Older
            // events/artifacts/segments are relevant only when still searching.
            if state
                .operation
                .as_ref()
                .is_some_and(|op| op.request == request)
            {
                return Ok(Some(state));
            }
        }
        if archive.is_empty() {
            return Ok(None);
        }
        if !digest(&archive) || !seen.insert(archive.clone()) {
            return Err(error("live.channel_history_corrupt"));
        }
        current = paths.history.join(format!("{archive}.jsonl"));
        expected = archive;
    }
    Err(error("live.channel_history_limit"))
}

/// Read-only idempotency query. Return the newest match immediately, as Go
/// historicalState does. Before a match, missing/corrupt required history is
/// an error, never permission to treat a request as new. Stored original result
/// bytes are returned without parsing and re-serializing the report.
pub fn historical_request(
    paths: &ChannelPaths,
    request: &str,
    control: &OperationControl,
) -> ChannelResult<Option<State>> {
    validate_request(request)?;
    history(paths, request, control)
}

pub struct DurableChannelJournal {
    paths: ChannelPaths,
    scope: PathBuf,
    owner: WindowOwner,
    lease: Option<Lease>,
    head: String,
    poisoned: bool,
    #[cfg(test)]
    fault: Option<tests::Fault>,
}
impl DurableChannelJournal {
    /// Own the exact existing Go connection claim's execution lock. The short
    /// admission lock waits at most the original control budget; execution is
    /// try-only. A fresh replay under this lease is mandatory before driving.
    pub fn acquire(
        paths: ChannelPaths,
        addon_parent: &Path,
        owner: WindowOwner,
        control: &OperationControl,
    ) -> ChannelResult<Self> {
        check_control(control)?;
        if owner.schema != OWNER_SCHEMA
            || owner.operation_id != paths.connection_id
            || owner.workspace_id != paths.workspace_id()?
            || !owner.resource.starts_with("window/")
            || owner.resource.len() > 256
            || owner.intent_sha256.len() != 64
            || hex::decode(&owner.intent_sha256).is_err()
        {
            return Err(error("journal.invalid_bootstrap_claim"));
        }
        let parent = fs::canonicalize(addon_parent).map_err(io_error)?;
        let scope = parent.join(".lycheedev-window-owners");
        // Existing ownership is a prerequisite. Never turn a missing scope or
        // claim into new admission, and never create a claim as a side effect.
        ordinary_path(&scope, true, false)?;
        let admission = Lease::acquire(&scope, "window-admission", true, control)?;
        let acquire = (|| {
            read_owner(&scope, &owner, control)?;
            Lease::acquire(
                &scope,
                &format!("execution:{}", owner.resource),
                false,
                control,
            )
        })();
        let gate_close = admission.close();
        let lease = match acquire {
            Ok(lease) => match gate_close {
                Ok(()) => lease,
                Err(e) => return joined([Err(e), lease.close()]).map(|_| unreachable!()),
            },
            Err(e) => return joined([Err(e), gate_close]).map(|_| unreachable!()),
        };
        let segment = match read_segment(&paths.log, control) {
            Ok(segment) => segment,
            Err(e) => return joined([Err(e), lease.close()]).map(|_| unreachable!()),
        };
        // Connection admission persists the initial log before publishing its
        // claim. This existing-claim adapter must never reseed a lost chain.
        let Some(last) = segment.events.last() else {
            return joined([Err(error("live.channel_journal_missing")), lease.close()])
                .map(|_| unreachable!());
        };
        let head = last.checksum.clone();
        let mut journal = Self {
            paths,
            scope,
            owner,
            lease: Some(lease),
            head,
            poisoned: false,
            #[cfg(test)]
            fault: None,
        };
        if let Err(e) = read_state(&journal.paths, last, control) {
            return joined([Err(e), journal.close()]).map(|_| unreachable!());
        }
        if let Err(e) = journal.check(control) {
            return joined([Err(e), journal.close()]).map(|_| unreachable!());
        }
        Ok(journal)
    }
    pub fn paths(&self) -> &ChannelPaths {
        &self.paths
    }
    pub fn load(&self, control: &OperationControl) -> ChannelResult<State> {
        self.check(control)?;
        let segment = self.segment(control)?;
        read_state(
            &self.paths,
            segment
                .events
                .last()
                .ok_or_else(|| error("live.channel_journal_missing"))?,
            control,
        )
    }
    /// Explicit close reports errors. Drop still releases the OS handle on
    /// unwind; no marker, log, history, artifact, or lock file is removed.
    pub fn close(&mut self) -> ChannelResult<()> {
        self.lease.take().map_or(Ok(()), Lease::close)
    }
    fn check(&self, control: &OperationControl) -> ChannelResult<()> {
        check_control(control)?;
        if self.lease.is_none() {
            return Err(error("journal.window_run_closed"));
        }
        if self.poisoned {
            return Err(error("live.channel_journal_reload_required"));
        }
        read_owner(&self.scope, &self.owner, control)
    }
    fn segment(&self, control: &OperationControl) -> ChannelResult<Segment> {
        let segment = read_segment(&self.paths.log, control)?;
        if segment.events.last().map(|e| &e.checksum) != Some(&self.head) {
            return Err(error("journal.memory_checkpoint_conflict"));
        }
        Ok(segment)
    }
    fn immutable(
        &mut self,
        path: &Path,
        bytes: &[u8],
        maximum: usize,
        invalid: &'static str,
        conflict: &'static str,
        control: &OperationControl,
    ) -> ChannelResult<()> {
        check_control(control)?;
        if bytes.len() > maximum {
            return Err(error(invalid));
        }
        if let Some(existing) = read_file(path, maximum, invalid, control)? {
            if existing != bytes {
                return Err(error(conflict));
            }
            return Ok(());
        }
        storage::atomic_write_controlled(path, bytes, control).map_err(storage_error)
    }
    fn snapshot(&mut self, state: &State, control: &OperationControl) -> ChannelResult<State> {
        self.paths.check_state(state)?;
        let snapshot = snapshot(state)?;
        for (digest, bytes) in &snapshot.artifacts {
            self.check(control)?;
            self.immutable(
                &self.paths.artifact(digest)?,
                bytes,
                MAX_PAYLOAD,
                "live.channel_artifact_invalid",
                "live.channel_artifact_corrupt",
                control,
            )?;
            self.cut(FaultPoint::ArtifactDurable, control)?;
        }
        Ok(snapshot.state)
    }
    fn append_inner(
        &mut self,
        control: &OperationControl,
        event: &str,
        state: &State,
    ) -> ChannelResult<()> {
        self.check(control)?;
        let segment = self.segment(control)?;
        let snapshot = self.snapshot(state, control)?;
        let bytes = prepare_event(&segment.events, event, &snapshot)?;
        // prepare_event applies Go's JSON-only 2 MiB write bound. The fixed
        // Go reader instead bounds the LF-inclusive line; deliberately retain
        // that upstream inconsistency at exactly 2 MiB of JSON. Total-log
        // admission includes the newline, as AppendMemoryEvent does.
        if bytes.len() > MAX_LOG_BYTES.saturating_sub(segment.bytes.len()) {
            return Err(error("journal.memory_log_limit"));
        }
        self.check(control)?;
        self.cut(FaultPoint::BeforeAppend, control)?;
        fs::create_dir_all(self.paths.log.parent().unwrap()).map_err(io_error)?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // permit readers, deny other writes/deletes while appending
        }
        let mut file = options.open(&self.paths.log).map_err(io_error)?;
        let write = (|| {
            let metadata = file.metadata().map_err(io_error)?;
            if !metadata.is_file() || metadata.len() != segment.bytes.len() as u64 {
                return Err(error("journal.memory_checkpoint_conflict"));
            }
            self.check(control)?;
            // A deterministic fixture can leave an actual incomplete final
            // record before the ordinary write path; no production branch.
            #[cfg(test)]
            if self
                .fault
                .as_ref()
                .is_some_and(|f| f.point == FaultPoint::AppendPartial)
            {
                file.write_all(&bytes[..bytes.len() / 2])
                    .map_err(io_error)?;
                self.cut(FaultPoint::AppendPartial, control)?;
            }
            for chunk in bytes.chunks(65536) {
                check_control(control)?;
                file.write_all(chunk).map_err(io_error)?;
            }
            self.cut(FaultPoint::AppendWritten, control)
        })();
        // Always attempt sync AND checked close, even after a short write or
        // cancellation. Join independent failures instead of hiding one.
        let sync = file.sync_all().map_err(io_error);
        let sync_fault = self.cut(FaultPoint::AppendSynced, control);
        let close = close_file(file);
        let close_fault = self.cut(FaultPoint::AppendClosed, control);
        joined([
            write,
            sync,
            sync_fault,
            close,
            close_fault,
            check_control(control),
        ])?;
        // Standalone non-first event cannot replay from sequence one. Extract
        // only its hash after the actual append has completed successfully.
        let last: MemoryEvent =
            serde_json::from_slice(&bytes).map_err(|_| error("journal.memory_log_corrupt"))?;
        self.head = last.checksum;
        Ok(())
    }
    fn checkpoint_inner(
        &mut self,
        control: &OperationControl,
        state: &mut State,
        allow_pending_known_zero: bool,
    ) -> ChannelResult<()> {
        self.check(control)?;
        self.paths.check_state(state)?;
        let quiet = state.transaction.is_none()
            && state
                .reload
                .as_ref()
                .is_none_or(|r| r.phase == ReloadPhase::Complete)
            && state
                .recovery
                .as_ref()
                .is_none_or(|r| r.phase == RecoveryPhase::Complete)
            && state.operation.as_ref().is_none_or(|o| {
                matches!(
                    o.stage,
                    OperationStage::Complete
                        | OperationStage::ExecutionUnknown
                        | OperationStage::Cancelled
                )
            });
        let known_zero = state
            .input
            .as_ref()
            .is_some_and(|i| may_prepare_again(i.outcome.as_ref()));
        if !quiet && !(allow_pending_known_zero && known_zero) {
            return Err(ChannelError::Pending { causes: vec![] });
        }
        let segment = self.segment(control)?;
        if segment.events.len() < CHECKPOINT_EVENTS && segment.bytes.len() < CHECKPOINT_BYTES {
            return Ok(());
        }
        let expected = &segment
            .events
            .last()
            .ok_or_else(|| error("journal.memory_checkpoint_conflict"))?
            .checksum;
        let previous = state.archive.clone();
        state.archive = expected.clone();
        let checkpoint = (|| {
            let snapshot = self.snapshot(state, control)?;
            let bytes = prepare_event(&[], "checkpoint", &snapshot)?;
            // Do not run the stricter reader over the just-generated event:
            // Go accepts 2 MiB JSON plus LF here, even though ReadMemoryLog
            // later rejects that LF-inclusive boundary. Capture its own hash.
            let checkpoint: MemoryEvent =
                serde_json::from_slice(&bytes).map_err(|_| error("journal.memory_log_corrupt"))?;
            let archive = self.paths.history.join(format!("{expected}.jsonl"));
            self.immutable(
                &archive,
                &segment.bytes,
                MAX_LOG_BYTES,
                "journal.memory_checkpoint_conflict",
                "journal.memory_checkpoint_conflict",
                control,
            )?;
            self.cut(FaultPoint::ArchiveDurable, control)?;
            self.check(control)?;
            self.segment(control)?;
            self.cut(FaultPoint::BeforeCheckpointReplace, control)?;
            storage::atomic_write_controlled(&self.paths.log, &bytes, control)
                .map_err(storage_error)?;
            self.cut(FaultPoint::CheckpointDurable, control)?;
            self.head = checkpoint.checksum;
            Ok(())
        })();
        if checkpoint.is_err() {
            state.archive = previous;
        }
        checkpoint
    }
    fn cut(&mut self, point: FaultPoint, control: &OperationControl) -> ChannelResult<()> {
        #[cfg(test)]
        if self.fault.as_ref().is_some_and(|f| f.point == point) {
            return self.fault.take().unwrap().trigger(control);
        }
        let _ = (point, control);
        Ok(())
    }
}
impl StateJournal for DurableChannelJournal {
    fn append(
        &mut self,
        control: &OperationControl,
        event: &str,
        state: &State,
    ) -> ChannelResult<()> {
        let result = self.append_inner(control, event, state);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn historical_request(
        &mut self,
        control: &OperationControl,
        request: &str,
    ) -> ChannelResult<Option<State>> {
        self.check(control)?;
        self.segment(control)?;
        historical_request(&self.paths, request, control)
    }
    fn write_result(
        &mut self,
        control: &OperationControl,
        operation: &Operation,
    ) -> ChannelResult<()> {
        let result = (|| {
            self.check(control)?;
            let state = self.load(control)?;
            if state.operation.as_ref() != Some(operation)
                || operation.stage != OperationStage::ResultVerified
                || operation.result_bytes.0.is_empty()
                || operation.result_bytes.0.len() > MAX_PAYLOAD
                || operation.report_bytes as usize != operation.result_bytes.0.len()
                || operation.report_checksum != checksum(&operation.result_bytes.0)
            {
                return Err(error("live.channel_result_invalid"));
            }
            // Operation::validate is part of State validation. Preserve its
            // LMO identity and exact base64 resultBytes, not parsed RawMessage.
            let bytes = json(operation)?;
            let path = self.paths.results.join(format!("{}.json", operation.id));
            storage::atomic_write_controlled(&path, &bytes, control).map_err(storage_error)?;
            self.cut(FaultPoint::ResultDurable, control)
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn checkpoint(
        &mut self,
        control: &OperationControl,
        state: &mut State,
        allow_pending_known_zero: bool,
    ) -> ChannelResult<()> {
        let result = self.checkpoint_inner(control, state, allow_pending_known_zero);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
}

// Test cuts perform real filesystem operations up to this boundary; production
// contains no fault configuration or environment-variable control surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultPoint {
    ArtifactDurable,
    BeforeAppend,
    #[cfg(test)]
    AppendPartial,
    AppendWritten,
    AppendSynced,
    AppendClosed,
    ArchiveDurable,
    BeforeCheckpointReplace,
    CheckpointDurable,
    ResultDurable,
}

#[cfg(test)]
#[path = "durable_journal_tests.rs"]
mod tests;
