use super::*;
use lycheedev_core::control::TrackedCancellation;
use serde::Deserialize;
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    name: String,
    fragment: usize,
    input: Vec<u8>,
    maximum: usize,
    cancel_after_raw_read: bool,
    expected_kind: String,
    expected_hex: String,
}
fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("fixtures.json")).unwrap()
}
fn unhex(raw: &str) -> Vec<u8> {
    raw.as_bytes()
        .chunks_exact(2)
        .map(|s| u8::from_str_radix(std::str::from_utf8(s).unwrap(), 16).unwrap())
        .collect()
}
struct Scripted {
    input: Vec<u8>,
    position: usize,
    fragment: usize,
    cancel: Option<TrackedCancellation>,
    raw_bytes: Arc<AtomicUsize>,
    pending_tail: bool,
    competing_charge: Option<Arc<Budget>>,
    ready: Option<std::sync::mpsc::Sender<()>>,
}
impl AsyncRead for Scripted {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let n = (this.input.len() - this.position)
            .min(this.fragment)
            .min(out.remaining());
        if n == 0 && this.pending_tail {
            return Poll::Pending;
        }
        out.put_slice(&this.input[this.position..this.position + n]);
        this.position += n;
        if let Some(budget) = this.competing_charge.take() {
            budget
                .charge(Cost {
                    network_bytes: 1,
                    ..Cost::default()
                })
                .unwrap();
        }
        this.raw_bytes.fetch_add(n, Ordering::SeqCst);
        if let Some(ready) = this.ready.take() {
            let _ = ready.send(());
        }
        if let Some(cancel) = this.cancel.take() {
            cancel.cancel();
        }
        Poll::Ready(Ok(()))
    }
}
fn source(input: Vec<u8>, fragment: usize) -> Scripted {
    Scripted {
        input,
        position: 0,
        fragment,
        cancel: None,
        raw_bytes: Arc::new(AtomicUsize::new(0)),
        pending_tail: false,
        competing_charge: None,
        ready: None,
    }
}
fn kind(error: &ToolkitError) -> &str {
    match error.code.as_str() {
        "command.cancelled" => "cancelled",
        "records.metadata_limit" => "limit",
        _ => match error.message.as_str() {
            "gzip: invalid header" => "header",
            "gzip: invalid checksum" => "checksum",
            "unexpected EOF" => "unexpected_eof",
            message if message.contains("decompression error") => "deflate",
            _ => "other",
        },
    }
}
#[test]
fn fixed_wire_gzip_fixtures_cover_framing_members_errors_and_delivery_counts() {
    for fixture in fixtures() {
        for fragment in [fixture.fragment] {
            let control = OperationControl::default();
            let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
            let mut input = source(fixture.input.clone(), fragment);
            if fixture.cancel_after_raw_read {
                input.cancel = Some(control.tracked_cancel_handle().unwrap());
            }
            let result = run(collect_admitted(
                input,
                ReadLimit::Complete(fixture.maximum),
                &control,
                Some(&budget),
                control.deadline(),
                &mut |_| Ok(()),
            ));
            let expected = unhex(&fixture.expected_hex);
            assert_eq!(
                budget.snapshot().unwrap().network_bytes as usize,
                expected.len(),
                "{} fragment{}",
                fixture.name,
                fragment
            );
            if fixture.expected_kind == "ok" {
                assert_eq!(
                    result.unwrap(),
                    expected,
                    "{} fragment{}",
                    fixture.name,
                    fragment
                );
            } else {
                assert_eq!(
                    kind(&result.unwrap_err()),
                    fixture.expected_kind,
                    "{} fragment{}",
                    fixture.name,
                    fragment
                );
            }
        }
    }
}
#[test]
fn raw_prefetch_is_distinct_from_delivered_gzip_bytes_and_probe_is_bounded() {
    let fixture = fixtures()
        .into_iter()
        .find(|v| v.name == "single/fragment-8192")
        .unwrap();
    for allowed in [2, 3] {
        let control = OperationControl::default();
        let budget = Budget::controlled(
            Cost {
                network_bytes: allowed,
                ..Cost::defaults()
            },
            control.deadline(),
            control.clone(),
        );
        let input = source(fixture.input.clone(), 8192);
        let raw = input.raw_bytes.clone();
        let result = run(collect_admitted(
            input,
            ReadLimit::Complete(100),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ));
        assert!(raw.load(Ordering::SeqCst) > 3);
        if allowed == 3 {
            assert_eq!(result.unwrap(), b"abc");
            assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, "records.query_resource_budget");
            assert_eq!(error.details["observedReadBytes"], 3);
            assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
        }
    }
}
#[test]
fn gzip_footer_error_and_budget_failure_retain_both_causes_and_actual_n() {
    for name in [
        "bad-body-crc",
        "bad-size",
        "deflate-prefix-plus-invalid-block",
    ] {
        let fixture = fixtures()
            .into_iter()
            .find(|v| v.name == format!("{name}/fragment-8192"))
            .unwrap();
        let control = OperationControl::default();
        let budget = Arc::new(Budget::controlled(
            Cost {
                network_bytes: 3,
                ..Cost::defaults()
            },
            control.deadline(),
            control.clone(),
        ));
        let mut input = source(fixture.input, 8192);
        // Another reader charges after our remaining snapshot. Our decode still
        // reports both actual n and CRC/DEFLATE failure; admission stays atomic.
        input.competing_charge = Some(budget.clone());
        let error = run(collect_admitted(
            input,
            ReadLimit::Complete(100),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ))
        .unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget", "{name}");
        assert_eq!(
            error.details["primaryDetails"]["observedReadBytes"], 3,
            "{name}"
        );
        assert_eq!(
            error.details["secondary"]["code"], "records.remote_http",
            "{name}"
        );
        assert_eq!(budget.snapshot().unwrap().network_bytes, 1);
    }
}
#[test]
fn pending_footer_cancellation_preserves_previously_decoded_n_without_outer_select() {
    let mut input = fixtures()
        .into_iter()
        .find(|v| v.name == "single/fragment-8192")
        .unwrap()
        .input;
    input.pop();
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let cancellation = control.tracked_cancel_handle().unwrap();
    let (ready, received) = std::sync::mpsc::channel();
    let mut input = source(input, 8192);
    input.pending_tail = true;
    input.ready = Some(ready);
    let stop = std::thread::spawn(move || {
        let _ = received.recv_timeout(Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(20));
        cancellation.cancel();
    });
    let result = run(collect_admitted(
        input,
        ReadLimit::Complete(100),
        &control,
        Some(&budget),
        control.deadline(),
        &mut |_| Ok(()),
    ));
    stop.join().unwrap();
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
}
#[test]
fn gzip_request_uses_decoded_length_and_range_keeps_identity_rejection() {
    let fixture = fixtures()
        .into_iter()
        .find(|v| v.name == "multiple/fragment-8192")
        .unwrap();
    let control = OperationControl::default();
    let mut response = format!(
        "HTTP/1.1 200 Test\r\nContent-Encoding: GZip\r\nContent-Length: {}\r\n\r\n",
        fixture.input.len()
    )
    .into_bytes();
    response.extend(&fixture.input);
    let (url, server) = super::super::tests::serve(response);
    let result = fetch_metadata(&url, 6, &control, None);
    let request = server.join().unwrap();
    assert_eq!(result.unwrap(), b"abcdef");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("accept-encoding: gzip")
    );
    let (url, server) = super::super::tests::serve(b"HTTP/1.1 206 Test\r\nContent-Encoding: gzip\r\nContent-Range: bytes 0-2/3\r\nContent-Length: 3\r\n\r\nabc".to_vec());
    let result = fetch_range(
        &url,
        RangeRequest {
            offset: 0,
            length: 3,
            total: 3,
        },
        &control,
        None,
    );
    let request = server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "records.remote_range");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("accept-encoding: identity")
    );
}
#[test]
fn metadata_automatic_gzip_is_not_a_general_content_encoding_decoder() {
    for (headers, body, expected) in [
        (
            "Content-Encoding: gzip\r\nContent-Length: 0\r\n",
            &b""[..],
            &b""[..],
        ),
        (
            "Content-Encoding: gzip, br\r\nContent-Length: 3\r\n",
            &b"abc"[..],
            &b"abc"[..],
        ),
        (
            "Content-Encoding: br\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\n",
            &b"abc"[..],
            &b"abc"[..],
        ),
    ] {
        let mut response = format!("HTTP/1.1 200 Test\r\n{headers}\r\n").into_bytes();
        response.extend(body);
        let (url, server) = super::super::tests::serve(response);
        let result = fetch_metadata(&url, 10, &OperationControl::default(), None);
        server.join().unwrap();
        assert_eq!(result.unwrap(), expected);
    }
}

pub(in crate::remote::transport) fn stalled_gzip(
    prefix: Vec<u8>,
) -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<bool>,
) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/gzip", listener.local_addr().unwrap());
    let (ready, received) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return false,
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(25)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 && Instant::now() < deadline
        {
            match stream.read(&mut byte) {
                Ok(0) => return false,
                Ok(_) => request.push(byte[0]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return false,
            }
        }
        if !request.ends_with(b"\r\n\r\n") {
            return false;
        }
        if stream
            .write_all(
                b"HTTP/1.1 200 Test\r\nContent-Encoding: gzip\r\nContent-Length: 1000000\r\n\r\n",
            )
            .and_then(|_| stream.write_all(&prefix))
            .is_err()
        {
            return false;
        }
        let _ = ready.send(());
        while Instant::now() < deadline {
            match stream.read(&mut byte) {
                Ok(0) => return true,
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    return true;
                }
                Err(e)
                    if matches!(
                        e.kind(),
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
fn real_gzip_header_body_footer_stalls_stop_and_close_with_decoded_accounting() {
    let valid = fixtures()
        .into_iter()
        .find(|v| v.name == "single/fragment-8192")
        .unwrap()
        .input;
    let mut body_prefix = valid[..10].to_vec();
    body_prefix.extend_from_slice(&[0, 3, 0, 252, 255, b'a', b'b', b'c']);
    for (phase, prefix) in [
        valid[..5].to_vec(),
        body_prefix,
        valid[..valid.len() - 1].to_vec(),
    ]
    .into_iter()
    .enumerate()
    {
        for stop_kind in 0..3 {
            let (url, ready, server) = stalled_gzip(prefix.clone());
            let control = OperationControl::uncancelable(
                Instant::now() + Duration::from_secs(if stop_kind == 1 { 1 } else { 20 }),
            );
            let deadline = if stop_kind == 2 {
                Instant::now() + Duration::from_secs(1)
            } else {
                control.deadline()
            };
            let budget = Budget::controlled(Cost::defaults(), deadline, control.clone());
            let stop = if stop_kind == 0 {
                let cancel = control.tracked_cancel_handle().unwrap();
                Some(std::thread::spawn(move || {
                    let _ = ready.recv_timeout(Duration::from_secs(1));
                    std::thread::sleep(Duration::from_millis(20));
                    let at = Instant::now();
                    cancel.cancel();
                    at
                }))
            } else {
                None
            };
            let result = fetch_metadata(&url, 100, &control, Some(&budget));
            let finished = Instant::now();
            let stopped = stop.map(|s| s.join().unwrap()).unwrap_or(deadline);
            let closed = server.join().unwrap();
            assert_eq!(
                result.unwrap_err().code,
                match stop_kind {
                    0 => "command.cancelled",
                    1 => "command.deadline",
                    _ => "records.query_resource_budget",
                },
                "phase{phase} stop{stop_kind}"
            );
            assert!(finished.duration_since(stopped) < Duration::from_millis(500));
            assert!(closed, "phase{phase} stop{stop_kind}");
            assert_eq!(budget.snapshot().unwrap().network_requests, 1);
            assert_eq!(
                budget.snapshot().unwrap().network_bytes,
                if phase == 0 { 0 } else { 3 }
            );
        }
    }
}

#[test]
fn same_raw_read_cancellation_budget_and_checksum_errors_all_remain_visible() {
    let fixture = fixtures()
        .into_iter()
        .find(|v| v.name == "bad-body-crc/fragment-8192")
        .unwrap();
    let control = OperationControl::default();
    let budget = Arc::new(Budget::controlled(
        Cost {
            network_bytes: 3,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    ));
    let mut input = source(fixture.input, 8192);
    input.cancel = Some(control.tracked_cancel_handle().unwrap());
    input.competing_charge = Some(budget.clone());
    let error = run(collect_admitted(
        input,
        ReadLimit::Complete(100),
        &control,
        Some(&budget),
        control.deadline(),
        &mut |_| Ok(()),
    ))
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["secondary"]["code"],
        "records.query_resource_budget"
    );
    assert_eq!(
        error.details["secondary"]["details"]["primaryDetails"]["observedReadBytes"],
        3
    );
    assert_eq!(
        error.details["secondary"]["details"]["secondary"]["message"],
        "gzip: invalid checksum"
    );
    assert_eq!(budget.snapshot().unwrap().network_bytes, 1);
}
#[test]
fn repeated_empty_members_keep_original_budget_deadline_and_zero_decoded_count() {
    struct EmptyMembers {
        member: Vec<u8>,
        position: usize,
    }
    impl AsyncRead for EmptyMembers {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            output: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            while output.remaining() != 0 {
                output.put_slice(&this.member[this.position..this.position + 1]);
                this.position = (this.position + 1) % this.member.len();
            }
            Poll::Ready(Ok(()))
        }
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &[]).unwrap();
    let input = EmptyMembers {
        member: encoder.finish().unwrap(),
        position: 0,
    };
    let control = OperationControl::default();
    let budget = Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_millis(30),
        control.clone(),
    );
    let error = run(collect_admitted(
        input,
        ReadLimit::Complete(100),
        &control,
        Some(&budget),
        control.deadline(),
        &mut |_| Ok(()),
    ))
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(control.check().is_ok());
    assert!(Instant::now().duration_since(budget.deadline) < Duration::from_millis(500));
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}

#[test]
fn pending_next_member_header_keeps_first_member_delivery_on_cancel() {
    let fixture = fixtures()
        .into_iter()
        .find(|v| v.name == "single/fragment-8192")
        .unwrap();
    for suffix in [Vec::new(), vec![0x1f]] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let cancellation = control.tracked_cancel_handle().unwrap();
        let (ready, received) = std::sync::mpsc::channel();
        let mut input = source([fixture.input.clone(), suffix].concat(), 8192);
        input.pending_tail = true;
        input.ready = Some(ready);
        let stop = std::thread::spawn(move || {
            let _ = received.recv_timeout(Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(20));
            cancellation.cancel();
        });
        let result = run(collect_admitted(
            input,
            ReadLimit::Complete(100),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ));
        stop.join().unwrap();
        assert_eq!(result.unwrap_err().code, "command.cancelled");
        assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    }
}
#[test]
fn exact_budget_probe_retains_observed_n_with_decode_boundary_errors() {
    for name in ["invalid-block-at-exact-probe", "bad-crc-at-exact-probe"] {
        let fixture = fixtures()
            .into_iter()
            .find(|v| v.name == format!("{name}/fragment-8192"))
            .unwrap();
        let control = OperationControl::default();
        let budget = Budget::controlled(
            Cost {
                network_bytes: 2,
                ..Cost::defaults()
            },
            control.deadline(),
            control.clone(),
        );
        let result = run(collect_admitted(
            source(fixture.input, 8192),
            ReadLimit::Complete(100),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ));
        let error = result.unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget", "{name}");
        assert_eq!(
            error.details["primaryDetails"]["observedReadBytes"], 3,
            "{name}"
        );
        assert_eq!(
            error.details["secondary"]["code"], "records.remote_http",
            "{name}"
        );
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}

#[test]
fn pending_decoded_output_retains_tail_and_terminal_error() {
    let fixture = fixtures()
        .into_iter()
        .find(|v| v.name == "short-deflate-pending-empty-input-drain/fragment-8192")
        .unwrap();
    let control = OperationControl::default();
    let mut reader = Gzip {
        input: source(fixture.input, 8192),
        control: &control,
        budget: None,
        deadline: control.deadline(),
        state: State::Header,
        decoder: Decoder::default(),
        input_error: None,
        crc: Crc::new(),
        input_buffer: [0; 8192],
        start: 0,
        end: 0,
        eof: false,
    };
    let mut output = [0u8; 8192];
    run(async {
        let mut produced = 0;
        reader.next(&mut output, &mut produced).await?;
        assert_eq!(produced, 8192);
        assert_eq!(
            reader.start, reader.end,
            "first decode consumed all raw input"
        );
        let before = (reader.start, reader.end);
        produced = 0;
        assert_eq!(
            reader
                .next(&mut output, &mut produced)
                .await
                .unwrap_err()
                .message,
            "unexpected EOF"
        );
        assert!(produced > 0, "pending decoded output must not be lost");
        assert_eq!((reader.start, reader.end), before);
        assert_eq!(8192 + produced, unhex(&fixture.expected_hex).len());
        Ok(())
    })
    .unwrap();
}

#[test]
fn empty_stored_flush_returns_limit_without_waiting_for_pending_tail() {
    let raw = [0, 3, 0, 252, 255, b'a', b'b', b'c', 0, 0, 0, 255, 255];
    let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    gzip.extend_from_slice(&raw);
    for fragment in [1, 2, 7, 8192] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let mut input = source(gzip.clone(), fragment);
        input.pending_tail = true;
        let error = run(collect_admitted(
            input,
            ReadLimit::Complete(2),
            &control,
            Some(&budget),
            Instant::now() + Duration::from_millis(100),
            &mut |_| Ok(()),
        ))
        .unwrap_err();
        assert_eq!(error.code, "records.metadata_limit", "fragment {fragment}");
        assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    }
}
#[test]
fn marker_bytes_inside_stored_payload_do_not_publish_a_flush() {
    let raw = [0, 8, 0, 247, 255, b'a', b'b', b'c', 0, 0, 0, 255, 255];
    let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
    gzip.extend_from_slice(&raw);
    for fragment in [1, 2, 7, 8192] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let mut input = source(gzip.clone(), fragment);
        input.pending_tail = true;
        let error = run(collect_admitted(
            input,
            ReadLimit::Complete(7),
            &control,
            Some(&budget),
            Instant::now() + Duration::from_millis(30),
            &mut |_| Ok(()),
        ))
        .unwrap_err();
        assert_eq!(
            error.message, "remote request timeout",
            "fragment {fragment}"
        );
        assert_eq!(budget.snapshot().unwrap().network_bytes, 8);
    }
}
#[test]
fn exact_window_limit_precedes_bad_footer_even_when_prefetched() {
    let mut gzip = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255, 1, 0, 128, 255, 127];
    gzip.resize(15 + 32768, b'a');
    gzip.extend_from_slice(&[0; 8]);
    for fragment in [1, 2, 7, 8192] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let error = run(collect_admitted(
            source(gzip.clone(), fragment),
            ReadLimit::Complete(32767),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ))
        .unwrap_err();
        assert_eq!(error.code, "records.metadata_limit", "fragment {fragment}");
        assert_eq!(budget.snapshot().unwrap().network_bytes, 32768);
    }
}

#[test]
fn public_boundary_matrix_matches_source_derived_reference_expectations() {
    // Lossless storage only: verify the exact original Go-vector bytes before
    // parsing. Use the existing flate2 backend, not the raw decoder under test.
    fn load(encoded: &str, expected_sha256: &str, expected_bytes: usize) -> Vec<Fixture> {
        use base64::Engine as _;
        use sha2::Digest as _;
        use std::io::Read as _;
        let compressed = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .unwrap();
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(compressed.as_slice())
            .take((expected_bytes + 1) as u64)
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes.len(), expected_bytes);
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(&bytes)),
            expected_sha256
        );
        serde_json::from_slice(&bytes).unwrap()
    }
    let mut cases = load(
        include_str!("boundary-fixtures-1.json.gz.b64"),
        "6cd4150e39b1d6c7b91eefaf52b44daaa0911e8b2f51b5af324efee864db600d",
        2302162,
    );
    cases.extend(load(
        include_str!("boundary-fixtures-2.json.gz.b64"),
        "95cc9ef961c70c17bd4bc8154f578287642a86b20dab7c16b6af2da494f42464",
        2302276,
    ));
    for fixture in cases {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let result = run(collect_admitted(
            source(fixture.input, fixture.fragment),
            ReadLimit::Complete(fixture.maximum),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ));
        assert_eq!(
            budget.snapshot().unwrap().network_bytes as usize,
            unhex(&fixture.expected_hex).len(),
            "{}",
            fixture.name
        );
        assert_eq!(
            kind(&result.unwrap_err()),
            fixture.expected_kind,
            "{}",
            fixture.name
        );
    }
}
#[test]
fn valid_matches_cross_dictionary_wrap_after_partial_flush() {
    let plain: Vec<u8> = (0..140_000).map(|i| (i % 251) as u8).collect();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &plain[..13]).unwrap();
    std::io::Write::flush(&mut encoder).unwrap();
    std::io::Write::write_all(&mut encoder, &plain[13..]).unwrap();
    let encoded = encoder.finish().unwrap();
    for fragment in [1, 2, 7, 8192] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let result = run(collect_admitted(
            source(encoded.clone(), fragment),
            ReadLimit::Complete(plain.len()),
            &control,
            Some(&budget),
            control.deadline(),
            &mut |_| Ok(()),
        ))
        .unwrap();
        assert_eq!(result, plain, "fragment {fragment}");
        assert_eq!(
            budget.snapshot().unwrap().network_bytes as usize,
            plain.len()
        );
    }
}
