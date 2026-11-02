use super::*;
use hyper_util::rt::TokioIo;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy)]
enum Step {
    Short(usize),
    Pending,
    Zero,
    Error,
}
#[derive(Default)]
struct State {
    bytes: Vec<u8>,
    steps: VecDeque<Step>,
    dropped: bool,
}
struct Mock(Arc<Mutex<State>>);
impl Drop for Mock {
    fn drop(&mut self) {
        self.0.lock().unwrap().dropped = true;
    }
}
impl tokio::io::AsyncRead for Mock {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl tokio::io::AsyncWrite for Mock {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.0.lock().unwrap();
        let n = match state.steps.pop_front() {
            Some(Step::Short(n)) => n.min(bytes.len()),
            Some(Step::Pending) => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Some(Step::Zero) => return Poll::Ready(Ok(0)),
            Some(Step::Error) => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            None => bytes.len(),
        };
        state.bytes.extend_from_slice(&bytes[..n]);
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
fn fixture(
    target: Option<&str>,
    steps: impl IntoIterator<Item = Step>,
) -> (ConnectTargetIo<TokioIo<Mock>>, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State {
        steps: steps.into_iter().collect(),
        ..Default::default()
    }));
    (
        ConnectTargetIo::new(TokioIo::new(Mock(state.clone())), target).unwrap(),
        state,
    )
}
fn write_all(io: &mut ConnectTargetIo<TokioIo<Mock>>, mut bytes: &[u8]) {
    let mut cx = Context::from_waker(std::task::Waker::noop());
    for _ in 0..1024 {
        if bytes.is_empty() {
            return;
        }
        match Pin::new(&mut *io).poll_write(&mut cx, bytes) {
            Poll::Ready(Ok(n)) => {
                assert!(n > 0 && n <= bytes.len());
                bytes = &bytes[n..];
            }
            Poll::Pending => (),
            Poll::Ready(Err(error)) => panic!("{error}"),
        }
    }
    panic!("bounded mock write did not finish");
}
fn flush(io: &mut ConnectTargetIo<TokioIo<Mock>>) -> io::Result<()> {
    let mut cx = Context::from_waker(std::task::Waker::noop());
    for _ in 0..1024 {
        if let Poll::Ready(result) = Pin::new(&mut *io).poll_flush(&mut cx) {
            return result;
        }
    }
    panic!("bounded mock flush did not finish");
}
#[test]
fn connect_projection_all_input_splits_preserve_headers_and_upgrade_bytes() {
    let suffix = b"host: xn---bcher-4ya.test:443\r\nx-marker: CONNECT lycheedev-connect.invalid:443 HTTP/1.1\r\n\r\n";
    for split in 0..=PREFIX.len() {
        let (mut io, state) = fixture(
            Some("-bücher.test:443"),
            [Step::Short(1), Step::Pending, Step::Short(2), Step::Pending],
        );
        write_all(&mut io, &PREFIX[..split]);
        write_all(&mut io, &PREFIX[split..]);
        write_all(&mut io, suffix);
        flush(&mut io).unwrap();
        let upgrade = b"\x16\x03\x01\0\x04abcd";
        write_all(&mut io, upgrade);
        let mut expected = "CONNECT -bücher.test:443 HTTP/1.1\r\n".as_bytes().to_vec();
        expected.extend_from_slice(suffix);
        expected.extend_from_slice(upgrade);
        assert_eq!(state.lock().unwrap().bytes, expected, "split={split}");
        drop(io);
        assert!(state.lock().unwrap().dropped);
    }
}
#[test]
fn connect_projection_counts_accepted_input_not_replacement_output() {
    let (mut io, state) = fixture(Some("短.test:443"), [Step::Short(1), Step::Pending]);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let request = [PREFIX, b"host: wire.test\r\n\r\n"].concat();
    assert!(
        matches!(Pin::new(&mut io).poll_write(&mut cx, &request), Poll::Ready(Ok(n)) if n == PREFIX.len())
    );
    assert!(state.lock().unwrap().bytes.is_empty());
    assert!(matches!(
        Pin::new(&mut io).poll_write(&mut cx, &request[PREFIX.len()..]),
        Poll::Pending
    ));
    assert_eq!(state.lock().unwrap().bytes, b"C");
    write_all(&mut io, &request[PREFIX.len()..]);
    flush(&mut io).unwrap();
    assert_eq!(
        state.lock().unwrap().bytes,
        "CONNECT 短.test:443 HTTP/1.1\r\nhost: wire.test\r\n\r\n".as_bytes()
    );
}
#[test]
fn connect_projection_zero_write_and_error_keep_consumption_boundary() {
    for (step, kind) in [
        (Step::Zero, io::ErrorKind::WriteZero),
        (Step::Error, io::ErrorKind::BrokenPipe),
    ] {
        let (mut io, state) = fixture(Some("-bücher.test:443"), [Step::Short(3), step]);
        write_all(&mut io, PREFIX);
        assert_eq!(flush(&mut io).unwrap_err().kind(), kind);
        assert_eq!(state.lock().unwrap().bytes, b"CON");
        drop(io);
        assert!(state.lock().unwrap().dropped);
    }
}
#[test]
fn connect_projection_refuses_controls_mismatch_and_incomplete_line() {
    for value in ["a\rb:443", "a\nb:443", "a\0b:443", "a\u{7f}b:443"] {
        let state = Arc::new(Mutex::new(State::default()));
        assert!(ConnectTargetIo::new(TokioIo::new(Mock(state.clone())), Some(value)).is_err());
        assert!(state.lock().unwrap().dropped);
    }
    let (mut io, state) = fixture(Some("-bücher.test:443"), []);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    assert!(matches!(
        Pin::new(&mut io).poll_write(&mut cx, b"GET / HTTP/1.1\r\n"),
        Poll::Ready(Err(_))
    ));
    assert!(state.lock().unwrap().bytes.is_empty());
    let (mut io, state) = fixture(Some("-bücher.test:443"), []);
    write_all(&mut io, &PREFIX[..5]);
    assert!(flush(&mut io).is_err());
    assert!(state.lock().unwrap().bytes.is_empty());
}
#[test]
fn ordinary_ascii_connect_is_unmodified_through_short_writes() {
    let (mut io, state) = fixture(None, [Step::Short(2), Step::Pending, Step::Short(1)]);
    let bytes = b"CONNECT example.test:00443 HTTP/1.1\r\nhost: example.test:00443\r\n\r\n";
    write_all(&mut io, bytes);
    flush(&mut io).unwrap();
    assert_eq!(state.lock().unwrap().bytes, bytes);
}
#[test]
fn cancelled_prefix_flush_drops_original_transport_without_retry() {
    struct Stalled {
        dropped: Arc<AtomicBool>,
    }
    impl Drop for Stalled {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    impl tokio::io::AsyncWrite for Stalled {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }
    for forward in [false, true] {
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        let result: Result<io::Result<()>> = run(async {
            let transport = TokioIo::new(Stalled {
                dropped: dropped.clone(),
            });
            let mut io = if forward {
                ConnectTargetIo::forward(transport, "http://name%zone/a")
            } else {
                ConnectTargetIo::new(transport, Some("-bücher.test:443"))
            }
            .unwrap();
            let prefix = if forward { FORWARD_PREFIX } else { PREFIX };
            let work = async move {
                std::future::poll_fn(|cx| Pin::new(&mut io).poll_write(cx, prefix)).await?;
                std::future::poll_fn(|cx| {
                    let result = Pin::new(&mut io).poll_flush(cx);
                    cancel.cancel();
                    result
                })
                .await
            };
            controlled(work, &control, None, control.deadline()).await
        });
        assert_eq!(result.unwrap_err().code, "command.cancelled");
        assert!(dropped.load(Ordering::SeqCst));
    }
}

#[test]
fn forward_projection_uses_only_fixed_get_prefix_at_every_input_boundary() {
    let raw = "http://name%zone:0080/keep/%2e?q=%2f";
    for split in 0..=FORWARD_PREFIX.len() {
        let state = Arc::new(Mutex::new(State {
            steps: [Step::Short(2), Step::Pending, Step::Short(1)]
                .into_iter()
                .collect(),
            ..Default::default()
        }));
        let mut io = ConnectTargetIo::forward(TokioIo::new(Mock(state.clone())), raw).unwrap();
        write_all(&mut io, &FORWARD_PREFIX[..split]);
        write_all(&mut io, &FORWARD_PREFIX[split..]);
        let headers=b"host: name%zone:0080\r\nx-literal: GET http://lycheedev-forward.invalid/ HTTP/1.1\r\n\r\n";
        write_all(&mut io, headers);
        flush(&mut io).unwrap();
        let mut expected = format!("GET {raw} HTTP/1.1\r\n").into_bytes();
        expected.extend_from_slice(headers);
        assert_eq!(state.lock().unwrap().bytes, expected, "{split}");
    }
}

#[test]
fn forward_projection_zero_error_and_incomplete_prefix_never_leak_placeholder() {
    for (step, kind) in [
        (Step::Zero, io::ErrorKind::WriteZero),
        (Step::Error, io::ErrorKind::BrokenPipe),
    ] {
        let state = Arc::new(Mutex::new(State {
            steps: [Step::Short(3), step].into_iter().collect(),
            ..Default::default()
        }));
        let mut io =
            ConnectTargetIo::forward(TokioIo::new(Mock(state.clone())), "http://name%zone/a")
                .unwrap();
        write_all(&mut io, FORWARD_PREFIX);
        assert_eq!(flush(&mut io).unwrap_err().kind(), kind);
        assert_eq!(state.lock().unwrap().bytes, b"GET");
        drop(io);
        assert!(state.lock().unwrap().dropped);
    }
    let state = Arc::new(Mutex::new(State::default()));
    let mut io =
        ConnectTargetIo::forward(TokioIo::new(Mock(state.clone())), "http://name%zone/a").unwrap();
    write_all(&mut io, &FORWARD_PREFIX[..5]);
    assert!(flush(&mut io).is_err());
    assert!(state.lock().unwrap().bytes.is_empty());
    for target in ["http://a/\r", "http://a/\n", "http://a/\0"] {
        assert!(ConnectTargetIo::forward(TokioIo::new(Mock(state.clone())), target).is_err());
    }
}
