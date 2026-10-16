use super::*;
use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
};

fn server<T: Send + 'static>(
    work: impl FnOnce(TcpStream) -> T + Send + 'static,
) -> (String, thread::JoinHandle<Option<T>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let task = thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    return Some(work(stream));
                }
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < until =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return None,
            }
        }
    });
    (address, task)
}
fn header(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 16384 {
        let mut byte = [0];
        if stream.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        bytes.push(byte[0]);
    }
    String::from_utf8(bytes).unwrap()
}
fn closed(stream: &mut TcpStream) -> bool {
    match stream.read(&mut [0]) {
        Ok(0) => true,
        Err(error) => matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
        ),
        _ => false,
    }
}
fn policy() -> GetPolicy<'static> {
    GetPolicy {
        locator: LocatorPolicy::HotfixRedirect,
        body: BodyPolicy::Identity,
        accept: None,
        referer: None,
        user_agent: Some("Go-http-client/1.1"),
        authorization: Some("Basic dTpw"),
    }
}
fn scope() -> (OperationControl, Budget) {
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(15));
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    (control, budget)
}
#[test]
fn raw_targets_preserve_encoded_dot_and_query_without_fragments() {
    for path in [
        "/a/%2e/b",
        "/a/%2E%2E/b?x=%2f&x=+",
        "/a/.%2e/b?",
        "/a/%2e./b#never-sent",
        "/a?q='raw'",
    ] {
        let full = format!("https://EXAMPLE.test{path}");
        let uri = target(&full).unwrap().unwrap();
        assert_eq!(
            uri.path_and_query().unwrap().as_str(),
            path.split('#').next().unwrap()
        );
    }
    assert!(
        target("https://example.test/a%2Fb?x=%2f")
            .unwrap()
            .is_none()
    );
    assert!(
        target("https://example.test?x=1#fragment")
            .unwrap()
            .is_none()
    );
}
#[test]
fn direct_raw_request_uses_original_target_and_existing_delivered_byte_accounting() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
            .unwrap();
        request
    });
    let (control, budget) = scope();
    let result = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let response = get_once(
            &format!("http://{address}/keep/%2e%2e/target?q=%2F#hidden"),
            policy(),
            &control,
            Some(&budget),
            deadline,
        )
        .await?;
        read_response(
            response,
            BodyPolicy::Identity,
            3,
            &control,
            Some(&budget),
            deadline,
        )
        .await
    });
    let request = peer.join().unwrap().unwrap();
    assert_eq!(result.unwrap(), b"abc");
    assert!(request.starts_with("GET /keep/%2e%2e/target?q=%2F HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: basic dtpw\r\n")
    );
    let used = budget.snapshot().unwrap();
    assert_eq!((used.network_requests, used.network_bytes), (1, 3));
}
#[test]
fn raw_range_never_accepts_whole_object_status() {
    for status in [200, 206] {
        let (address, peer) = server(move |mut stream| {
            let request = header(&mut stream);
            let _=stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Range: bytes 3-5/20\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc").as_bytes());
            request
        });
        let (control, budget) = scope();
        let result = fetch_range(
            &format!("http://{address}/x/%2e%2e/object"),
            RangeRequest {
                offset: 3,
                length: 3,
                total: 20,
            },
            &control,
            Some(&budget),
        );
        let request = peer.join().unwrap().unwrap();
        assert!(request.starts_with("GET /x/%2e%2e/object HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("accept-encoding: identity\r\n")
        );
        if status == 206 {
            assert_eq!(result.unwrap().bytes, b"abc");
        } else {
            assert_eq!(result.unwrap_err().code, "records.remote_range");
        }
        assert_eq!(
            budget.snapshot().unwrap().network_bytes,
            if status == 206 { 3 } else { 0 }
        );
    }
}
#[test]
fn http_forward_proxy_preserves_absolute_raw_target_and_proxy_only_auth() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        request
    });
    let (control, budget) = scope();
    let uri: Uri = "http://origin.invalid/keep/%2e%2e/target?q=1"
        .parse()
        .unwrap();
    let proxy = reqwest::Url::parse(&format!("http://u%3A:p%FF@{address}")).unwrap();
    let result = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let prepared = Prepared::with_proxy(uri, HeaderMap::new(), Some(proxy))?;
        attempt(Some(&budget))?;
        let response = controlled(prepared.send(), &control, Some(&budget), deadline).await??;
        read_response(
            response,
            BodyPolicy::Identity,
            1,
            &control,
            Some(&budget),
            deadline,
        )
        .await
    });
    let request = peer.join().unwrap().unwrap();
    result.unwrap();
    assert!(request.starts_with("GET http://origin.invalid/keep/%2e%2e/target?q=1 HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic dto6cp8=\r\n")
    );
    assert!(!request.to_ascii_lowercase().contains("\r\nauthorization:"));
}
#[test]
fn socks_proxy_resolves_original_domain_remotely_and_preserves_raw_target() {
    let (address, peer) = server(|mut stream| {
        let mut greeting = [0; 3];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 1, 0]);
        stream.write_all(&[5, 0]).unwrap();
        let mut prefix = [0; 5];
        stream.read_exact(&mut prefix).unwrap();
        assert_eq!(&prefix[..4], &[5, 1, 0, 3]);
        let mut host = vec![0; prefix[4] as usize];
        stream.read_exact(&mut host).unwrap();
        let mut port = [0; 2];
        stream.read_exact(&mut port).unwrap();
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
            .unwrap();
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        (host, port, request)
    });
    let (control, budget) = scope();
    let result = run(async {
        let prepared = Prepared::with_proxy(
            "http://origin.invalid:8080/a/%2e%2e/b".parse().unwrap(),
            HeaderMap::new(),
            Some(reqwest::Url::parse(&format!("socks5h://{address}")).unwrap()),
        )?;
        let response = controlled(
            prepared.send(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await??;
        assert_eq!(response.status().as_u16(), 200);
        Ok(())
    });
    let (host, port, request) = peer.join().unwrap().unwrap();
    result.unwrap();
    assert_eq!(host, b"origin.invalid");
    assert_eq!(u16::from_be_bytes(port), 8080);
    assert!(request.starts_with("GET /a/%2e%2e/b HTTP/1.1\r\n"));
}
#[test]
fn connect_proxy_keeps_origin_tls_and_cancellation_joins_socket_driver() {
    let (ready, received) = mpsc::channel();
    let (address, peer) = server(move |mut stream| {
        let connect = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .unwrap();
        let mut hello = [0; 5];
        stream.read_exact(&mut hello).unwrap();
        assert_eq!(hello[0], 22); // TLS handshake, never plaintext origin GET.
        ready.send(()).unwrap();
        // Drain the rest of ClientHello before waiting for closure.
        let mut rest = vec![0; u16::from_be_bytes([hello[3], hello[4]]) as usize];
        stream.read_exact(&mut rest).unwrap();
        (connect, closed(&mut stream))
    });
    let (control, budget) = scope();
    let cancel = control.tracked_cancel_handle().unwrap();
    let stop = thread::spawn(move || {
        let observed = received.recv_timeout(Duration::from_secs(5)).is_ok();
        cancel.cancel();
        observed
    });
    let result = run(async {
        let prepared = Prepared::with_proxy(
            "https://origin.invalid/a/%2e%2e/b".parse().unwrap(),
            HeaderMap::new(),
            Some(reqwest::Url::parse(&format!("http://u:p@{address}")).unwrap()),
        )?;
        controlled(
            prepared.send(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await?
    });
    let ready = stop.join().unwrap();
    let (request, closed) = peer.join().unwrap().unwrap();
    assert!(ready && closed);
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(request.starts_with("CONNECT origin.invalid:443 HTTP/1.1\r\n"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic dtpw\r\n")
    );
}
#[test]
fn raw_body_stop_preserves_delivered_bytes_and_closes_own_request() {
    for independent in [false, true] {
        let (ready, received) = mpsc::channel();
        let (address, peer) = server(move |mut stream| {
            let request = header(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc")
                .unwrap();
            ready.send(()).unwrap();
            (request, closed(&mut stream))
        });
        let (control, _) = scope();
        let original = if independent {
            OperationControl::default()
        } else {
            control.clone()
        };
        let budget = std::sync::Arc::new(Budget::controlled(
            Cost::defaults(),
            control.deadline(),
            original.clone(),
        ));
        let cancel = original.tracked_cancel_handle().unwrap();
        let observed = budget.clone();
        let stop = thread::spawn(move || {
            let ready = received.recv_timeout(Duration::from_secs(5)).is_ok();
            let until = Instant::now() + Duration::from_secs(5);
            while observed.snapshot().unwrap().network_bytes < 3 && Instant::now() < until {
                thread::sleep(Duration::from_millis(2));
            }
            let delivered = observed.snapshot().unwrap().network_bytes == 3;
            cancel.cancel();
            ready && delivered
        });
        let result = run(async {
            let deadline = request_deadline(&control, Some(&budget))?;
            let response = get_once(
                &format!("http://{address}/a/%2e%2e/b"),
                policy(),
                &control,
                Some(&budget),
                deadline,
            )
            .await?;
            read_response(
                response,
                BodyPolicy::Identity,
                10,
                &control,
                Some(&budget),
                deadline,
            )
            .await
        });
        assert!(stop.join().unwrap());
        let (_, closed) = peer.join().unwrap().unwrap();
        assert!(closed);
        assert!(result.is_err());
        if independent {
            control.check().unwrap();
        }
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    }
}

#[test]
fn raw_metadata_keeps_manual_gzip_and_charges_decoded_delivery() {
    let mut encoded = Vec::new();
    for text in [b"abc".as_slice(), b"def".as_slice()] {
        let mut member = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        member.write_all(text).unwrap();
        encoded.extend(member.finish().unwrap());
    }
    let (address, peer) = server(move |mut stream| {
        let request = header(&mut stream);
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",encoded.len()).as_bytes()).unwrap();
        stream.write_all(&encoded).unwrap();
        request
    });
    let (control, budget) = scope();
    let result = fetch_metadata(
        &format!("http://{address}/a/%2e%2e/b"),
        10,
        &control,
        Some(&budget),
    );
    let request = peer.join().unwrap().unwrap();
    assert_eq!(result.unwrap(), b"abcdef");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("accept-encoding: gzip\r\n")
    );
    assert_eq!(budget.snapshot().unwrap().network_bytes, 6);
}

#[test]
fn direct_and_https_proxy_handshakes_remain_tls_and_cancellable() {
    for proxy_tls in [false, true] {
        let (ready, received) = mpsc::channel();
        let (address, peer) = server(move |mut stream| {
            let mut hello = [0; 5];
            stream.read_exact(&mut hello).unwrap();
            assert_eq!(hello[0], 22);
            let mut body = vec![0; u16::from_be_bytes([hello[3], hello[4]]) as usize];
            stream.read_exact(&mut body).unwrap();
            ready.send(()).unwrap();
            closed(&mut stream)
        });
        let (control, budget) = scope();
        let cancel = control.tracked_cancel_handle().unwrap();
        let stop = thread::spawn(move || {
            let ready = received.recv_timeout(Duration::from_secs(5)).is_ok();
            cancel.cancel();
            ready
        });
        let result = run(async {
            let (origin, proxy) = if proxy_tls {
                (
                    "http://origin.invalid/a/%2e%2e/b".to_owned(),
                    Some(reqwest::Url::parse(&format!("https://{address}")).unwrap()),
                )
            } else {
                (format!("https://{address}/a/%2e%2e/b"), None)
            };
            let prepared = Prepared::with_proxy(origin.parse().unwrap(), HeaderMap::new(), proxy)?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        let ready = stop.join().unwrap();
        let closed = peer.join().unwrap().unwrap();
        assert!(ready && closed);
        assert_eq!(result.unwrap_err().code, "command.cancelled");
    }
}

#[test]
fn raw_header_wait_respects_independent_deadline_without_reset_or_second_attempt() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        (request, closed(&mut stream))
    });
    let (control, _) = scope();
    let budget = Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(2),
        control.clone(),
    );
    let result = run(async {
        get_once(
            &format!("http://{address}/a/%2e%2e/b"),
            policy(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await
    });
    let (request, closed) = peer.join().unwrap().unwrap();
    assert!(closed);
    assert!(request.starts_with("GET /a/%2e%2e/b "));
    assert_eq!(result.unwrap_err().code, "records.query_resource_budget");
    control.check().unwrap();
    let used = budget.snapshot().unwrap();
    assert_eq!((used.network_requests, used.network_bytes), (1, 0));
}
