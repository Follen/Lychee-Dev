//! Included as durable_journal::activation_history. Exact reload history uses
//! the same bounded authenticated segment/artifact replay as request history.
#![forbid(unsafe_code)]
use super::*;
pub fn historical_reload(
    paths: &ChannelPaths,
    request: &str,
    control: &OperationControl,
) -> ChannelResult<Option<State>> {
    validate_request(request)?;
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
        for (index, event) in segment.events.iter().enumerate().rev() {
            let state = read_state(paths, event, control)?;
            if index == segment.events.len() - 1 {
                archive = state.archive.clone();
            }
            if state.reload.as_ref().is_some_and(|r| r.request == request) {
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

/// First activation channel event, under its already-held exact execution
/// lease. A preexisting/partial log is evidence and is never replaced here.
pub fn seed_activation(
    paths: &ChannelPaths,
    state: &State,
    control: &OperationControl,
) -> ChannelResult<()> {
    check_control(control)?;
    paths.check_state(state)?;
    let prepared = snapshot(state)?;
    // Activation cannot introduce source/report artifacts or business requests.
    if !prepared.artifacts.is_empty()
        || state.operation.is_some()
        || state.transaction.is_some()
        || state.bound
        || state.closed
        || state.progress_version != 1
    {
        return Err(error("journal.invalid_transition"));
    }
    let event = if state.reload.is_some() {
        "activation_observed_reload"
    } else {
        "activation_runtime_selected"
    };
    let bytes = prepare_event(&[], event, &prepared.state)?;
    fs::create_dir_all(paths.log.parent().unwrap()).map_err(io_error)?;
    check_control(control)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.log)
        .map_err(io_error)?;
    let write = (|| {
        for chunk in bytes.chunks(65536) {
            check_control(control)?;
            file.write_all(chunk).map_err(io_error)?;
        }
        Ok(())
    })();
    let sync = file.sync_all().map_err(io_error);
    let close = close_file(file);
    joined([write, sync, close, check_control(control)])
}
