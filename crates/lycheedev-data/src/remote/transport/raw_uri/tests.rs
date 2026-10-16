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
            uri.wire.path_and_query().unwrap().as_str(),
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

#[test]
fn go_authority_identities_keep_numeric_names_ports_case_and_ipv6_zone() {
    for host in [
        "127.1",
        "0177.0.0.1",
        "0x7f000001",
        "2130706433",
        "127.0.0.1.",
    ] {
        let locator = format!("http://{host}:80/a");
        let target = target(&locator).unwrap().unwrap();
        assert_eq!(target.wire.to_string(), locator);
        assert_eq!(target.dial_host, host);
        assert_eq!(target.tls_host, host);
        assert_eq!(
            target.dial.as_ref().unwrap().authority().unwrap().as_str(),
            format!("{host}:80")
        );
    }
    let target = Target::parse("https://LOCALHOST:00443/a").unwrap();
    assert_eq!(target.wire.authority().unwrap().as_str(), "LOCALHOST:00443");
    assert_eq!(target.dial_host, "LOCALHOST");
    assert_eq!(target.dial_port, "00443");
    let target = Target::parse("https://[fe80::1%25Ethernet]:443/a").unwrap();
    assert_eq!(target.wire.authority().unwrap().as_str(), "[fe80::1]:443");
    assert_eq!(target.dial_host, "fe80::1%Ethernet");
    assert_eq!(target.tls_host, "fe80::1%Ethernet");
    assert_eq!(target.route, "https://[fe80::1%Ethernet]:443/a");
    assert_eq!(
        target.dial.unwrap().authority().unwrap().as_str(),
        "[fe80::1%Ethernet]:443"
    );
}

#[test]
fn go_wire_raw_punycode_and_lookup_dial_are_distinct() {
    // Go idna.ToASCII (wire) preserves raw casing/mapping; Lookup (dial)
    // performs UTS46 mapping. Codec vectors are independently known Punycode.
    let target = Target::parse("http://BÜCHER.example/a").unwrap();
    assert_eq!(
        target.wire.authority().unwrap().as_str(),
        "xn--BCHER-2pa.example"
    );
    assert_eq!(target.dial_host, "xn--bcher-kva.example");
    assert_eq!(target.tls_host, "xn--bcher-kva.example");
    let target = Target::parse("http://b%C3%BCcher.example/a").unwrap();
    assert_eq!(
        target.wire.authority().unwrap().as_str(),
        "xn--bcher-kva.example"
    );
    assert_eq!(target.dial_host, "xn--bcher-kva.example");
    // Lookup rejects the leading hyphen; direct DNS still receives the exact
    // original name, never the different wire Punycode endpoint.
    let target = Target::parse("http://-bücher.example/a").unwrap();
    assert_eq!(target.dial_host, "-bücher.example");
    assert!(target.dial.is_none());
    assert!(target.wire.authority().unwrap().as_str().is_ascii());
}

#[test]
fn forward_proxy_sends_original_numeric_and_raw_idna_wire_authorities() {
    for (locator, authority) in [
        ("http://127.1:80/keep/%2e/target", "127.1:80"),
        ("http://0177.0.0.1/keep/%2e/target", "0177.0.0.1"),
        ("http://0x7f000001/keep/%2e/target", "0x7f000001"),
        ("http://2130706433/keep/%2e/target", "2130706433"),
        (
            "http://BÜCHER.example/keep/%2e/target",
            "xn--BCHER-2pa.example",
        ),
        ("http://[fe80::1%25Ethernet]/keep/%2e/target", "[fe80::1]"),
    ] {
        let (address, peer) = server(|mut stream| {
            let request = header(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            request
        });
        let (control, budget) = scope();
        let result: Result<Vec<u8>> = run(async {
            let prepared = Prepared::with_target(
                Target::parse(locator)?,
                HeaderMap::new(),
                Some(reqwest::Url::parse(&format!("http://{address}")).unwrap()),
            )?;
            let deadline = request_deadline(&control, Some(&budget))?;
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
        assert!(
            request.starts_with(&format!(
                "GET http://{authority}/keep/%2e/target HTTP/1.1\r\n"
            )),
            "{request}"
        );
        assert!(
            request.contains(&format!("host: {authority}\r\n")),
            "{request}"
        );
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    }
}

#[test]
fn raw_direct_resolver_receives_exact_name_not_whatwg_or_private_placeholder() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx")
            .unwrap();
        request
    });
    let port = address.rsplit(':').next().unwrap();
    // An admitted raw target can have a DNS spelling independent from wire
    // Host. This is the production ExactName+HttpConnector path, with a local
    // literal test endpoint; no environment or resolver process is changed.
    let mut target = Target::parse(&format!("http://ORIGINAL.test:{port}/a")).unwrap();
    target.dial_host = "127.0.0.1".into();
    let (control, budget) = scope();
    let result: Result<Vec<u8>> = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let prepared = Prepared::with_target(target, HeaderMap::new(), None)?;
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
    assert_eq!(result.unwrap(), b"x");
    assert!(request.contains(&format!("host: ORIGINAL.test:{port}\r\n")));
    assert!(!request.contains("lycheedev-dial.invalid"));
}

#[test]
fn invalid_explicit_port_is_admitted_but_never_dials_a_default_endpoint() {
    let (control, budget) = scope();
    let result: Result<reqwest::Response> = run(async {
        let prepared = Prepared::with_target(
            Target::parse("http://127.0.0.1:65536/a")?,
            HeaderMap::new(),
            None,
        )?;
        let deadline = request_deadline(&control, Some(&budget))?;
        attempt(Some(&budget))?;
        controlled(prepared.send(), &control, Some(&budget), deadline).await?
    });
    assert_eq!(result.unwrap_err().code, "records.remote_http");
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
}

#[test]
fn connect_uses_exact_padded_port_zone_target_and_zone_free_host() {
    for (locator, target, host) in [
        (
            "https://origin.invalid:00443/a",
            "origin.invalid:00443",
            "origin.invalid:00443",
        ),
        (
            "https://[fe80::1%25Ethernet]:00443/a",
            "[fe80::1%Ethernet]:00443",
            "[fe80::1]:00443",
        ),
        (
            "https://origin.invalid:65536/a",
            "origin.invalid:65536",
            "origin.invalid:65536",
        ),
    ] {
        let (address, peer) = server(|mut stream| {
            let request = header(&mut stream);
            stream
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            (request, closed(&mut stream))
        });
        let (control, budget) = scope();
        let result: Result<reqwest::Response> = run(async {
            let prepared = Prepared::with_target(
                Target::parse(locator)?,
                HeaderMap::new(),
                Some(reqwest::Url::parse(&format!("http://u:p@{address}")).unwrap()),
            )?;
            let deadline = request_deadline(&control, Some(&budget))?;
            attempt(Some(&budget))?;
            controlled(prepared.send(), &control, Some(&budget), deadline).await?
        });
        let (request, closed) = peer.join().unwrap().unwrap();
        assert!(closed);
        assert_eq!(result.unwrap_err().code, "records.remote_http");
        assert!(
            request.starts_with(&format!("CONNECT {target} HTTP/1.1\r\n")),
            "{request}"
        );
        assert!(request.contains(&format!("host: {host}\r\n")), "{request}");
        assert!(
            request.contains("proxy-authorization: Basic dTpw\r\n"),
            "{request}"
        );
        assert!(!request.contains("\r\nauthorization:"));
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    }
}

#[test]
fn tls_uses_admitted_verification_name_and_cancel_drops_direct_socket() {
    let (ready, received) = mpsc::channel();
    let (address, peer) = server(move |mut stream| {
        let mut prefix = [0; 5];
        stream.read_exact(&mut prefix).unwrap();
        assert_eq!(prefix[0], 22);
        let mut hello = vec![0; u16::from_be_bytes([prefix[3], prefix[4]]) as usize];
        stream.read_exact(&mut hello).unwrap();
        ready.send(()).unwrap();
        (hello, closed(&mut stream))
    });
    let port = address.rsplit(':').next().unwrap();
    let mut target = Target::parse(&format!("https://WIRE.invalid:{port}/a")).unwrap();
    target.dial_host = "127.0.0.1".into();
    target.tls_host = "verify.invalid".into();
    let (control, budget) = scope();
    let cancel = control.tracked_cancel_handle().unwrap();
    let stop = thread::spawn(move || {
        let ready = received.recv_timeout(Duration::from_secs(5)).is_ok();
        cancel.cancel();
        ready
    });
    let result: Result<reqwest::Response> = run(async {
        let prepared = Prepared::with_target(target, HeaderMap::new(), None)?;
        controlled(
            prepared.send(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await?
    });
    let ready = stop.join().unwrap();
    let (hello, closed) = peer.join().unwrap().unwrap();
    assert!(ready && closed);
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(
        hello
            .windows(b"verify.invalid".len())
            .any(|w| w == b"verify.invalid")
    );
    assert!(
        !hello
            .windows(b"WIRE.invalid".len())
            .any(|w| w == b"WIRE.invalid")
    );
    assert!(
        !hello
            .windows(b"lycheedev-dial.invalid".len())
            .any(|w| w == b"lycheedev-dial.invalid")
    );
}

#[test]
fn proxy_own_literal_port_and_raw_basic_use_the_original_endpoint() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx")
            .unwrap();
        request
    });
    let port = address.rsplit(':').next().unwrap();
    let proxy = crate::remote::proxy::ProxyEndpoint {
        scheme: "http".into(),
        authority: format!("u%FF:p%FE@127.0.0.1:0{port}"),
    };
    let (control, budget) = scope();
    let result: Result<Vec<u8>> = run(async {
        let prepared = Prepared::with_route(
            Target::parse("http://origin.invalid/a")?,
            HeaderMap::new(),
            Some(proxy),
        )?;
        let deadline = request_deadline(&control, Some(&budget))?;
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
    assert_eq!(result.unwrap(), b"x");
    assert!(request.starts_with("GET http://origin.invalid/a HTTP/1.1\r\n"));
    assert!(request.contains("proxy-authorization: Basic df86cP4=\r\n"));
    assert!(!request.contains("\r\nauthorization:"));
}

#[test]
fn socks_offers_go_methods_preserves_raw_credentials_and_unicode_fallback_target() {
    for selected in [0u8, 2] {
        let (address, peer) = server(move |mut stream| {
            let mut greeting = [0; 4];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 2, 0, 2]);
            stream.write_all(&[5, selected]).unwrap();
            if selected == 2 {
                let mut auth = [0; 7];
                stream.read_exact(&mut auth).unwrap();
                assert_eq!(auth, [1, 2, b'u', 255, 2, b'p', 254]);
                stream.write_all(&[1, 0]).unwrap();
            }
            let mut prefix = [0; 5];
            stream.read_exact(&mut prefix).unwrap();
            assert_eq!(&prefix[..4], &[5, 1, 0, 3]);
            let mut host = vec![0; usize::from(prefix[4])];
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
        let result: Result<reqwest::Response> = run(async {
            let proxy = crate::remote::proxy::ProxyEndpoint {
                scheme: "socks5h".into(),
                authority: format!("u%FF:p%FE@{address}"),
            };
            let prepared = Prepared::with_route(
                Target::parse("http://-bücher.example:8080/a")?,
                HeaderMap::new(),
                Some(proxy),
            )?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        let (host, port, request) = peer.join().unwrap().unwrap();
        assert_eq!(result.unwrap().status(), hyper::StatusCode::OK);
        assert_eq!(host, "-bücher.example".as_bytes());
        assert_eq!(u16::from_be_bytes(port), 8080);
        assert!(request.contains("host: xn---bcher-4ya.example:8080\r\n"));
        assert!(!request.contains("authorization:"));
    }
}

#[test]
fn socks_auth_validation_is_after_method_selection_and_cancel_owns_socket() {
    for selected in [0u8, 2] {
        let (ready, received) = mpsc::channel();
        let (address, peer) = server(move |mut stream| {
            let mut greeting = [0; 4];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 2, 0, 2]);
            stream.write_all(&[5, selected]).unwrap();
            if selected == 0 {
                let mut prefix = [0; 5];
                stream.read_exact(&mut prefix).unwrap();
                let mut rest = vec![0; usize::from(prefix[4]) + 2];
                stream.read_exact(&mut rest).unwrap();
                ready.send(()).unwrap();
            }
            closed(&mut stream)
        });
        let (control, budget) = scope();
        let cancel = control.tracked_cancel_handle().unwrap();
        let stop = thread::spawn(move || {
            if selected == 0 {
                let observed = received.recv_timeout(Duration::from_secs(5)).is_ok();
                cancel.cancel();
                observed
            } else {
                true
            }
        });
        let result: Result<reqwest::Response> = run(async {
            let proxy = crate::remote::proxy::ProxyEndpoint {
                scheme: "socks5h".into(),
                authority: format!(":p@{address}"),
            };
            let prepared = Prepared::with_route(
                Target::parse("http://origin.invalid/a")?,
                HeaderMap::new(),
                Some(proxy),
            )?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        assert!(stop.join().unwrap());
        assert!(peer.join().unwrap().unwrap());
        assert_eq!(
            result.unwrap_err().code,
            if selected == 0 {
                "command.cancelled"
            } else {
                "records.remote_http"
            }
        );
    }
}

#[test]
fn socks_rejects_malformed_reply_and_retains_original_request_budget() {
    for reply in [[4, 0, 0, 1], [5, 0, 1, 1], [5, 0, 0, 9], [5, 5, 0, 1]] {
        let (address, peer) = server(move |mut stream| {
            let mut greeting = [0; 3];
            stream.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            stream.write_all(&[5, 0]).unwrap();
            let mut prefix = [0; 5];
            stream.read_exact(&mut prefix).unwrap();
            let mut rest = vec![0; usize::from(prefix[4]) + 2];
            stream.read_exact(&mut rest).unwrap();
            stream.write_all(&reply).unwrap();
            closed(&mut stream)
        });
        let (control, budget) = scope();
        let result: Result<reqwest::Response> = run(async {
            let proxy = crate::remote::proxy::ProxyEndpoint {
                scheme: "socks5h".into(),
                authority: address,
            };
            let prepared = Prepared::with_route(
                Target::parse("http://origin.invalid/a")?,
                HeaderMap::new(),
                Some(proxy),
            )?;
            attempt(Some(&budget))?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        assert!(peer.join().unwrap().unwrap());
        assert_eq!(result.unwrap_err().code, "records.remote_http");
        let used = budget.snapshot().unwrap();
        assert_eq!((used.network_requests, used.network_bytes), (1, 0));
    }
}

#[test]
fn unicode_connect_fallback_preserves_target_host_headers_and_cancelled_owner() {
    for cancel_wait in [false, true] {
        let (ready, received) = mpsc::channel();
        let (address, peer) = server(move |mut stream| {
            let request = header(&mut stream);
            ready.send(()).unwrap();
            if !cancel_wait {
                stream
                    .write_all(
                        b"HTTP/1.1 407 Proxy Authentication Required\r\nContent-Length: 0\r\n\r\n",
                    )
                    .unwrap();
            }
            (request, closed(&mut stream))
        });
        let (control, budget) = scope();
        let cancel = control.tracked_cancel_handle().unwrap();
        let stop = thread::spawn(move || {
            let ready = received.recv_timeout(Duration::from_secs(5)).is_ok();
            if cancel_wait {
                cancel.cancel();
            }
            ready
        });
        let result: Result<reqwest::Response> = run(async {
            let proxy = crate::remote::proxy::ProxyEndpoint {
                scheme: "http".into(),
                authority: format!("u:p@{address}"),
            };
            let prepared = Prepared::with_route(
                Target::parse("https://-bücher.example:00443/a")?,
                HeaderMap::new(),
                Some(proxy),
            )?;
            attempt(Some(&budget))?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        assert!(stop.join().unwrap());
        let (request, closed) = peer.join().unwrap().unwrap();
        assert!(closed);
        assert!(request.starts_with("CONNECT -bücher.example:00443 HTTP/1.1\r\n"));
        assert!(request.contains("host: xn---bcher-4ya.example:00443\r\n"));
        assert!(request.contains("proxy-authorization: Basic dTpw\r\n"));
        assert!(!request.contains("lycheedev-connect.invalid"));
        assert_eq!(
            result.unwrap_err().code,
            if cancel_wait {
                "command.cancelled"
            } else {
                "records.remote_http"
            }
        );
        let used = budget.snapshot().unwrap();
        assert_eq!((used.network_requests, used.network_bytes), (1, 0));
    }
}

#[test]
fn forward_proxy_get_preserves_go_percent_host_and_exact_absolute_target() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc")
            .unwrap();
        request
    });
    let (control, budget) = scope();
    let result: Result<Vec<u8>> = run(async {
        let prepared = Prepared::with_route(
            Target::parse("http://name%25zone:0080/keep/%2e?q=%2f")?,
            HeaderMap::new(),
            Some(crate::remote::proxy::ProxyEndpoint {
                scheme: "http".into(),
                authority: address,
            }),
        )?;
        let deadline = request_deadline(&control, Some(&budget))?;
        attempt(Some(&budget))?;
        let response = controlled(prepared.send(), &control, Some(&budget), deadline).await??;
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
    assert!(request.starts_with("GET http://name%zone:0080/keep/%2e?q=%2f HTTP/1.1\r\n"));
    assert!(request.contains("host: name%zone:0080\r\n"));
    assert!(!request.contains("lycheedev-"));
    let used = budget.snapshot().unwrap();
    assert_eq!((used.network_requests, used.network_bytes), (1, 3));
}

#[test]
fn invalid_wire_host_fails_after_proxy_connect_without_sending_placeholder() {
    for host in [
        "bad<host",
        "bad>host",
        "bad\"host",
        "%FF",
        "%EF%BF%BD",
        "xn--abc-.é",
    ] {
        let (address, peer) = server(|mut stream| closed(&mut stream));
        let (control, budget) = scope();
        let result: Result<reqwest::Response> = run(async {
            let prepared = Prepared::with_route(
                Target::parse(&format!("http://{host}/a"))?,
                HeaderMap::new(),
                Some(crate::remote::proxy::ProxyEndpoint {
                    scheme: "http".into(),
                    authority: address,
                }),
            )?;
            attempt(Some(&budget))?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        assert!(peer.join().unwrap().unwrap(), "{host}");
        assert_eq!(result.unwrap_err().code, "records.remote_http");
        let used = budget.snapshot().unwrap();
        assert_eq!((used.network_requests, used.network_bytes), (1, 0));
    }
}

#[test]
fn direct_invalid_host_is_empty_but_valid_percent_host_is_preserved() {
    for (host, expected) in [
        ("bad<host", ""),
        ("bad>host", ""),
        ("bad\"host", ""),
        ("name%25zone", "name%zone"),
    ] {
        let (address, peer) = server(|mut stream| {
            let request = header(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            request
        });
        let (control, budget) = scope();
        let result: Result<reqwest::Response> = run(async {
            let mut target = Target::parse(&format!("http://{host}/raw/%2e?q=x"))?;
            // Override only the resolver fixture destination. Production still
            // uses the original Go canonical name, never the private URI host.
            target.dial_host = "127.0.0.1".into();
            target.dial_port = address.rsplit(':').next().unwrap().into();
            let prepared = Prepared::with_route(target, HeaderMap::new(), None)?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        let request = peer.join().unwrap().unwrap();
        assert_eq!(result.unwrap().status(), hyper::StatusCode::OK);
        assert!(request.starts_with("GET /raw/%2e?q=x HTTP/1.1\r\n"));
        assert!(request.contains(&format!("host: {expected}\r\n")));
        assert!(!request.contains("lycheedev-"));
    }
}

#[test]
fn non_utf8_socks_target_stays_raw_until_go_wire_validation() {
    let (address, peer) = server(|mut stream| {
        let mut greeting = [0; 3];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting, [5, 1, 0]);
        stream.write_all(&[5, 0]).unwrap();
        let mut request = [0; 9];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(request, [5, 1, 0, 3, 2, 0xe2, 0x82, 0, 80]);
        stream
            .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
            .unwrap();
        closed(&mut stream)
    });
    let (control, budget) = scope();
    let result: Result<reqwest::Response> = run(async {
        let target = Target::parse("http://%E2%82/a")?;
        assert_eq!(target.dial_host, "\u{fffd}\u{fffd}");
        assert_eq!(target.dial_bytes, [0xe2, 0x82]);
        let prepared = Prepared::with_route(
            target,
            HeaderMap::new(),
            Some(crate::remote::proxy::ProxyEndpoint {
                scheme: "socks5h".into(),
                authority: address,
            }),
        )?;
        attempt(Some(&budget))?;
        controlled(
            prepared.send(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await?
    });
    assert!(peer.join().unwrap().unwrap());
    assert_eq!(result.unwrap_err().code, "records.remote_http");
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
}

#[test]
fn parse_valid_hostless_requests_fail_inside_the_original_roundtrip_attempt() {
    for locator in [
        "http:/hostless",
        "http:///hostless",
        "http:bad%zz",
        "ftp://origin.invalid/a",
    ] {
        let (control, budget) = scope();
        let result: Result<reqwest::Response> = run(async {
            get_once(
                locator,
                policy(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await
        });
        assert!(result.is_err(), "{locator}");
        assert_eq!(budget.snapshot().unwrap().network_requests, 1, "{locator}");
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
    let (control, budget) = scope();
    let result: Result<reqwest::Response> = run(async {
        get_once(
            "http://bad host/a",
            policy(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await
    });
    assert!(result.is_err());
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
fn non_ascii_zone_keeps_go_wire_punycode_and_endpoint_separate() {
    // Fixed Go PunycodeHostPort processes the whole authority without a port,
    // but the split host with a port; removeZone follows that transformation.
    for (locator, host, forward) in [
        (
            "http://[fe80::1%25é]:80/a",
            "[xn--fe80::1]:80",
            "http://[xn--fe80::1]:80/a",
        ),
        (
            "http://[fe80::1%25é]/a",
            "xn--[fe80::1%]-j7a",
            "http://xn--[fe80::1%]-j7a/a",
        ),
        (
            "http://[fe80::1%25é]:/a",
            "xn--fe80::1%-i1a",
            "http://xn--fe80::1%-i1a/a",
        ),
    ] {
        let target = Target::parse(locator).unwrap();
        assert_eq!(target.host_header, host);
        assert_eq!(target.forward_target, forward);
        assert_eq!(target.dial_host, "fe80::1%é");
        assert_eq!(target.tls_host, "fe80::1%é");
        assert_eq!(target.connect_target, "[fe80::1%é]:80");
        assert_eq!(target.connect_host, "[xn--fe80::1]:80");
    }
}

#[test]
fn forward_proxy_handles_empty_origin_hostname_and_unicode_zone_without_placeholder() {
    for (locator, expected) in [
        ("http://:00080/a/%2e?q=x", "http://:00080/a/%2e?q=x"),
        ("http://[fe80::1%25é]/a", "http://xn--[fe80::1%]-j7a/a"),
    ] {
        let (address, peer) = server(|mut stream| {
            let request = header(&mut stream);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            request
        });
        let (control, budget) = scope();
        let result: Result<reqwest::Response> = run(async {
            let prepared = Prepared::with_route(
                Target::parse(locator)?,
                HeaderMap::new(),
                Some(crate::remote::proxy::ProxyEndpoint {
                    scheme: "http".into(),
                    authority: address,
                }),
            )?;
            attempt(Some(&budget))?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        let request = peer.join().unwrap().unwrap();
        assert_eq!(result.unwrap().status(), hyper::StatusCode::OK);
        assert!(
            request.starts_with(&format!("GET {expected} HTTP/1.1\r\n")),
            "{request}"
        );
        assert!(!request.contains("lycheedev-"));
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    }
}

#[test]
fn unknown_proxy_scheme_with_port_uses_plain_forward_and_without_port_dials_zero() {
    let (address, peer) = server(|mut stream| {
        let request = header(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        request
    });
    let (control, budget) = scope();
    let result: Result<reqwest::Response> = run(async {
        let prepared = Prepared::with_route(
            Target::parse("http://origin.invalid/a")?,
            HeaderMap::new(),
            Some(crate::remote::proxy::ProxyEndpoint {
                scheme: "unrecognized".into(),
                authority: address,
            }),
        )?;
        attempt(Some(&budget))?;
        controlled(
            prepared.send(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await?
    });
    let request = peer.join().unwrap().unwrap();
    result.unwrap();
    assert!(request.starts_with("GET http://origin.invalid/a HTTP/1.1\r\n"));
    let (control, budget) = scope();
    let result: Result<reqwest::Response> = run(async {
        let prepared = Prepared::with_route(
            Target::parse("http://origin.invalid/a")?,
            HeaderMap::new(),
            Some(crate::remote::proxy::ProxyEndpoint {
                scheme: "unrecognized".into(),
                authority: "127.0.0.1".into(),
            }),
        )?;
        attempt(Some(&budget))?;
        controlled(
            prepared.send(),
            &control,
            Some(&budget),
            request_deadline(&control, Some(&budget))?,
        )
        .await?
    });
    let failure = result.unwrap_err();
    assert_eq!(failure.code, "records.remote_http");
    assert!(!failure.message.contains("proxy configuration"));
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
}

#[test]
fn late_wire_validation_keeps_original_connection_failure_and_closes_successful_owner() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Owned(Arc<AtomicBool>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    struct Connector {
        fail: bool,
        dropped: Arc<AtomicBool>,
    }
    impl Service<Uri> for Connector {
        type Response = Owned;
        type Error = io::Error;
        type Future = std::future::Ready<io::Result<Owned>>;
        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, _: Uri) -> Self::Future {
            std::future::ready(if self.fail {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "original connect failure",
                ))
            } else {
                Ok(Owned(self.dropped.clone()))
            })
        }
    }
    for fail in [false, true] {
        let dropped = Arc::new(AtomicBool::new(false));
        let result: Result<()> = run(async {
            let mut connector = BeforeWrite {
                inner: Connector {
                    fail,
                    dropped: dropped.clone(),
                },
                error: Some("late invalid wire Host"),
            };
            connector
                .call("http://private.invalid/".parse().unwrap())
                .await
                .map(|_| ())
                .map_err(|e| http(e.to_string()))
        });
        assert_eq!(
            result.unwrap_err().message,
            if fail {
                "original connect failure"
            } else {
                "late invalid wire Host"
            }
        );
        assert_eq!(dropped.load(Ordering::SeqCst), !fail);
    }
}

// Read the bounded ClientHello fixture only. This is not production TLS parsing.
fn client_hello_sni(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut record = [0; 5];
    stream.read_exact(&mut record).unwrap();
    assert_eq!(record[0], 22);
    let mut bytes = vec![0; u16::from_be_bytes([record[3], record[4]]) as usize];
    stream.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes[0], 1);
    let mut at = 4 + 2 + 32;
    at += 1 + bytes[at] as usize;
    at += 2 + u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
    at += 1 + bytes[at] as usize;
    let end = at + 2 + u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
    at += 2;
    assert!(end <= bytes.len());
    while at + 4 <= end {
        let kind = u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        let length = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        at += 4;
        let data = &bytes[at..at + length];
        if kind == 0 {
            assert_eq!(data[2], 0);
            let n = u16::from_be_bytes([data[3], data[4]]) as usize;
            return Some(data[5..5 + n].to_vec());
        }
        at += length;
    }
    None
}

#[test]
fn single_dot_tls_uses_go_name_without_changing_wire_identity_and_cancels() {
    for name in ["example.test", "example.test."] {
        let (ready, received) = mpsc::channel();
        let (address, peer) = server(move |mut stream| {
            let name = client_hello_sni(&mut stream);
            ready.send(()).unwrap();
            (name, closed(&mut stream))
        });
        let (control, budget) = scope();
        let cancel = control.tracked_cancel_handle().unwrap();
        let stop = thread::spawn(move || {
            let ready = received.recv_timeout(Duration::from_secs(5)).is_ok();
            cancel.cancel();
            ready
        });
        let result: Result<reqwest::Response> = run(async {
            let mut target = Target::parse(&format!("https://{name}/a"))?;
            assert_eq!(target.host_header, name);
            target.dial_host = "127.0.0.1".into();
            target.dial_port = address.rsplit(':').next().unwrap().into();
            let prepared = Prepared::with_route(target, HeaderMap::new(), None)?;
            attempt(Some(&budget))?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        let ready = stop.join().unwrap();
        let (sni, closed) = peer.join().unwrap().unwrap();
        assert!(ready && closed);
        assert_eq!(sni.as_deref(), Some(b"example.test".as_slice()));
        assert_eq!(result.unwrap_err().code, "command.cancelled");
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    }
    // These prove our exact certificate-policy input, not Schannel's observed
    // multi-dot SNI or certificate acceptance. Those require Windows evidence.
    assert_eq!(tls_name("example.test.."), "example.test.");
    assert_eq!(tls_name("example.test..."), "example.test..");
}

#[test]
fn ip_tls_retains_go_zone_verification_input_and_omits_sni() {
    for tunneled in [false, true] {
        let (ready, received) = mpsc::channel();
        let (address, peer) = server(move |mut stream| {
            if tunneled {
                let request = header(&mut stream);
                assert!(request.starts_with("CONNECT [fe80::1%é]:443 HTTP/1.1\r\n"));
                stream
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .unwrap();
            }
            let name = client_hello_sni(&mut stream);
            ready.send(()).unwrap();
            (name, closed(&mut stream))
        });
        let (control, budget) = scope();
        let cancel = control.tracked_cancel_handle().unwrap();
        let stop = thread::spawn(move || {
            let ready = received.recv_timeout(Duration::from_secs(5)).is_ok();
            cancel.cancel();
            ready
        });
        let result: Result<reqwest::Response> = run(async {
            let mut target = Target::parse("https://[fe80::1%25é]/a")?;
            assert_eq!(target.tls_host, "fe80::1%é");
            let proxy = if tunneled {
                Some(crate::remote::proxy::ProxyEndpoint {
                    scheme: "http".into(),
                    authority: address.clone(),
                })
            } else {
                target.dial_host = "127.0.0.1".into();
                target.dial_port = address.rsplit(':').next().unwrap().into();
                None
            };
            let prepared = Prepared::with_route(target, HeaderMap::new(), proxy)?;
            attempt(Some(&budget))?;
            controlled(
                prepared.send(),
                &control,
                Some(&budget),
                request_deadline(&control, Some(&budget))?,
            )
            .await?
        });
        let ready = stop.join().unwrap();
        let (sni, closed) = peer.join().unwrap().unwrap();
        assert!(ready && closed);
        assert!(sni.is_none());
        assert_eq!(result.unwrap_err().code, "command.cancelled");
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    }
}
