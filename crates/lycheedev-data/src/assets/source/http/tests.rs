use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};
struct Seen {
    url: String,
    referer: Option<String>,
    deadline: Instant,
}
struct Script {
    replies: RefCell<VecDeque<reqwest::Response>>,
    seen: RefCell<Vec<Seen>>,
}
impl Script {
    fn new(replies: Vec<reqwest::Response>) -> Self {
        Self {
            replies: RefCell::new(replies.into()),
            seen: RefCell::new(Vec::new()),
        }
    }
}
impl Sender for Script {
    async fn get(
        &self,
        url: &str,
        policy: GetPolicy<'_>,
        control: &OperationControl,
        deadline: Instant,
    ) -> Result<reqwest::Response> {
        control.check()?;
        assert!(matches!(policy.body, BodyPolicy::GoAutomaticGzip));
        assert!(matches!(policy.locator, LocatorPolicy::HttpsOnly));
        assert_eq!(policy.user_agent, Some("Go-http-client/1.1"));
        assert_eq!(policy.authorization, None);
        self.seen.borrow_mut().push(Seen {
            url: url.into(),
            referer: policy.referer.map(str::to_owned),
            deadline,
        });
        Ok(self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("only admitted requests"))
    }
}
// A received HTTP body carries the wire Content-Length as its size hint. A
// Vec-backed synthetic Response instead reports Vec::len(), even if its header
// says otherwise, so it cannot model a short declared-length response.
struct DeclaredBody {
    data: Option<hyper::body::Bytes>,
    remaining: Option<u64>,
    polls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl hyper::body::Body for DeclaredBody {
    type Data = hyper::body::Bytes;
    type Error = std::convert::Infallible;
    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<std::result::Result<hyper::body::Frame<Self::Data>, Self::Error>>>
    {
        let this = self.get_mut();
        this.polls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let data = this.data.take();
        if let (Some(remaining), Some(data)) = (&mut this.remaining, &data) {
            *remaining = remaining.saturating_sub(data.len() as u64);
        }
        std::task::Poll::Ready(data.map(|data| Ok(hyper::body::Frame::data(data))))
    }
    fn is_end_stream(&self) -> bool {
        self.data.is_none()
    }
    fn size_hint(&self) -> hyper::body::SizeHint {
        let mut hint = hyper::body::SizeHint::new();
        if let Some(length) = self.remaining {
            hint.set_exact(length);
        }
        hint
    }
}
fn response_with_polls(
    status: u16,
    location: Option<&str>,
    length: Option<usize>,
    gzip: bool,
    bytes: Vec<u8>,
) -> (
    reqwest::Response,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let mut response = ureq::http::Response::builder().status(status);
    if let Some(location) = location {
        response = response.header("Location", location);
    }
    if let Some(length) = length {
        response = response.header("Content-Length", length);
    }
    if gzip {
        response = response.header("Content-Encoding", "gzip");
    }
    let polls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let body = reqwest::Body::wrap(DeclaredBody {
        data: (!bytes.is_empty()).then(|| bytes.into()),
        remaining: length.map(|n| n as u64),
        polls: polls.clone(),
    });
    let response: reqwest::Response = response.body(body).unwrap().into();
    assert_eq!(response.content_length(), length.map(|n| n as u64));
    (response, polls)
}
fn response(
    status: u16,
    location: Option<&str>,
    length: Option<usize>,
    gzip: bool,
    bytes: Vec<u8>,
) -> reqwest::Response {
    response_with_polls(status, location, length, gzip, bytes).0
}
fn scope() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(300))
}
fn run(script: &Script, maximum: usize) -> Result<Vec<u8>> {
    fetch_with("https://origin.test/a/b", maximum, &scope(), script)
}
#[test]
fn listfile_redirects_cross_host_keep_one_120_second_deadline_and_exact_referer() {
    let script = Script::new(vec![
        response(
            302,
            Some("https://second.test/x"),
            Some(2),
            false,
            b"hi".to_vec(),
        ),
        response(307, Some("../done?q=1"), Some(0), false, vec![]),
        response(200, None, Some(3), false, b"abc".to_vec()),
    ]);
    let start = Instant::now();
    assert_eq!(run(&script, 100).unwrap(), b"abc");
    let seen = script.seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].url, "https://second.test/x");
    assert_eq!(seen[2].url, "https://second.test/done?q=1");
    assert_eq!(seen[1].referer.as_deref(), Some("https://origin.test/a/b"));
    assert_eq!(seen[2].referer.as_deref(), Some("https://second.test/x"));
    assert!(seen[0].deadline >= start + Duration::from_secs(119));
    assert!(seen[0].deadline <= start + Duration::from_secs(121));
    assert!(seen.iter().all(|s| s.deadline == seen[0].deadline));
}
#[test]
fn listfile_fifth_redirect_and_any_userinfo_or_http_are_rejected() {
    let script = Script::new(
        (0..5)
            .map(|_| response(302, Some("/again"), Some(0), false, vec![]))
            .collect(),
    );
    assert_eq!(run(&script, 9).unwrap_err().code, "records.listfile_query");
    assert_eq!(script.seen.borrow().len(), 5);
    for location in ["http://origin.test/x", "https://u:p@origin.test/x"] {
        let script = Script::new(vec![response(302, Some(location), Some(0), false, vec![])]);
        assert_eq!(run(&script, 9).unwrap_err().code, "records.listfile_query");
        assert_eq!(script.seen.borrow().len(), 1);
    }
}
#[test]
fn listfile_status_missing_location_empty_and_identity_declared_limit_are_distinct() {
    for (status, location, length, body, code) in [
        (
            404,
            None,
            Some(200),
            b"x".to_vec(),
            "records.listfile_unavailable",
        ),
        (302, None, Some(0), vec![], "records.listfile_unavailable"),
        (200, None, Some(0), vec![], "records.listfile_unavailable"),
        (
            200,
            None,
            Some(20),
            b"short".to_vec(),
            "records.listfile_limit",
        ),
        (200, None, None, vec![0; 20], "records.listfile_limit"),
    ] {
        let script = Script::new(vec![response(status, location, length, false, body)]);
        assert_eq!(run(&script, 10).unwrap_err().code, code);
    }
}
#[test]
fn listfile_declared_length_rejects_before_body_poll_but_unknown_length_reads() {
    let (response, polls) = response_with_polls(200, None, Some(20), false, b"short".to_vec());
    let script = Script::new(vec![response]);
    assert_eq!(run(&script, 10).unwrap_err().code, "records.listfile_limit");
    assert_eq!(polls.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(script.seen.borrow().len(), 1);
    let (response, polls) = response_with_polls(200, None, None, false, vec![0; 20]);
    let script = Script::new(vec![response]);
    assert_eq!(run(&script, 10).unwrap_err().code, "records.listfile_limit");
    assert!(polls.load(std::sync::atomic::Ordering::Relaxed) > 0);
    assert_eq!(script.seen.borrow().len(), 1);
}
fn compressed(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
#[test]
fn listfile_gzip_members_use_decoded_limit_not_compressed_content_length() {
    let mut body = compressed(b"a");
    body.extend(compressed(b"bc"));
    let n = body.len();
    assert!(n > 3);
    let script = Script::new(vec![response(200, None, Some(n), true, body)]);
    assert_eq!(run(&script, 3).unwrap(), b"abc");
    let body = compressed(&vec![b'x'; 100]);
    let script = Script::new(vec![response(200, None, Some(body.len()), true, body)]);
    assert_eq!(run(&script, 10).unwrap_err().code, "records.listfile_limit");
}
#[test]
fn listfile_redirect_drain_bad_gzip_is_ignored_only_for_compatible_follow() {
    let script = Script::new(vec![
        response(302, Some("/next"), None, true, b"bad".to_vec()),
        response(200, None, Some(1), false, b"x".to_vec()),
    ]);
    assert_eq!(run(&script, 3).unwrap(), b"x");
    let script = Script::new(vec![response(
        302,
        Some("http://no.test"),
        None,
        true,
        b"bad".to_vec(),
    )]);
    let error = run(&script, 3).unwrap_err();
    assert_eq!(error.code, "records.listfile_query");
    assert_eq!(
        error.details["redirectBodyErrors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
struct Cancel {
    calls: Cell<usize>,
}
impl Sender for Cancel {
    async fn get(
        &self,
        _: &str,
        _: GetPolicy<'_>,
        control: &OperationControl,
        _: Instant,
    ) -> Result<reqwest::Response> {
        self.calls.set(self.calls.get() + 1);
        control.tracked_cancel_handle().unwrap().cancel();
        Ok(response(
            302,
            Some("/next"),
            Some(2049),
            false,
            vec![0; 2049],
        ))
    }
}
#[test]
fn listfile_original_cancel_during_redirect_prevents_second_send_and_preserves_ordered_scope() {
    let control = scope();
    let sender = Cancel {
        calls: Cell::new(0),
    };
    let error = fetch_with("https://origin.test/a", 100, &control, &sender).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(sender.calls.get(), 1);
    assert!(control.stop_cause().is_ok());
}
#[test]
fn listfile_invalid_initial_locator_never_sends_and_original_expiry_wins() {
    let script = Script::new(vec![]);
    for url in [
        "http://origin.test",
        "https://u@origin.test",
        "relative",
        "https:opaque",
    ] {
        assert_eq!(
            fetch_with(url, 100, &scope(), &script).unwrap_err().code,
            "records.listfile_query"
        );
    }
    let control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        fetch_with("bad", 0, &control, &script).unwrap_err().code,
        "command.deadline"
    );
    assert!(script.seen.borrow().is_empty());
}

// Real shared send/read cancellation, routed only to a task-owned loopback
// socket while the production consumer still sees its original HTTPS locator.
#[test]
fn listfile_real_stalled_headers_and_body_cancel_drop_request_before_return() {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread,
    };
    struct Loopback {
        url: String,
        headers: mpsc::Sender<()>,
    }
    impl Sender for Loopback {
        async fn get(
            &self,
            _: &str,
            policy: GetPolicy<'_>,
            control: &OperationControl,
            deadline: Instant,
        ) -> Result<reqwest::Response> {
            let result = transport::get_once(&self.url, policy, control, None, deadline).await;
            if result.is_ok() {
                let _ = self.headers.send(());
            }
            result
        }
    }
    fn accept(listener: &TcpListener, end: Instant) -> std::io::Result<TcpStream> {
        loop {
            match listener.accept() {
                Ok((stream, _)) => return Ok(stream),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => return Err(e),
            }
        }
    }
    for body in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let control = scope();
        let cancel = control.tracked_cancel_handle().unwrap();
        let (tx, rx) = mpsc::channel();
        let peer = thread::spawn(move || -> std::io::Result<bool> {
            let end = Instant::now() + Duration::from_secs(5);
            let mut stream = accept(&listener, end)?;
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            let mut header = Vec::new();
            let mut bytes = [0; 1024];
            while !header.windows(4).any(|s| s == b"\r\n\r\n") && Instant::now() < end {
                match stream.read(&mut bytes) {
                    Ok(0) => return Ok(true),
                    Ok(n) => header.extend_from_slice(&bytes[..n]),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e) => return Err(e),
                }
            }
            if body {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc")?;
                let _ = rx.recv_timeout(Duration::from_secs(2));
            }
            cancel.cancel();
            while Instant::now() < end {
                match stream.read(&mut bytes) {
                    Ok(0) => return Ok(true),
                    Ok(_) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        return Ok(true);
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(false)
        });
        let sender = Loopback {
            url: format!("http://{address}/"),
            headers: tx,
        };
        let result = fetch_with("https://origin.test/listfile", 1024, &control, &sender);
        let closed = peer.join().unwrap().unwrap();
        assert_eq!(result.unwrap_err().code, "command.cancelled");
        assert!(closed, "owned request socket must close after cancellation");
    }
}
