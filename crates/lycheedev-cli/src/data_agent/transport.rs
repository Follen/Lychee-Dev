//! Bounded ingress and cancellation independent of the busy query worker.
//! Only the worker owns Engine/results. The reader owns stdin, never a table or
//! native handle. Its blocking read is detached only until this CLI process exits.
use super::{ReceiptEvent, ReceiptObserver};
use lycheedev_data::{
    agent_session::{PROTOCOL, Request, Response, Session, Task},
    request::{DataContext, SessionConfig},
};
use lycheedev_core::{
    Result, ToolkitError,
    control::{OperationControl, TrackedCancellation},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    sync::{
        Arc, Mutex,
        mpsc::{self, TrySendError},
    },
    time::{Duration, Instant},
};
fn fault(code: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(code, message, exit)
}

const MAX_LINE: usize = 1 << 20;
const QUEUE: usize = 16;
struct OutputState {
    writer: Mutex<std::io::Stdout>,
    observer: Option<ReceiptObserver>,
}
type Output = Arc<OutputState>;
struct Pending {
    request: Request,
    control: OperationControl,
}
#[derive(Default)]
struct State {
    requests: BTreeMap<u64, TrackedCancellation>,
    closing: bool,
    close_id: Option<u64>,
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|error| error.into_inner())
}
fn stop(state: &Mutex<State>, id: Option<u64>) {
    let mut state = lock(state);
    state.closing = true;
    state.close_id = id;
    for cancellation in state.requests.values() {
        cancellation.cancel();
    }
}
fn send(output: &Output, id: Option<u64>, result: Result<Response>) -> Result<()> {
    #[derive(serde::Serialize)]
    struct Frame<'a> {
        protocol: &'static str,
        id: Option<u64>,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<&'a Response>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a ToolkitError>,
    }
    let frame = match &result {
        Ok(value) => Frame {
            protocol: PROTOCOL,
            id,
            ok: true,
            result: Some(value),
            error: None,
        },
        Err(error) => Frame {
            protocol: PROTOCOL,
            id,
            ok: false,
            result: None,
            error: Some(error),
        },
    };
    let outcome: Result<()> = (|| {
        let mut writer = lock(&output.writer);
        serde_json::to_writer(&mut *writer, &frame)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        drop(writer);
        if let (Some(id), Some(observer)) = (id, &output.observer) {
            observer(ReceiptEvent::Completed(id))?;
        }
        Ok(())
    })();
    outcome.map_err(|error| match &result {
        Ok(Response::Capture(capture) | Response::Domain { capture, .. }) => {
            capture.committed_error(error)
        }
        Err(primary) if primary.details.get("capturePublication").is_some() => {
            let mut primary = primary.clone();
            primary.details["transportFailure"] = json!(error);
            primary
        }
        _ => error,
    })
}
/// Bounded line reader checks size before extending the retained request.
fn read_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(fault(
                    "data.session.incomplete_frame",
                    4,
                    "EOF in a non-terminated JSONL frame",
                ))
            };
        }
        let end = buffer.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(buffer.len(), |position| position + 1);
        if line
            .len()
            .checked_add(count)
            .is_none_or(|length| length > MAX_LINE)
        {
            return Err(fault(
                "data.session.frame_limit",
                3,
                "Agent request exceeds one MiB; worker is closing",
            ));
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(Some(line));
        }
    }
}
fn reader(
    parent: OperationControl,
    state: Arc<Mutex<State>>,
    output: Output,
    queue: mpsc::SyncSender<Pending>,
) {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut last_id = 0;
    loop {
        if lock(&state).closing {
            break;
        }
        let line = match read_line(&mut input) {
            Ok(Some(line)) => line,
            Ok(None) => {
                stop(&state, None);
                break;
            }
            Err(error) => {
                let _ = send(&output, None, Err(error));
                stop(&state, None);
                break;
            }
        };
        let request: Request = match serde_json::from_slice(&line) {
            Ok(request) => request,
            Err(error) => {
                if send(
                    &output,
                    None,
                    Err(fault("data.session.invalid_frame", 2, error.to_string())),
                )
                .is_err()
                {
                    stop(&state, None);
                    break;
                }
                continue;
            }
        };
        let id = request.id;
        let validation = if request.protocol != PROTOCOL {
            Err(fault(
                "data.session.protocol",
                3,
                "Unsupported Agent protocol version",
            ))
        } else if id <= last_id {
            Err(fault(
                "data.session.request_order",
                2,
                "Request IDs must increase strictly; requests are never replayed",
            ))
        } else if request.timeout_ms == 0 || request.timeout_ms > 3_600_000 {
            Err(fault(
                "data.session.timeout",
                2,
                "timeout_ms must be 1..3600000",
            ))
        } else {
            Ok(())
        };
        // Reserve every syntactically valid new ID even if admission fails.
        // A later retry must use a new ID and an intentional new request.
        last_id = last_id.max(id);
        if let Err(error) = validation {
            if send(&output, Some(id), Err(error)).is_err() {
                stop(&state, None);
                break;
            }
            continue;
        }
        match request.task {
            Task::Close => {
                stop(&state, Some(id));
                break;
            }
            Task::Cancel { request_id } => {
                let found = {
                    let state = lock(&state);
                    state
                        .requests
                        .get(&request_id)
                        .map(|token| {
                            token.cancel();
                            true
                        })
                        .unwrap_or(false)
                };
                if send(&output, Some(id), Ok(Response::Value(json!({"requestId":request_id,"cancellationRequested":found,"replayed":false})))).is_err() { stop(&state, None); break; }
                continue;
            }
            _ => {}
        }
        let control = match parent
            .ordered_child(Instant::now() + Duration::from_millis(request.timeout_ms))
        {
            Ok(control) => control,
            Err(error) => {
                let _ = send(&output, Some(id), Err(error));
                stop(&state, None);
                break;
            }
        };
        let token = match control.tracked_cancel_handle() {
            Ok(token) => token,
            Err(error) => {
                let _ = send(&output, Some(id), Err(error));
                stop(&state, None);
                break;
            }
        };
        lock(&state).requests.insert(id, token);
        match queue.try_send(Pending { request, control }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                lock(&state).requests.remove(&id);
                if send(
                    &output,
                    Some(id),
                    Err(fault(
                        "data.session.queue_full",
                        3,
                        "Agent request queue is full; no work was started",
                    )),
                )
                .is_err()
                {
                    stop(&state, None);
                    break;
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                stop(&state, None);
                break;
            }
        }
    }
}
pub(super) fn serve_stdio_observed(
    context: DataContext,
    config: SessionConfig,
    observer: ReceiptObserver,
) -> Result<()> {
    run(context, config, Some(observer))
}
fn run(context: DataContext, config: SessionConfig, observer: Option<ReceiptObserver>) -> Result<()> {
    context.control.check()?;
    let parent = context.control.clone();
    let mut session = Session::new(context, config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let output = Arc::new(OutputState {
        writer: Mutex::new(std::io::stdout()),
        observer,
    });
    let state = Arc::new(Mutex::new(State::default()));
    let (sender, receiver) = mpsc::sync_channel(QUEUE);
    let thread_state = state.clone();
    let thread_output = output.clone();
    let thread_control = parent.clone();
    std::thread::Builder::new()
        .name("data-agent-ingress".into())
        .spawn(move || reader(thread_control, thread_state, thread_output, sender))?;
    if let Some(observer) = &output.observer {
        observer(ReceiptEvent::Ready)?;
    }

    let outcome = (|| {
        loop {
            if lock(&state).closing {
                break;
            }
            parent.check()?;
            session.expire();
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(pending) => {
                    let id = pending.request.id;
                    let result =
                        runtime.block_on(session.execute(pending.request.task, pending.control));
                    lock(&state).requests.remove(&id);
                    send(&output, Some(id), result)?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(())
    })();
    let close_id = lock(&state).close_id;
    stop(&state, close_id);
    // Drain cancelled pending work without executing it or replaying a query.
    while let Ok(pending) = receiver.try_recv() {
        let _ = send(
            &output,
            Some(pending.request.id),
            Err(fault(
                "command.cancelled",
                7,
                "Agent session closed before execution",
            )),
        );
    }
    let cleanup = session.close();
    drop(session);
    lock(&state).requests.clear();
    let outcome = match (outcome, cleanup) {
        (Ok(()), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(mut error), Err(cleanup)) => {
            if let Some(details) = error.details.as_object_mut() {
                details.insert("cleanupFailure".into(), json!(cleanup));
            } else {
                error.details = json!({"primary":error.clone(),"cleanup":cleanup});
            }
            Err(error)
        }
    };
    if let Some(id) = close_id {
        if let Err(error) = &outcome {
            send(&output, Some(id), Err(error.clone()))?;
            return outcome;
        }
        send(
            &output,
            Some(id),
            Ok(Response::Value(
                json!({"closed":true,"handlesReleased":true}),
            )),
        )?;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixture::{self, Fixture};
    use lycheedev_core::{evidence, metadata::Metadata};
    use std::sync::atomic::{AtomicUsize, Ordering};


    fn receipt_failure_preserves_committed_capture(home: &std::path::Path, response: Response) {
        let value = serde_json::to_value(&response).unwrap();
        let capture = value.get("capture").unwrap_or(&value);
        let id = capture["id"].as_str().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let output = Arc::new(OutputState {
            writer: Mutex::new(std::io::stdout()),
            observer: Some(Arc::new(move |event| {
                assert!(matches!(event, ReceiptEvent::Completed(7)));
                observed.fetch_add(1, Ordering::SeqCst);
                Err(fault(
                    "test.receipt_finalize",
                    5,
                    "post-flush receipt finalization failed",
                ))
            })),
        });
        // Run the actual serializer/newline/flush path before its existing
        // completion observer fails. This is not an injected stdout failure.
        let primary = send(&output, Some(7), Ok(response)).unwrap_err();
        assert_eq!(primary.code, "test.receipt_finalize");
        assert_eq!(
            primary.details["capturePublication"],
            json!({"state":"committed","id":id})
        );
        assert_eq!(primary.details["completedCaptures"], json!([id]));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Sending that committed error can itself fail delivery. Its original
        // publication identity must survive rather than becoming an IO-only error.
        let delivered = send(&output, Some(7), Err(primary.clone())).unwrap_err();
        assert_eq!(delivered.code, primary.code);
        assert_eq!(
            delivered.details["capturePublication"],
            primary.details["capturePublication"]
        );
        assert_eq!(
            delivered.details["completedCaptures"],
            primary.details["completedCaptures"]
        );
        assert_eq!(delivered.details["primary"], primary.details["primary"]);
        assert_eq!(
            delivered.details["transportFailure"]["code"],
            "test.receipt_finalize"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "one completion per explicit send; no automatic retry"
        );

        let metadata = Metadata::open(home, false).unwrap();
        let captures = metadata.list::<serde_json::Value>("capture/", 10).unwrap();
        metadata.close().unwrap();
        assert_eq!(
            captures,
            vec![capture.clone()],
            "delivery failures must not replay capture work"
        );
        assert_eq!(evidence::read_capture(home, id).unwrap(), *capture);
        let body = evidence::read_blob(home, &capture["blob"], 16 << 20).unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&body).is_ok());
    }

    #[test]
    fn session_capture_keeps_committed_identity_after_post_flush_receipt_failure() {
        let fx = Fixture::new(&[fixture::table(
            "Sample",
            921,
            &[("ID", "uint")],
            &[vec![fixture::Cell::U(1)]],
        )]);
        let mut session = Session::new(fx.context(), Default::default()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let query = serde_json::from_value(
            json!({"kind":"plan","plan":{"op":"scan","table":{"name":"Sample","mode":"static"}}}),
        )
        .unwrap();
        let result = runtime
            .block_on(session.execute(Task::Query { query }, OperationControl::default()))
            .unwrap();
        let value = serde_json::to_value(&result).unwrap();
        let handle = value["result"]["handle"].as_str().unwrap().to_owned();
        drop(result);
        let response = runtime
            .block_on(session.execute(
                Task::Capture {
                    handle: handle.clone(),
                },
                OperationControl::default(),
            ))
            .unwrap();
        receipt_failure_preserves_committed_capture(&fx.home, response);
        let page = runtime
            .block_on(session.execute(
                Task::Page {
                    handle,
                    offset: 0,
                    limit: 1,
                },
                OperationControl::default(),
            ))
            .unwrap();
        assert_eq!(serde_json::to_value(page).unwrap()["rows"], json!([[1]]));
        session.close().unwrap();
    }

    #[test]
    fn session_domain_capture_keeps_committed_identity_after_post_flush_receipt_failure() {
        let fx = Fixture::new(&fixture::items());
        let mut session = Session::new(fx.context(), Default::default()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response = runtime
            .block_on(session.execute(
                Task::Navigate {
                    request: lycheedev_data::engine::DomainRequest::ItemGet {
                        item_id: 7,
                        mode: lycheedev_data::engine::SourceMode::Static,
                        hotfix_captures: vec![],
                    },
                },
                OperationControl::default(),
            ))
            .unwrap();
        receipt_failure_preserves_committed_capture(&fx.home, response);
        session.close().unwrap();
    }

    #[test]
    fn frame_size_is_bounded_and_eof_never_dispatches_partial_json() {
        let mut input = std::io::Cursor::new(b"{}\n{}");
        assert_eq!(read_line(&mut input).unwrap().unwrap(), b"{}\n");
        assert_eq!(
            read_line(&mut input).unwrap_err().code,
            "data.session.incomplete_frame"
        );
        let mut input = std::io::Cursor::new(vec![b'x'; MAX_LINE + 1]);
        assert_eq!(
            read_line(&mut input).unwrap_err().code,
            "data.session.frame_limit"
        );
    }
    #[test]
    fn close_cancels_busy_and_queued_requests_without_waiting_for_engine() {
        let token = TrackedCancellation::new();
        let state = Mutex::new(State {
            requests: BTreeMap::from([(1, token.clone())]),
            ..Default::default()
        });
        stop(&state, Some(2));
        assert!(token.is_cancelled());
        assert_eq!(lock(&state).close_id, Some(2));
    }
}
