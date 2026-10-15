use super::*;
use reqwest::header::HeaderValue;
use std::{
    io::{Cursor, Read, Write},
    net::TcpListener,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Instant,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

struct ReadyReader<R>(R);
impl<R: Read + Unpin> AsyncRead for ReadyReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.0.read(buffer.initialize_unfilled()) {
            Ok(n) => {
                buffer.advance(n);
                Poll::Ready(Ok(()))
            }
            Err(error) => Poll::Ready(Err(error)),
        }
    }
}
fn read_bounded(
    body: impl Read + Unpin,
    maximum: usize,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<Vec<u8>> {
    run(super::read_bounded(
        ReadyReader(body),
        maximum,
        control,
        budget,
        request_deadline(control, budget)?,
    ))
}

pub(super) fn serve(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/object", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(500);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return String::new(),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                break;
            }
            request.push(byte[0]);
        }
        // Rejecting headers may close the connection before the body is sent.
        let _ = stream.write_all(&response);
        String::from_utf8(request).unwrap()
    });
    (url, handle)
}
fn reply(status: u16, headers: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} Test\r\nConnection: close\r\n{headers}\r\n{body}").into_bytes()
}
fn request() -> RangeRequest {
    RangeRequest {
        offset: 3,
        length: 5,
        total: 20,
    }
}
fn headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("content-range", HeaderValue::from_static("bytes 3-7/20"));
    headers.insert("content-length", HeaderValue::from_static("5"));
    headers
}
#[test]
fn loopback_range_is_exact_and_network_ledger_counts_actual_bytes() {
    let (url, server) = serve(reply(
        206,
        "Content-Range: bytes 3-7/20\r\nContent-Length: 5\r\n",
        "fghij",
    ));
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = fetch_range(&url, request(), &control, Some(&budget));
    let observed = server.join().unwrap();
    let result = result.unwrap();
    assert_eq!(
        result,
        RangeReply {
            bytes: b"fghij".to_vec(),
            object_bytes: 20
        }
    );
    assert!(
        observed
            .to_ascii_lowercase()
            .contains("range: bytes=3-7\r\n")
    );
    assert!(
        observed
            .to_ascii_lowercase()
            .contains("accept-encoding: identity\r\n")
    );
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 5);
}
#[test]
fn loopback_statuses_do_not_read_full_objects_or_follow_redirects() {
    for (status, code) in [
        (200, "records.remote_range"),
        (302, "records.remote_range"),
        (404, "records.remote_object_missing"),
        (503, "records.remote_http"),
    ] {
        let (url, server) = serve(reply(
            status,
            "Content-Length: 1000000000\r\nLocation: https://must-not-follow.example.com/\r\n",
            "",
        ));
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let result = fetch_range(&url, request(), &control, Some(&budget));
        server.join().unwrap();
        assert_eq!(result.unwrap_err().code, code);
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}
#[test]
fn range_headers_require_unique_exact_uncompressed_response() {
    for (name, value) in [
        ("content-range", "bytes 2-7/20"),
        ("content-range", "bytes 3-8/20"),
        ("content-range", "bytes 3-7/21"),
        ("content-range", "bytes 3-7/*"),
        ("content-encoding", "gzip"),
        ("content-type", "Multipart/byteranges"),
        ("content-length", "6"),
        ("content-length", "+5"),
    ] {
        let mut h = headers();
        h.insert(name, HeaderValue::from_str(value).unwrap());
        assert_eq!(
            validate_headers(&h, request()).unwrap_err().code,
            "records.remote_range",
            "{name} {value}"
        );
    }
    for name in ["content-range", "content-encoding", "content-length"] {
        let mut h = headers();
        let value = if name == "content-range" {
            "bytes 3-7/20"
        } else if name == "content-length" {
            "5"
        } else {
            "identity"
        };
        h.append(name, HeaderValue::from_str(value).unwrap());
        h.append(name, HeaderValue::from_str(value).unwrap());
        assert!(validate_headers(&h, request()).is_err());
    }
    let mut h = headers();
    h.remove("content-range");
    assert!(validate_headers(&h, request()).is_err());
}
#[test]
fn unknown_total_is_learned_and_large_totals_are_rejected() {
    let unknown = RangeRequest {
        total: 0,
        ..request()
    };
    assert_eq!(validate_headers(&headers(), unknown).unwrap(), 20);
    let mut h = headers();
    h.insert(
        "content-range",
        HeaderValue::from_static("bytes 3-7/1125899906842624"),
    );
    assert_eq!(validate_headers(&h, unknown).unwrap(), MAX_OBJECT_BYTES);
    h.insert(
        "content-range",
        HeaderValue::from_static("bytes 3-7/1125899906842625"),
    );
    assert_eq!(
        validate_headers(&h, unknown).unwrap_err().code,
        "records.metadata_limit"
    );
    for values in [
        vec![],
        vec!["bytes 0-1/2", "bytes 0-1/2"],
        vec!["Bytes 0-1/2"],
        vec!["bytes -1-1/2"],
        vec!["bytes 1-0/2"],
        vec!["bytes 0-1/2/3"],
        vec!["bytes +0-1/2"],
        vec!["bytes 0-1/9223372036854775808"],
    ] {
        assert!(content_range(&values).is_err(), "{values:?}");
    }
}
#[test]
fn invalid_requests_and_cancellation_never_attempt_network() {
    let control = OperationControl::default();
    for req in [
        RangeRequest {
            length: 0,
            ..request()
        },
        RangeRequest {
            length: MAX_RANGE_BYTES + 1,
            ..request()
        },
        RangeRequest {
            offset: -1,
            ..request()
        },
        RangeRequest {
            total: -1,
            ..request()
        },
        RangeRequest {
            offset: i64::MAX,
            ..request()
        },
        RangeRequest {
            total: MAX_OBJECT_BYTES + 1,
            ..request()
        },
    ] {
        assert!(fetch_range("https://must-not-contact.example.com", req, &control, None).is_err());
    }
    let cancelled = OperationControl::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(true)),
    );
    assert_eq!(
        fetch_range("invalid URL", request(), &cancelled, None)
            .unwrap_err()
            .exit_code,
        7
    );
    let expired = OperationControl::uncancelable(Instant::now());
    assert_eq!(
        fetch_metadata("invalid URL", 10, &expired, None)
            .unwrap_err()
            .code,
        "command.deadline"
    );
}
#[test]
fn loopback_short_and_excess_bodies_are_never_successful_ranges() {
    for body in ["abc", "abcdef"] {
        let (url, server) = serve(reply(206, "Content-Range: bytes 3-7/20\r\n", body));
        let result = fetch_range(&url, request(), &OperationControl::default(), None);
        server.join().unwrap();
        assert_eq!(result.unwrap_err().code, "records.remote_range");
    }
}
#[test]
fn body_read_admission_allows_only_remaining_plus_one_probe() {
    let control = OperationControl::default();
    let budget = Budget::controlled(
        Cost {
            network_bytes: 3,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    );
    let mut body = Cursor::new(b"abcdefgh".to_vec());
    assert_eq!(
        read_bounded(&mut body, 8, &control, Some(&budget))
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(body.position(), 4);
    let budget = Budget::controlled(
        Cost {
            network_bytes: 3,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    );
    assert_eq!(
        read_bounded(&b"abc"[..], 3, &control, Some(&budget)).unwrap(),
        b"abc"
    );
    assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    let budget = Budget::controlled(
        Cost {
            network_bytes: u64::MAX,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    );
    assert_eq!(
        read_bounded(&b"abc"[..], 3, &control, Some(&budget)).unwrap(),
        b"abc"
    );
}
#[test]
fn loopback_metadata_is_bounded_and_failed_attempts_are_counted() {
    let (url, server) = serve(reply(503, "Content-Length: 100\r\n", ""));
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = fetch_metadata(&url, 10, &control, Some(&budget));
    server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "records.remote_http");
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    let (url, server) = serve(reply(200, "", "abcd"));
    let result = fetch_metadata(&url, 3, &control, None);
    server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "records.metadata_limit");
}
#[test]
fn availability_fallback_is_narrow() {
    for code in ["records.remote_http", "records.remote_object_missing"] {
        assert!(super::super::availability_failure(&fault(code, 3, code)));
    }
    for code in [
        "records.remote_range",
        "records.metadata_limit",
        "records.invalid_metadata",
        "container.integrity_mismatch",
        "operation.cancelled",
        "command.deadline",
        "records.query_resource_budget",
    ] {
        assert!(!super::super::availability_failure(&fault(code, 3, code)));
    }
}

#[test]
fn loopback_fixture_accept_terminates_when_request_fails_before_io() {
    let (url, server) = serve(reply(206, "", ""));
    let started = Instant::now();
    assert!(
        fetch_range(
            &url,
            RangeRequest {
                length: 0,
                ..request()
            },
            &OperationControl::default(),
            None
        )
        .is_err()
    );
    assert!(server.join().unwrap().is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn body_bytes_returned_with_cancellation_are_charged_and_stop_is_retained() {
    struct CancelRead {
        token: Arc<AtomicBool>,
    }
    impl Read for CancelRead {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            output[..3].copy_from_slice(b"abc");
            self.token.store(true, std::sync::atomic::Ordering::Release);
            Ok(3)
        }
    }
    for maximum in [2, 3] {
        let token = Arc::new(AtomicBool::new(false));
        let control =
            OperationControl::new(Instant::now() + Duration::from_secs(10), token.clone());
        let budget = Budget::controlled(
            Cost {
                network_bytes: maximum,
                ..Cost::defaults()
            },
            control.deadline(),
            control.clone(),
        );
        let error = read_bounded(CancelRead { token }, 10, &control, Some(&budget)).unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        if maximum == 3 {
            assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
        } else {
            assert_eq!(
                error.details["secondary"]["code"],
                "records.query_resource_budget"
            );
            assert_eq!(
                error.details["secondary"]["details"]["observedReadBytes"],
                3
            );
            assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
        }
    }
}

// A fixture always terminates, including a rejected request that never connects.
// A stalled peer must see EOF/reset after the owned runtime is shut down.
fn stalled_peer(
    body: bool,
) -> (
    String,
    std::sync::mpsc::Receiver<()>,
    thread::JoinHandle<bool>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/object", listener.local_addr().unwrap());
    let (ready, received) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return false,
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(25)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 && Instant::now() < deadline
        {
            match stream.read(&mut byte) {
                Ok(0) => return false,
                Ok(_) => request.push(byte[0]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return false,
            }
        }
        if !request.ends_with(b"\r\n\r\n") {
            return false;
        }
        if body && stream
            .write_all(
                b"HTTP/1.1 206 Test\r\nContent-Range: bytes 3-7/20\r\nContent-Length: 5\r\n\r\nabc",
            )
            .is_err()
        {
            return false;
        }
        let _ = ready.send(());
        while Instant::now() < deadline {
            match stream.read(&mut byte) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    return true;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return false,
            }
        }
        false
    });
    (url, received, server)
}
fn stalled_cancel(body: bool) {
    let (url, ready, server) = stalled_peer(body);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(20));
    let cancellation = control.tracked_cancel_handle().unwrap();
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        control.clone(),
    ));
    let observed = budget.clone();
    let stop = thread::spawn(move || {
        let _ = ready.recv_timeout(Duration::from_secs(1));
        let wait_until = Instant::now() + Duration::from_secs(1);
        while body && observed.snapshot().unwrap().network_bytes < 3 && Instant::now() < wait_until
        {
            thread::sleep(Duration::from_millis(5));
        }
        let at = Instant::now();
        cancellation.cancel();
        at
    });
    let result = fetch_range(&url, request(), &control, Some(&budget));
    let finished = Instant::now();
    let cancelled_at = stop.join().unwrap();
    let peer_closed = server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(finished.duration_since(cancelled_at) < Duration::from_millis(500));
    assert!(peer_closed, "cancelled HTTP request left its socket open");
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    assert_eq!(
        budget.snapshot().unwrap().network_bytes,
        if body { 3 } else { 0 }
    );
}
#[test]
fn loopback_cancel_stalled_headers_closes_owned_http_connection() {
    stalled_cancel(false);
}
#[test]
fn loopback_cancel_stalled_body_preserves_bytes_and_closes_connection() {
    stalled_cancel(true);
}
#[test]
fn loopback_deadline_stalled_headers_closes_owned_http_connection() {
    let (url, _ready, server) = stalled_peer(false);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(250));
    let result = fetch_range(&url, request(), &control, None);
    let peer_closed = server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "command.deadline");
    assert!(peer_closed);
}

#[test]
fn operation_runtime_drops_spawned_drivers_before_return_and_unwind() {
    struct DropProbe(Arc<AtomicBool>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    for panic in [false, true] {
        let dropped = Arc::new(AtomicBool::new(false));
        let probe = DropProbe(dropped.clone());
        let result = std::panic::catch_unwind(|| {
            run(async move {
                tokio::spawn(async move {
                    let _probe = probe;
                    std::future::pending::<()>().await;
                });
                if panic {
                    panic!("controlled runtime unwind fixture");
                }
                Ok(())
            })
        });
        assert_eq!(result.is_err(), panic);
        assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
    }
}
fn stalled_tls_peer() -> (
    String,
    std::sync::mpsc::Receiver<()>,
    thread::JoinHandle<bool>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("https://{}/object", listener.local_addr().unwrap());
    let (ready, received) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return false,
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(25)))
            .unwrap();
        let mut buffer = [0u8; 4096];
        let mut observed_hello = false;
        while Instant::now() < deadline {
            match stream.read(&mut buffer) {
                Ok(0) => return observed_hello,
                Ok(_) => {
                    if !observed_hello {
                        observed_hello = true;
                        let _ = ready.send(());
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    return observed_hello;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return false,
            }
        }
        false
    });
    (url, received, server)
}
#[test]
fn loopback_cancel_stalled_tls_handshake_closes_owned_connection() {
    let (url, received, server) = stalled_tls_peer();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(20));
    let cancellation = control.tracked_cancel_handle().unwrap();
    let stop = thread::spawn(move || {
        let _ = received.recv_timeout(Duration::from_secs(1));
        let at = Instant::now();
        cancellation.cancel();
        at
    });
    let result = fetch_range(&url, request(), &control, None);
    let finished = Instant::now();
    let at = stop.join().unwrap();
    let closed = server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(finished.duration_since(at) < Duration::from_millis(500));
    assert!(closed);
}
#[test]
fn body_read_large_frame_keeps_remaining_plus_one_and_exact_eof() {
    for (limit, expected) in [(3, false), (3, true)] {
        let control = OperationControl::default();
        let budget = Budget::controlled(
            Cost {
                network_bytes: limit,
                ..Cost::defaults()
            },
            control.deadline(),
            control.clone(),
        );
        let (url, server) = serve(reply(
            200,
            "",
            &"a".repeat(if expected { 3 } else { 65536 }),
        ));
        let result = fetch_metadata(&url, 65536, &control, Some(&budget));
        server.join().unwrap();
        assert_eq!(result.is_ok(), expected);
        if expected {
            assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, "records.query_resource_budget");
            assert_eq!(error.details["observedReadBytes"], 4);
        }
    }
}

#[test]
fn loopback_deadline_stalled_body_preserves_bytes_and_closes_connection() {
    let (url, _ready, server) = stalled_peer(true);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(1));
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = fetch_range(&url, request(), &control, Some(&budget));
    let finished = Instant::now();
    let closed = server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "command.deadline");
    assert!(finished.duration_since(control.deadline()) < Duration::from_millis(500));
    assert!(closed);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
}
#[test]
fn loopback_deadline_stalled_tls_closes_owned_connection() {
    let (url, _ready, server) = stalled_tls_peer();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(1));
    let result = fetch_range(&url, request(), &control, None);
    let finished = Instant::now();
    let closed = server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "command.deadline");
    assert!(finished.duration_since(control.deadline()) < Duration::from_millis(500));
    assert!(closed);
}
#[test]
fn independent_budget_deadline_stops_header_body_and_tls_without_resetting_control() {
    for phase in 0..3 {
        let (url, _ready, server) = if phase == 2 {
            stalled_tls_peer()
        } else {
            stalled_peer(phase == 1)
        };
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(20));
        let budget = Budget::controlled(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(1),
            control.clone(),
        );
        let result = fetch_range(&url, request(), &control, Some(&budget));
        let finished = Instant::now();
        let closed = server.join().unwrap();
        assert_eq!(result.unwrap_err().code, "records.query_resource_budget");
        assert!(control.check().is_ok());
        assert!(finished.duration_since(budget.deadline) < Duration::from_millis(500));
        assert!(closed);
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
        assert_eq!(
            budget.snapshot().unwrap().network_bytes,
            if phase == 1 { 3 } else { 0 }
        );
    }
}
