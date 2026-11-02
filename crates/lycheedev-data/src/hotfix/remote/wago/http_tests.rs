//! Production adapter loopbacks only; no Wago/public endpoint, game, or environment mutation.
use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: thread::JoinHandle<()>,
}
impl Server {
    fn new(count: usize, action: impl Fn(usize, &mut TcpStream) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/start", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = thread::spawn(move || {
            for index in 0..count {
                let until = Instant::now() + Duration::from_secs(1);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < until =>
                        {
                            thread::sleep(Duration::from_millis(2));
                        }
                        _ => return,
                    }
                };
                // Windows can inherit nonblocking mode from the listener.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                action(index, &mut stream);
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }
    fn finish(self) -> Vec<String> {
        self.task.join().unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}
fn send(stream: &mut TcpStream, status: u16, headers: &str, bytes: &[u8]) {
    let _ = stream.write_all(
        format!("HTTP/1.1 {status} Test\r\nConnection: close\r\n{headers}\r\n").as_bytes(),
    );
    let _ = stream.write_all(bytes);
}
fn assert_waiting_for_body(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let result = stream.read(&mut [0]);
    assert!(
        matches!(result, Err(ref error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)),
        "consumer closed before completing the qualified redirect drain: {result:?}"
    );
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
}
fn socket_closed(stream: &mut TcpStream) -> bool {
    match stream.read(&mut [0]) {
        Ok(0) => true,
        Err(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
        ),
        _ => false,
    }
}
fn body(stream: &mut TcpStream, bytes: &[u8]) {
    send(
        stream,
        200,
        &format!("Content-Length: {}\r\n", bytes.len()),
        bytes,
    );
}
fn scope(limits: Cost) -> (OperationControl, Budget) {
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let budget = Budget::controlled(limits, control.deadline(), control.clone());
    (control, budget)
}
// with_budget_check preserves its first HTTP cause and appends the original
// invocation/Budget checks with the existing typed finish_owned envelope.
// Follow only that exact envelope: an arbitrary nested JSON `code` cannot
// satisfy a stop assertion, and every appended cause must deserialize intact.
fn primary_with_later_causes(failure: &ToolkitError) -> (ToolkitError, Vec<ToolkitError>) {
    let mut primary = failure.clone();
    let mut later = Vec::new();
    for depth in 0..=2 {
        if primary.details["schema"] != lycheedev_core::storage::FILE_CLEANUP_SCHEMA {
            return (primary, later);
        }
        assert!(
            depth < 2,
            "only the two post-request scope checks wrap here"
        );
        let fields = primary.details.as_object().unwrap();
        assert_eq!(fields.len(), 4);
        assert!(fields["control"].is_null());
        let previous: ToolkitError = serde_json::from_value(fields["primary"].clone()).unwrap();
        let appended: ToolkitError = serde_json::from_value(fields["cleanup"].clone()).unwrap();
        assert_eq!(primary.code, previous.code);
        assert_eq!(primary.exit_code, previous.exit_code);
        assert_eq!(
            primary.message,
            format!("{}\n{}", previous.message, appended.message)
        );
        later.insert(0, appended);
        primary = previous;
    }
    unreachable!()
}
fn assert_later_stops(causes: &[ToolkitError], code: &str) {
    for cause in causes {
        let stop = if cause.code == "records.hotfix_wago_http" {
            assert_eq!(cause.exit_code, 5);
            serde_json::from_value::<ToolkitError>(cause.details["originalStop"].clone()).unwrap()
        } else {
            cause.clone()
        };
        assert_eq!(stop.code, code);
        assert_eq!(
            stop.exit_code,
            if code == "command.cancelled" { 7 } else { 5 }
        );
    }
}
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn final_page_headers_body_and_work_use_public_adapter() {
    let server = Server::new(1, |_, stream| body(stream, b"<html>page</html>"));
    let (control, budget) = scope(Cost::defaults());
    assert_eq!(
        PublicFetcher.fetch(&server.url, &control, &budget).unwrap(),
        b"<html>page</html>"
    );
    let requests = server.finish();
    let request = requests[0].to_ascii_lowercase();
    assert!(request.starts_with("get /start http/1.1\r\n"));
    assert!(request.contains("accept: text/html,application/xhtml+xml\r\n"));
    assert!(request.contains("accept-encoding: gzip\r\n"));
    assert!(
        request.contains(
            "user-agent: Go-http-client/1.1\r\n"
                .to_ascii_lowercase()
                .as_str()
        )
    );
    assert!(!request.contains("range:") && !request.contains("referer:"));
    let used = budget.snapshot().unwrap();
    assert_eq!(used.network_requests, 0);
    assert_eq!(used.network_bytes, 0);
    assert_eq!(used.decode_work, 17);
    let logical = budget.logical_snapshot().unwrap();
    assert_eq!((logical.network_requests, logical.network_bytes), (0, 0));
    assert_eq!(logical.decode_work, 17);
}

#[test]
fn every_get_redirect_status_follows_relative_location_and_previous_referer() {
    for status in [301, 302, 303, 307, 308] {
        let server = Server::new(3, move |index, stream| match index {
            0 => send(
                stream,
                status,
                "Location: nested/one?x=1#part\r\nContent-Length: 1\r\n",
                b"a",
            ),
            1 => send(
                stream,
                status,
                "Location: ../two?x=2\r\nContent-Length: 1\r\n",
                b"b",
            ),
            _ => body(stream, b"ok"),
        });
        let original = server.url.clone();
        let origin = original.trim_end_matches("/start");
        let (control, budget) = scope(Cost::defaults());
        assert_eq!(
            PublicFetcher.fetch(&original, &control, &budget).unwrap(),
            b"ok"
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        for request in &requests {
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("user-agent: go-http-client/1.1\r\n")
            );
        }
        assert!(requests[1].starts_with("GET /nested/one?x=1 HTTP/1.1\r\n"));
        assert!(requests[1].contains(&format!("referer: {original}\r\n")));
        assert!(requests[2].starts_with("GET /two?x=2 HTTP/1.1\r\n"));
        assert!(requests[2].contains(&format!("referer: {origin}/nested/one?x=1#part\r\n")));
        let used = budget.snapshot().unwrap();
        assert_eq!(
            (used.network_requests, used.network_bytes, used.decode_work),
            (0, 0, 2)
        );
        let logical = budget.logical_snapshot().unwrap();
        assert_eq!((logical.network_requests, logical.network_bytes), (0, 0));
        assert_eq!(logical.decode_work, 2);
    }
}

#[test]
fn tenth_redirect_drains_then_stops_without_eleventh_request() {
    let server = Server::new(11, |_, stream| {
        send(
            stream,
            302,
            "Location: /again\r\nContent-Length: 1\r\n",
            b"x",
        )
    });
    let (control, budget) = scope(Cost::defaults());
    let failure = PublicFetcher
        .fetch(&server.url, &control, &budget)
        .unwrap_err();
    assert_eq!(failure.code, "records.hotfix_wago_http");
    assert!(failure.message.contains("stopped after 10 redirects"));
    assert_eq!(server.finish().len(), 10);
    let used = budget.snapshot().unwrap();
    assert_eq!(
        (used.network_requests, used.network_bytes, used.decode_work),
        (0, 0, 0)
    );
}

#[test]
fn missing_or_empty_location_and_non_200_status_do_not_read_body() {
    for (status, header) in [(302, ""), (302, "Location: \r\n"), (206, ""), (404, "")] {
        let server = Server::new(1, move |_, stream| {
            send(
                stream,
                status,
                &format!("{header}Content-Length: 4\r\n"),
                b"body",
            )
        });
        let (control, budget) = scope(Cost::defaults());
        let failure = PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err();
        assert_eq!(failure.code, "records.hotfix_wago_http");
        assert!(failure.message.ends_with(&format!("status {status}")));
        assert_eq!(server.finish().len(), 1);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}

#[test]
fn invalid_location_fails_before_redirect_drain() {
    let server = Server::new(1, |_, stream| {
        send(
            stream,
            302,
            "Location: /bad%zz\r\nContent-Length: 3\r\n",
            b"",
        );
        assert!(socket_closed(stream))
    });
    let (control, budget) = scope(Cost::defaults());
    assert!(
        PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err()
            .message
            .contains("invalid URL escape")
    );
    server.finish();
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}

#[test]
fn redirect_drain_uses_only_length_qualified_2048_prefix() {
    for length in [Some(2049), Some(2048), None] {
        let server = Server::new(2, move |index, stream| {
            if index == 0 {
                let declared = length
                    .map(|n| format!("Content-Length: {n}\r\n"))
                    .unwrap_or_default();
                send(stream, 302, &format!("Location: /final\r\n{declared}"), b"");
                if length != Some(2049) {
                    stream.write_all(&vec![b'x'; 2047]).unwrap();
                    assert_waiting_for_body(stream);
                    stream.write_all(b"x").unwrap();
                }
                // Known >2048 skips without waiting. Otherwise exactly2048 is
                // enough: even unknown-length bodies must not probe another byte.
                assert!(socket_closed(stream));
            } else {
                body(stream, b"ok");
            }
        });
        let (control, budget) = scope(Cost::defaults());
        assert_eq!(
            PublicFetcher.fetch(&server.url, &control, &budget).unwrap(),
            b"ok"
        );
        assert_eq!(server.finish().len(), 2);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}

#[test]
fn redirect_slurp_does_not_consume_a_shared_network_byte_budget() {
    let server = Server::new(2, |index, stream| {
        if index == 0 {
            send(
                stream,
                302,
                "Location: /empty\r\nContent-Length: 3\r\n",
                b"abc",
            );
        } else {
            body(stream, b"");
        }
    });
    let (control, budget) = scope(Cost {
        network_bytes: 0,
        network_requests: 0,
        ..Cost::defaults()
    });
    assert_eq!(
        PublicFetcher.fetch(&server.url, &control, &budget).unwrap(),
        b""
    );
    assert_eq!(server.finish().len(), 2);
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
    assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
}

#[test]
fn corrupt_redirect_gzip_is_ignored_for_follow_but_kept_if_later_failure() {
    for terminal in [200, 503] {
        let mut compressed = gzip(b"abc");
        let footer = compressed.len() - 8;
        compressed[footer] ^= 1;
        let server = Server::new(2, move |index, stream| {
            if index == 0 {
                send(
                    stream,
                    302,
                    &format!(
                        "Location: /end\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n",
                        compressed.len()
                    ),
                    &compressed,
                );
            } else {
                send(stream, terminal, "Content-Length: 2\r\n", b"ok");
            }
        });
        let (control, budget) = scope(Cost::defaults());
        let result = PublicFetcher.fetch(&server.url, &control, &budget);
        if terminal == 200 {
            assert_eq!(result.unwrap(), b"ok");
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.code, "records.hotfix_wago_http");
            assert_eq!(
                failure.details["redirectBodyErrors"][0]["message"],
                "gzip: invalid checksum"
            );
        }
        assert_eq!(server.finish().len(), 2);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}

#[test]
fn final_gzip_returns_decoded_bytes_and_maps_codec_errors() {
    for corrupt in [false, true] {
        let mut compressed = gzip(b"decoded");
        if corrupt {
            let footer = compressed.len() - 8;
            compressed[footer] ^= 1;
        }
        let server = Server::new(1, move |_, stream| {
            send(
                stream,
                200,
                &format!(
                    "Content-Encoding: GZip\r\nContent-Length: {}\r\n",
                    compressed.len()
                ),
                &compressed,
            )
        });
        let (control, budget) = scope(Cost::defaults());
        let result = PublicFetcher.fetch(&server.url, &control, &budget);
        if corrupt {
            assert_eq!(result.unwrap_err().code, "records.hotfix_wago_http");
        } else {
            assert_eq!(result.unwrap(), b"decoded");
        }
        server.finish();
        let used = budget.snapshot().unwrap();
        assert_eq!(used.network_bytes, 0);
        assert_eq!(used.decode_work, if corrupt { 0 } else { 7 });
    }
}

#[test]
fn short_body_with_huge_declared_length_is_http_error_after_actual_read() {
    let server = Server::new(1, |_, stream| {
        send(stream, 200, "Content-Length: 999999999\r\n", b"abc")
    });
    let (control, budget) = scope(Cost::defaults());
    assert_eq!(
        PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err()
            .code,
        "records.hotfix_wago_http"
    );
    server.finish();
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    assert_eq!(budget.snapshot().unwrap().decode_work, 0);
}

#[test]
fn exact_page_limit_succeeds_and_one_extra_byte_maps_to_hotfix_budget() {
    for size in [PAGE_BYTES, PAGE_BYTES + 1] {
        let server = Server::new(1, move |_, stream| body(stream, &vec![b'x'; size]));
        let (control, budget) = scope(Cost::defaults());
        let result = PublicFetcher.fetch(&server.url, &control, &budget);
        if size == PAGE_BYTES {
            assert_eq!(result.unwrap().len(), PAGE_BYTES);
        } else {
            let failure = result.unwrap_err();
            assert_eq!(failure.code, "records.hotfix_budget");
            assert!(
                failure
                    .message
                    .contains(&format!("page exceeds {PAGE_BYTES} bytes"))
            );
        }
        server.finish();
        let used = budget.snapshot().unwrap();
        assert_eq!(used.network_bytes, 0);
        assert_eq!(
            used.decode_work,
            if size == PAGE_BYTES {
                PAGE_BYTES as u64
            } else {
                0
            }
        );
    }
}

#[test]
fn final_work_admission_fails_without_publishing_a_page() {
    let server = Server::new(1, |_, stream| body(stream, b"abc"));
    let (control, budget) = scope(Cost {
        decode_work: 2,
        ..Cost::defaults()
    });
    assert_eq!(
        PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    server.finish();
    let used = budget.snapshot().unwrap();
    assert_eq!(
        (used.network_requests, used.network_bytes, used.decode_work),
        (0, 0, 0)
    );
}

#[test]
fn cancellation_during_stalled_headers_or_body_closes_owned_socket() {
    for prefix in [
        b"".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc",
    ] {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        let server = Server::new(1, move |_, stream| {
            stream.write_all(prefix).unwrap();
            ready_tx.send(()).unwrap();
            let closed = socket_closed(stream);
            closed_tx.send(closed).unwrap();
        });
        let (control, budget) = scope(Cost::defaults());
        let cancel = control.tracked_cancel_handle().unwrap();
        let stopper = thread::spawn(move || {
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            thread::sleep(Duration::from_millis(30));
            cancel.cancel();
        });
        let started = Instant::now();
        let failure = PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit_code),
            ("records.hotfix_wago_http", 5)
        );
        let (primary, later) = primary_with_later_causes(&failure);
        let stop: ToolkitError =
            serde_json::from_value(primary.details["originalStop"].clone()).unwrap();
        assert_eq!(
            (stop.code.as_str(), stop.exit_code),
            ("command.cancelled", 7)
        );
        assert_later_stops(&later, "command.cancelled");
        assert!(started.elapsed() < Duration::from_secs(1));
        stopper.join().unwrap();
        assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        server.finish();
        assert_eq!(budget.snapshot().unwrap().network_requests, 0);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}

#[test]
fn cancellation_during_redirect_drain_prevents_second_send() {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let server = Server::new(2, move |index, stream| {
        if index == 0 {
            send(
                stream,
                302,
                "Location: /later\r\nContent-Length: 10\r\n",
                b"abc",
            );
            ready_tx.send(()).unwrap();
            closed_tx.send(socket_closed(stream)).unwrap();
        } else {
            body(stream, b"ok");
        }
    });
    let (control, budget) = scope(Cost::defaults());
    let cancel = control.tracked_cancel_handle().unwrap();
    let stopper = thread::spawn(move || {
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(30));
        cancel.cancel();
    });
    let failure = PublicFetcher
        .fetch(&server.url, &control, &budget)
        .unwrap_err();
    assert_eq!(
        (failure.code.as_str(), failure.exit_code),
        ("records.hotfix_wago_http", 5)
    );
    let (primary, later) = primary_with_later_causes(&failure);
    let stop: ToolkitError =
        serde_json::from_value(primary.details["originalStop"].clone()).unwrap();
    assert_eq!(
        (stop.code.as_str(), stop.exit_code),
        ("command.cancelled", 7)
    );
    assert_later_stops(&later, "command.cancelled");
    stopper.join().unwrap();
    assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
    assert_eq!(server.finish().len(), 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}

#[test]
fn original_request_deadline_is_not_restarted_after_redirect() {
    let (closed_tx, closed_rx) = mpsc::channel();
    let server = Server::new(2, move |index, stream| {
        if index == 0 {
            thread::sleep(Duration::from_millis(40));
            send(
                stream,
                302,
                "Location: /stall\r\nContent-Length: 0\r\n",
                b"",
            );
        } else {
            closed_tx.send(socket_closed(stream)).unwrap();
        }
    });
    let (control, budget) = scope(Cost::defaults());
    let started = Instant::now();
    let failure = crate::remote::transport::run(fetch_http(
        &server.url,
        &control,
        &budget,
        started + Duration::from_millis(120),
    ))
    .map_err(http_error)
    .unwrap_err();
    assert_eq!(failure.code, "records.hotfix_wago_http");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn stop_before_fetch_charges_no_attempt() {
    let (control, budget) = scope(Cost::defaults());
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        PublicFetcher
            .fetch("http://127.0.0.1:1/", &control, &budget)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
#[ignore = "subprocess helper; invoked by the isolated CGI proxy regression"]
fn cgi_child_case() {
    assert_eq!(
        std::env::var("LYCHEE_WAGO_CGI_FIXTURE").expect("required CGI child fixture flag"),
        "1"
    );
    let (control, budget) = scope(Cost::defaults());
    let failure = PublicFetcher
        .fetch("http://127.0.0.1:1/", &control, &budget)
        .unwrap_err();
    assert_eq!(failure.code, "records.hotfix_wago_http");
    assert!(failure.message.contains("CGI"));
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
fn cgi_proxy_is_checked_before_loopback_and_no_proxy_in_isolated_process() {
    let test = format!(
        "{}::cgi_child_case",
        module_path!().split_once("::").unwrap().1
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--ignored", "--test-threads=1"])
        .env("LYCHEE_WAGO_CGI_FIXTURE", "1")
        .env("REQUEST_METHOD", "GET")
        // Windows environment keys are case-insensitive: remove the Unix alias
        // before assigning the effective value, never clear it afterwards.
        .env_remove("http_proxy")
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("bounded CGI child did not terminate");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reference_resolution_and_referer_keep_original_url_components() {
    let base = RedirectUrl::absolute("https://user:pass@EXAMPLE.test:443/a/b?old=1#old").unwrap();
    for (location, expected) in [
        ("../c", "https://user:pass@EXAMPLE.test:443/c"),
        ("?new=2", "https://user:pass@EXAMPLE.test:443/a/b?new=2"),
        ("?", "https://user:pass@EXAMPLE.test:443/a/b?"),
        ("#new", "https://user:pass@EXAMPLE.test:443/a/b?old=1#new"),
        ("#", "https://user:pass@EXAMPLE.test:443/a/b?old=1#old"),
        ("///c", "https://user:pass@EXAMPLE.test:443///c"),
        ("//Else.test/c", "https://Else.test/c"),
        ("/a\\b", "https://user:pass@EXAMPLE.test:443/a%5Cb"),
        ("/a%2Fb/c", "https://user:pass@EXAMPLE.test:443/a%2Fb/c"),
        ("/a%2Fb/../c", "https://user:pass@EXAMPLE.test:443/c"),
        ("/a b", "https://user:pass@EXAMPLE.test:443/a%20b"),
        ("/a//b/./", "https://user:pass@EXAMPLE.test:443/a//b/"),
        ("HTTPS://Else.test/x", "https://Else.test/x"),
    ] {
        let next = base.resolve(location).unwrap();
        assert_eq!(next.text(true), expected, "{location}");
        assert!(next.wire().is_ok(), "{location}");
        assert_eq!(
            base.referer(&next).unwrap(),
            "https://EXAMPLE.test:443/a/b?old=1#old"
        );
    }
    let insecure = base.resolve("http://other.test/x").unwrap();
    assert_eq!(base.referer(&insecure), None);
}

#[test]
fn raw_path_is_reescaped_as_a_whole_only_when_wire_requires_it() {
    for (input, output) in [
        ("/%61%2fb", "/%61%2fb"),
        ("/%61%2fb x", "/a/b%20x"),
        ("/猫", "/%E7%8C%AB"),
        ("/a[0]", "/a[0]"),
    ] {
        assert_eq!(url_component(input, false).unwrap(), output);
    }
    assert!(url_component("/%", false).is_err());
    assert!(url_component("/%a", false).is_err());
}

#[test]
fn encoded_dot_redirect_sends_original_target_through_controlled_transport() {
    let server = Server::new(2, |index, stream| {
        if index == 0 {
            send(
                stream,
                302,
                "Location: /keep/%2e%2e/target\r\nContent-Length: 0\r\n",
                b"",
            );
        } else {
            body(stream, b"raw-target");
        }
    });
    let (control, budget) = scope(Cost::defaults());
    let fetched = PublicFetcher.fetch(&server.url, &control, &budget).unwrap();
    assert_eq!(fetched, b"raw-target");
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("GET /keep/%2e%2e/target HTTP/1.1\r\n"));
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
fn transport_error_mapping_preserves_resource_stop_and_secondary_details() {
    for code in [
        "records.remote_http",
        "records.metadata_limit",
        "command.cancelled",
        "command.deadline",
        "records.query_resource_budget",
        "records.internal",
    ] {
        let mut original = fault(code, 5, "original");
        original.details =
            json!({"observedReadBytes": 17, "secondary": {"message": "codec failure"}});
        let mapped = http_error(original.clone());
        assert_eq!(mapped.details, original.details);
        assert_eq!(
            mapped.code,
            match code {
                "records.remote_http" => "records.hotfix_wago_http",
                "records.metadata_limit" => "records.hotfix_budget",
                _ => code,
            }
        );
    }
}

#[test]
fn expired_original_request_deadline_cannot_charge_a_send_attempt() {
    let (control, budget) = scope(Cost::defaults());
    let deadline = Instant::now() - Duration::from_millis(1);
    let failure = crate::remote::transport::run(fetch_http(
        "http://127.0.0.1:1/",
        &control,
        &budget,
        deadline,
    ))
    .map_err(http_error)
    .unwrap_err();
    assert_eq!(failure.code, "records.hotfix_wago_http");
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
fn redirect_hops_are_not_successful_page_requests() {
    let server = Server::new(3, |index, stream| {
        if index < 2 {
            send(
                stream,
                302,
                "Location: /later\r\nContent-Length: 0\r\n",
                b"",
            );
        } else {
            body(stream, b"ok");
        }
    });
    let (control, budget) = scope(Cost {
        network_requests: 0,
        network_bytes: 0,
        ..Cost::defaults()
    });
    assert_eq!(
        PublicFetcher.fetch(&server.url, &control, &budget).unwrap(),
        b"ok"
    );
    assert_eq!(server.finish().len(), 3);
    assert_eq!(budget.snapshot().unwrap(), Cost::work(2));
    assert_eq!(budget.logical_snapshot().unwrap(), Cost::work(2));
}

#[test]
fn unsupported_redirect_scheme_is_rejected_after_qualified_drain() {
    let server = Server::new(1, |_, stream| {
        send(
            stream,
            302,
            "Location: ftp://example.invalid/next\r\nContent-Length: 3\r\n",
            b"abc",
        )
    });
    let (control, budget) = scope(Cost::defaults());
    assert_eq!(
        PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err()
            .code,
        "records.hotfix_wago_http"
    );
    assert_eq!(server.finish().len(), 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}

#[test]
fn invalid_authority_is_rejected_before_any_redirect_drain() {
    for location in [
        "http://%65xample.test/",
        "http://example%2Etest/",
        "http://%2fexample.test/",
        "http://user name@example.test/",
        "http://u[ser]@example.test/",
        "http://u猫@example.test/",
        "http://bad%zz@example.test/",
        "http://example.test:bad/",
        "http://example.test:1:2/",
        "http://[127.0.0.1]/",
    ] {
        assert!(RedirectUrl::parse(location).is_err(), "{location}");
        let server = Server::new(1, move |_, stream| {
            send(
                stream,
                302,
                &format!("Location: {location}\r\nContent-Length: 3\r\n"),
                b"",
            );
            assert!(socket_closed(stream));
        });
        let (control, budget) = scope(Cost::defaults());
        let failure = PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err();
        assert_eq!(failure.code, "records.hotfix_wago_http", "{location}");
        assert_eq!(server.finish().len(), 1);
        let used = budget.snapshot().unwrap();
        assert_eq!(
            (used.network_requests, used.network_bytes),
            (0, 0),
            "{location}"
        );
    }
}

#[test]
fn parse_valid_transport_invalid_urls_drain_before_failure() {
    for location in [
        "http://127.0.0.1:65536/",
        "http://127.0.0.1:99999999999999999999999/",
        "http:bad%zz",
        "http:/hostless",
    ] {
        let parsed = RedirectUrl::parse(location).unwrap();
        if location == "http:bad%zz" {
            assert_eq!(parsed.opaque.as_deref(), Some("bad%zz"));
            assert!(parsed.path.is_empty());
        }
        let server = Server::new(1, move |_, stream| {
            send(
                stream,
                302,
                &format!("Location: {location}\r\nContent-Length: 3\r\n"),
                b"",
            );
            assert_waiting_for_body(stream);
            stream.write_all(b"abc").unwrap();
        });
        let (control, budget) = scope(Cost::defaults());
        let failure = PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err();
        assert_eq!(failure.code, "records.hotfix_wago_http", "{location}");
        assert_eq!(server.finish().len(), 1);
        let used = budget.snapshot().unwrap();
        assert_eq!(
            (used.network_requests, used.network_bytes),
            (0, 0),
            "{location}"
        );
    }
}

#[test]
fn numeric_authorities_reach_transport_without_whatwg_rewriting() {
    // Do not perform DNS for these names in a test: OS getaddrinfo owns their
    // interpretation. The raw connector's local forward-proxy regression
    // independently inspects the actual request and preserves these names.
    for host in [
        "127.1",
        "0177.0.0.1",
        "0x7f000001",
        "2130706433",
        "127.0.0.1.",
        "example%25test",
    ] {
        let reference =
            RedirectUrl::absolute(&format!("http://{host}:80/keep/%2e/target")).unwrap();
        assert_eq!(
            reference.wire().unwrap(),
            format!("http://{host}:80/keep/%2e/target")
        );
    }
}

#[test]
fn authority_parse_preserves_port_stage_and_valid_userinfo() {
    for authority in [
        "user:pass@example.test:65536",
        "u%20ser:p%3Ass@example.test:443",
        "user:p@ss@example.test",
        "[::1]:65536",
        "[fe80::1%25eth0]:80",
        "example.test:",
    ] {
        assert!(validate_authority("http", authority).is_ok(), "{authority}");
    }
    for authority in [
        "%41.test",
        "user name@example.test",
        "[::1%25]:80",
        "[::1%25z%2Fone]:80",
        "[::1]:+80",
    ] {
        assert!(
            validate_authority("http", authority).is_err(),
            "{authority}"
        );
    }
    let normal = RedirectUrl::absolute("http://user:pass@EXAMPLE.test:80/a").unwrap();
    assert!(normal.wire().is_ok());
    let ipv6 = RedirectUrl::absolute("http://[0:0:0:0:0:0:0:1]:80/a").unwrap();
    assert!(ipv6.wire().is_ok());
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}
fn credentials(url: &str, userinfo: &str) -> String {
    url.replacen("http://", &format!("http://{userinfo}@"), 1)
}
fn server_authority(url: &str) -> &str {
    url.split_once("://")
        .unwrap()
        .1
        .split('/')
        .next()
        .unwrap()
        .rsplit('@')
        .next()
        .unwrap()
}

#[test]
fn basic_authorization_preserves_raw_bytes_and_explicit_empty_userinfo() {
    for (userinfo, expected) in [
        ("%FF:pass", "Basic /zpwYXNz"),
        ("user:%FF", "Basic dXNlcjr/"),
        ("", "Basic Og=="),
        (":", "Basic Og=="),
        ("%E7%8C%AB:%3A+", "Basic 54yrOjor"),
        ("a%3Ab:p%3Aq", "Basic YTpiOnA6cQ=="),
    ] {
        let server = Server::new(1, |_, stream| body(stream, b"ok"));
        let original = credentials(&server.url, userinfo);
        let parsed = RedirectUrl::absolute(&original).unwrap();
        assert_eq!(parsed.wire().unwrap(), server.url);
        assert_eq!(parsed.authorization().unwrap().as_deref(), Some(expected));
        let (control, budget) = scope(Cost::defaults());
        assert_eq!(
            PublicFetcher.fetch(&original, &control, &budget).unwrap(),
            b"ok"
        );
        let requests = server.finish();
        assert_eq!(
            header(&requests[0], "Authorization"),
            Some(expected),
            "{userinfo}"
        );
        assert!(requests[0].starts_with("GET /start HTTP/1.1\r\n"));
        assert_eq!(
            header(&requests[0], "Host"),
            Some(server_authority(&original))
        );
    }
}

#[test]
fn same_host_absolute_or_network_reference_clears_url_credentials() {
    for network_path in [false, true] {
        let target = Arc::new(Mutex::new(String::new()));
        let shared = target.clone();
        let server = Server::new(3, move |index, stream| match index {
            0 => send(
                stream,
                302,
                "Location: /relative\r\nContent-Length: 0\r\n",
                b"",
            ),
            1 => send(
                stream,
                302,
                &format!(
                    "Location: {}\r\nContent-Length: 0\r\n",
                    shared.lock().unwrap()
                ),
                b"",
            ),
            _ => body(stream, b"ok"),
        });
        let absolute = server.url.replace("/start", "/absolute");
        *target.lock().unwrap() = if network_path {
            absolute.trim_start_matches("http:").to_owned()
        } else {
            absolute
        };
        let original = credentials(&server.url, "user:%FF");
        let (control, budget) = scope(Cost::defaults());
        assert_eq!(
            PublicFetcher.fetch(&original, &control, &budget).unwrap(),
            b"ok"
        );
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            header(&requests[0], "Authorization"),
            Some("Basic dXNlcjr/")
        );
        assert_eq!(
            header(&requests[1], "Authorization"),
            Some("Basic dXNlcjr/")
        );
        assert_eq!(header(&requests[2], "Authorization"), None);
        for request in &requests[1..] {
            assert!(!header(request, "Referer").unwrap().contains('@'));
        }
    }
}

#[test]
fn cross_host_return_and_replacement_credentials_use_only_resolved_url() {
    let to_b = Arc::new(Mutex::new(String::new()));
    let to_a = Arc::new(Mutex::new(String::new()));
    let replacement = Arc::new(Mutex::new(String::new()));
    let a_target = to_b.clone();
    let a = Server::new(3, move |index, stream| match index {
        0 => send(
            stream,
            302,
            "Location: /relative\r\nContent-Length: 0\r\n",
            b"",
        ),
        1 => send(
            stream,
            302,
            &format!(
                "Location: {}\r\nContent-Length: 0\r\n",
                a_target.lock().unwrap()
            ),
            b"",
        ),
        _ => body(stream, b"ok"),
    });
    let return_target = to_a.clone();
    let replacement_target = replacement.clone();
    let b = Server::new(2, move |index, stream| {
        let target = if index == 0 {
            replacement_target.lock().unwrap().clone()
        } else {
            return_target.lock().unwrap().clone()
        };
        send(
            stream,
            302,
            &format!("Location: {target}\r\nContent-Length: 0\r\n"),
            b"",
        );
    });
    // Distinct host spellings as well as distinct ports; both routes are local.
    let b_url = b.url.replace("127.0.0.1", "localhost");
    *to_b.lock().unwrap() = b_url.clone();
    *replacement.lock().unwrap() = credentials(&b_url.replace("/start", "/replace"), "%FF:new");
    *to_a.lock().unwrap() = a.url.replace("/start", "/returned");
    let original = credentials(&a.url, "old:%FF");
    let (control, budget) = scope(Cost::defaults());
    assert_eq!(
        PublicFetcher.fetch(&original, &control, &budget).unwrap(),
        b"ok"
    );
    let a_requests = a.finish();
    let b_requests = b.finish();
    assert_eq!((a_requests.len(), b_requests.len()), (3, 2));
    assert_eq!(
        header(&a_requests[0], "Authorization"),
        Some("Basic b2xkOv8=")
    );
    assert_eq!(
        header(&a_requests[1], "Authorization"),
        Some("Basic b2xkOv8=")
    );
    assert_eq!(header(&a_requests[2], "Authorization"), None);
    assert_eq!(header(&b_requests[0], "Authorization"), None);
    assert_eq!(
        header(&b_requests[1], "Authorization"),
        Some("Basic /zpuZXc=")
    );
    for request in a_requests.iter().chain(&b_requests) {
        assert_eq!(header(request, "User-Agent"), Some("Go-http-client/1.1"));
        assert!(!header(request, "Referer").unwrap_or_default().contains('@'));
    }
}

#[test]
fn tenth_redirect_cancel_keeps_limit_primary_and_stop_secondary() {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let server = Server::new(11, move |index, stream| {
        if index < 9 {
            send(
                stream,
                302,
                "Location: /again\r\nContent-Length: 1\r\n",
                b"x",
            );
        } else {
            send(
                stream,
                302,
                "Location: /again\r\nContent-Length: 10\r\n",
                b"abc",
            );
            ready_tx.send(()).unwrap();
            closed_tx.send(socket_closed(stream)).unwrap();
        }
    });
    let (control, budget) = scope(Cost::defaults());
    let cancel = control.tracked_cancel_handle().unwrap();
    let stopper = thread::spawn(move || {
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        thread::sleep(Duration::from_millis(30));
        cancel.cancel();
    });
    let failure = PublicFetcher
        .fetch(&server.url, &control, &budget)
        .unwrap_err();
    assert_eq!(
        (failure.code.as_str(), failure.exit_code),
        ("records.hotfix_wago_http", 5)
    );
    assert!(failure.message.contains("stopped after 10 redirects"));
    let (primary, later) = primary_with_later_causes(&failure);
    let drains: Vec<ToolkitError> =
        serde_json::from_value(primary.details["redirectBodyErrors"].clone()).unwrap();
    assert_eq!(drains.len(), 1);
    assert_eq!(
        (drains[0].code.as_str(), drains[0].exit_code),
        ("command.cancelled", 7)
    );
    assert_later_stops(&later, "command.cancelled");
    stopper.join().unwrap();
    assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
    assert_eq!(server.finish().len(), 10);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}

#[test]
fn deadline_before_fetch_is_direct_but_waiting_headers_or_body_is_http() {
    let expired = OperationControl::uncancelable(Instant::now() - Duration::from_millis(1));
    let budget = Budget::controlled(Cost::defaults(), expired.deadline(), expired.clone());
    let error = PublicFetcher
        .fetch("http://127.0.0.1:1/", &expired, &budget)
        .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
    for prefix in [
        b"".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc",
    ] {
        let (closed_tx, closed_rx) = mpsc::channel();
        let server = Server::new(1, move |_, stream| {
            stream.write_all(prefix).unwrap();
            closed_tx.send(socket_closed(stream)).unwrap();
        });
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(1));
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let failure = PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit_code),
            ("records.hotfix_wago_http", 5)
        );
        // Control and reqwest deadlines both map to HTTP. If control wins its
        // exact typed cause remains available, without changing public priority.
        let (primary, later) = primary_with_later_causes(&failure);
        if !primary.details["originalStop"].is_null() {
            let stop: ToolkitError =
                serde_json::from_value(primary.details["originalStop"].clone()).unwrap();
            assert_eq!(
                (stop.code.as_str(), stop.exit_code),
                ("command.deadline", 5)
            );
        }
        assert_later_stops(&later, "command.deadline");
        assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        assert_eq!(server.finish().len(), 1);
    }
}

#[test]
fn in_flight_stop_preserves_secondary_diagnostics_without_replay() {
    for code in [
        "command.cancelled",
        "command.deadline",
        "operation.cancelled",
    ] {
        let mut stop = fault(code, 7, "stopped read");
        stop.details = json!({"observedReadBytes": 3, "secondary": {"code": "records.remote_http", "message": "short body"}});
        let mapped = inflight_error(stop.clone());
        assert_eq!(
            (mapped.code.as_str(), mapped.exit_code),
            ("records.hotfix_wago_http", 5)
        );
        assert_eq!(
            mapped.details["originalStop"],
            serde_json::to_value(&stop).unwrap()
        );
        for safety in ["records.query_resource_budget", "records.internal"] {
            stop.details["secondary"]["code"] = json!(safety);
            assert_eq!(
                inflight_error(stop.clone()).details["originalStop"]["details"],
                stop.details
            );
        }
    }
}

#[test]
fn deadline_during_ordinary_or_tenth_drain_has_public_priority() {
    for redirects in [1, 10] {
        let (closed_tx, closed_rx) = mpsc::channel();
        let server = Server::new(redirects + 1, move |index, stream| {
            if index + 1 < redirects {
                send(
                    stream,
                    302,
                    "Location: /again\r\nContent-Length: 0\r\n",
                    b"",
                );
            } else {
                send(
                    stream,
                    302,
                    "Location: /again\r\nContent-Length: 10\r\n",
                    b"abc",
                );
                closed_tx.send(socket_closed(stream)).unwrap();
            }
        });
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(1));
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let failure = PublicFetcher
            .fetch(&server.url, &control, &budget)
            .unwrap_err();
        assert_eq!(
            (failure.code.as_str(), failure.exit_code),
            ("records.hotfix_wago_http", 5)
        );
        assert_eq!(
            failure.message.contains("stopped after 10 redirects"),
            redirects == 10
        );
        assert!(closed_rx.recv_timeout(Duration::from_secs(2)).unwrap());
        assert_eq!(server.finish().len(), redirects);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
        assert_eq!(budget.logical_snapshot().unwrap().network_bytes, 0);
    }
}

#[test]
fn tenth_drain_limit_has_no_shared_network_quota_competitor() {
    let server = Server::new(11, |index, stream| {
        send(
            stream,
            302,
            &format!(
                "Location: /again\r\nContent-Length: {}\r\n",
                if index == 9 { 3 } else { 0 }
            ),
            if index == 9 { b"abc" } else { b"" },
        );
    });
    let (control, budget) = scope(Cost {
        network_bytes: 0,
        network_requests: 0,
        ..Cost::defaults()
    });
    let failure = PublicFetcher
        .fetch(&server.url, &control, &budget)
        .unwrap_err();
    assert_eq!(failure.code, "records.hotfix_wago_http");
    assert!(failure.message.contains("stopped after 10 redirects"));
    assert_eq!(server.finish().len(), 10);
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
    assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
}

#[test]
fn independent_budget_cancel_drops_header_and_body_requests_without_hop_accounting() {
    for body_started in [false, true] {
        let original = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
        let independent = OperationControl::default();
        let cancel = independent.tracked_cancel_handle().unwrap();
        let budget = Budget::controlled(Cost::defaults(), independent.deadline(), independent);
        let (tx, rx) = mpsc::channel();
        let server = Server::new(1, move |_, stream| {
            if body_started {
                send(stream, 200, "Content-Length: 100\r\n", b"abc");
            }
            cancel.cancel();
            let _ = tx.send(socket_closed(stream));
        });
        let failure = PublicFetcher
            .fetch(&server.url, &original, &budget)
            .unwrap_err();
        assert_eq!(failure.code, "command.cancelled");
        original.check().unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(3)).unwrap());
        assert_eq!(server.finish().len(), 1);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
        assert_eq!(budget.snapshot().unwrap().network_requests, 0);
    }
}

#[test]
fn typed_http_stop_and_redirect_limit_retain_all_post_request_scope_causes() {
    let (control, budget) = scope(Cost::defaults());
    control.tracked_cancel_handle().unwrap().cancel();
    let original = control.check().unwrap_err();
    let mapped = inflight_error(original.clone());
    let stopped: Result<()> = Err(mapped.clone());
    let failure = finish_stop(
        finish_stop(stopped, control.check().map_err(inflight_error)),
        budget.check(),
    )
    .unwrap_err();
    let (primary, later) = primary_with_later_causes(&failure);
    assert_eq!(
        serde_json::to_value(&primary).unwrap(),
        serde_json::to_value(&mapped).unwrap()
    );
    // The identical mapped invocation observation is deduplicated. The Budget
    // observation keeps its original typed stop without replacing the HTTP leaf.
    assert_eq!(later.len(), 1);
    assert_eq!(
        serde_json::to_value(&later[0]).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_later_stops(&later, "command.cancelled");

    let selected = error("wago_http", 5, "stopped after 10 redirects")
        .with_details(json!({"primaryDetails": null, "redirectBodyErrors": [original]}));
    let stopped: Result<()> = Err(selected.clone());
    let failure = finish_stop(
        finish_stop(stopped, control.check().map_err(inflight_error)),
        budget.check(),
    )
    .unwrap_err();
    let (primary, later) = primary_with_later_causes(&failure);
    assert_eq!(
        serde_json::to_value(&primary).unwrap(),
        serde_json::to_value(&selected).unwrap()
    );
    assert_eq!(later.len(), 2);
    assert_eq!(
        serde_json::to_value(&later[0]).unwrap(),
        serde_json::to_value(&mapped).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&later[1]).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_later_stops(&later, "command.cancelled");
}
