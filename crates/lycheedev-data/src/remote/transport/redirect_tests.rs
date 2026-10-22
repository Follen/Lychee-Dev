use super::*;
fn policy() -> GetPolicy<'static> {
    GetPolicy {
        locator: LocatorPolicy::HotfixRedirect,
        body: BodyPolicy::GoAutomaticGzip,
        accept: Some("text/html,application/xhtml+xml"),
        referer: None,
        user_agent: None,
        authorization: None,
    }
}
fn response(length: Option<usize>, body: &[u8], extra: &str) -> Vec<u8> {
    let length = length
        .map(|n| format!("Content-Length: {n}\r\n"))
        .unwrap_or_default();
    [
        format!("HTTP/1.1 302 Found\r\nConnection: close\r\n{length}{extra}\r\n").as_bytes(),
        body,
    ]
    .concat()
}
#[test]
fn redirect_drain_counts_only_prefix_without_a_limit_probe() {
    for (declared, size, expected) in [
        (Some(2049), 2049, 0),
        (Some(2048), 2048, 2048),
        (Some(3), 3, 3),
        (None, 4096, 2048),
    ] {
        let (url, server) = super::tests::serve(response(declared, &vec![b'x'; size], ""));
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let result = run(async {
            let deadline = request_deadline(&control, Some(&budget))?;
            let response = get_once(&url, policy(), &control, Some(&budget), deadline).await?;
            assert!(
                drain_redirect_response(
                    response,
                    BodyPolicy::GoAutomaticGzip,
                    &control,
                    Some(&budget),
                    deadline
                )
                .await?
                .is_none()
            );
            Ok(())
        });
        let request = server.join().unwrap();
        result.unwrap();
        assert!(
            request
                .to_ascii_lowercase()
                .contains("accept: text/html,application/xhtml+xml\r\n")
        );
        assert_eq!(budget.snapshot().unwrap().network_bytes, expected);
        assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    }
}
#[test]
fn gzip_redirect_drain_uses_decoded_prefix_and_retains_failure_causes() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("gzip/fixtures.json")).unwrap();
    for (name, allowance, expected, error) in [
        ("decoded-limit", 10000, 2048, false),
        ("bad-body-crc", 10000, 3, true),
        ("bad-body-crc", 2, 0, true),
    ] {
        let fixture = fixtures
            .iter()
            .find(|v| v["name"] == format!("{name}/fragment-8192"))
            .unwrap();
        let body: Vec<u8> = serde_json::from_value(fixture["input"].clone()).unwrap();
        let (url, server) = super::tests::serve(response(
            Some(body.len()),
            &body,
            "Content-Encoding: gzip\r\n",
        ));
        let control = OperationControl::default();
        let budget = Budget::controlled(
            Cost {
                network_bytes: allowance,
                ..Cost::defaults()
            },
            control.deadline(),
            control.clone(),
        );
        let result = run(async {
            let deadline = request_deadline(&control, Some(&budget))?;
            let response = get_once(&url, policy(), &control, Some(&budget), deadline).await?;
            drain_redirect_response(
                response,
                BodyPolicy::GoAutomaticGzip,
                &control,
                Some(&budget),
                deadline,
            )
            .await
        });
        server.join().unwrap();
        let fault = if allowance == 2 {
            Some(result.unwrap_err())
        } else {
            result.unwrap()
        };
        assert_eq!(fault.is_some(), error);
        if allowance == 2 {
            let fault = fault.unwrap();
            assert_eq!(fault.code, "records.query_resource_budget");
            assert_eq!(fault.details["primaryDetails"]["observedReadBytes"], 3);
            assert_eq!(
                fault.details["secondary"]["message"],
                "gzip: invalid checksum"
            );
        }
        assert_eq!(budget.snapshot().unwrap().network_bytes, expected);
    }
}
#[test]
fn explicit_http_hop_preserves_shared_deadline_attempts_and_headers() {
    let (target, second) =
        super::tests::serve(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc".to_vec());
    let (start, first) =
        super::tests::serve(response(Some(1), b"x", &format!("Location: {target}\r\n")));
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let response = get_once(&start, policy(), &control, Some(&budget), deadline).await?;
        assert_eq!(response.status().as_u16(), 302);
        let location = response.headers()["Location"].to_str().unwrap().to_owned();
        assert!(
            drain_redirect_response(
                response,
                BodyPolicy::GoAutomaticGzip,
                &control,
                Some(&budget),
                deadline
            )
            .await?
            .is_none()
        );
        let next = GetPolicy {
            referer: Some(&start),
            ..policy()
        };
        let response = get_once(&location, next, &control, Some(&budget), deadline).await?;
        read_response(
            response,
            BodyPolicy::GoAutomaticGzip,
            16 << 20,
            &control,
            Some(&budget),
            deadline,
        )
        .await
    });
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert_eq!(result.unwrap(), b"abc");
    assert!(!first.to_ascii_lowercase().contains("referer:"));
    assert!(second.contains(&format!("referer: {start}\r\n")));
    assert_eq!(budget.snapshot().unwrap().network_requests, 2);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 4);
}
#[test]
fn private_get_cannot_loosen_other_remote_locator_policies_or_replay_after_stop() {
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = run(get_once(
        "http://not-loopback.example/x",
        GetPolicy {
            locator: LocatorPolicy::HttpsOnly,
            ..policy()
        },
        &control,
        Some(&budget),
        control.deadline(),
    ));
    assert!(result.unwrap_err().message.contains("HTTPS"));
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
    control.tracked_cancel_handle().unwrap().cancel();
    let error = run(get_once(
        "http://127.0.0.1:1/x",
        policy(),
        &control,
        Some(&budget),
        control.deadline(),
    ))
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
fn redirect_drain_cancellation_closes_socket_and_prevents_next_attempt() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("gzip/fixtures.json")).unwrap();
    let fixture = fixtures
        .iter()
        .find(|v| v["name"] == "single/fragment-8192")
        .unwrap();
    let mut body: Vec<u8> = serde_json::from_value(fixture["input"].clone()).unwrap();
    body.pop();
    let (url, ready, server) = gzip::tests::stalled_gzip(body);
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let cancel = control.tracked_cancel_handle().unwrap();
    let stop = std::thread::spawn(move || {
        let _ = ready.recv_timeout(Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(20));
        cancel.cancel();
    });
    let result = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let response = get_once(&url, policy(), &control, Some(&budget), deadline).await?;
        let drained = drain_redirect_response(
            response,
            BodyPolicy::GoAutomaticGzip,
            &control,
            Some(&budget),
            deadline,
        )
        .await
        .unwrap_err();
        assert_eq!(drained.code, "command.cancelled");
        // Go ignores drain failure for redirect choice. The next send still
        // observes the same original cancellation and must not start a request.
        let error = get_once(
            "http://127.0.0.1:1/next",
            policy(),
            &control,
            Some(&budget),
            deadline,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        Ok(())
    });
    stop.join().unwrap();
    let closed = server.join().unwrap();
    result.unwrap();
    assert!(closed);
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
}
#[test]
fn wago_read_first_does_not_inherit_metadata_declared_length_preflight() {
    let oversized = (16 << 20) + 1;
    let (url, server) = super::tests::serve(
        format!("HTTP/1.1 200 OK\r\nContent-Length: {oversized}\r\n\r\nabc").into_bytes(),
    );
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let response = get_once(&url, policy(), &control, Some(&budget), deadline).await?;
        read_response(
            response,
            BodyPolicy::GoAutomaticGzip,
            16 << 20,
            &control,
            Some(&budget),
            deadline,
        )
        .await
    });
    server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "records.remote_http");
    assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    let (url, server) =
        super::tests::serve(b"HTTP/1.1 200 OK\r\nContent-Length: 4097\r\n\r\nabc".to_vec());
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let result = fetch_metadata(&url, 4096, &control, Some(&budget));
    server.join().unwrap();
    assert_eq!(result.unwrap_err().code, "records.metadata_limit");
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}
#[test]
fn redirect_drain_budget_overrun_is_terminal_before_a_later_small_success() {
    let (next, next_server) =
        super::tests::serve(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx".to_vec());
    let (first, first_server) =
        super::tests::serve(response(Some(3), b"abc", &format!("Location: {next}\r\n")));
    let control = OperationControl::default();
    let budget = Budget::controlled(
        Cost {
            network_bytes: 2,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    );
    let result = run(async {
        let deadline = request_deadline(&control, Some(&budget))?;
        let response = get_once(&first, policy(), &control, Some(&budget), deadline).await?;
        let _diagnostic = drain_redirect_response(
            response,
            BodyPolicy::GoAutomaticGzip,
            &control,
            Some(&budget),
            deadline,
        )
        .await?;
        let response = get_once(&next, policy(), &control, Some(&budget), deadline).await?;
        read_response(
            response,
            BodyPolicy::GoAutomaticGzip,
            10,
            &control,
            Some(&budget),
            deadline,
        )
        .await
    });
    first_server.join().unwrap();
    let next_request = next_server.join().unwrap();
    let error = result.unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(error.details["observedReadBytes"], 3);
    assert!(next_request.is_empty());
    assert_eq!(budget.snapshot().unwrap().network_requests, 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
}
#[test]
fn original_request_timeout_is_checked_before_accounting_another_hop() {
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let deadline = Instant::now() - Duration::from_millis(1);
    let error = run(get_once(
        "http://127.0.0.1:1/next",
        policy(),
        &control,
        Some(&budget),
        deadline,
    ))
    .unwrap_err();
    assert_eq!(error.message, "remote request timeout");
    assert_eq!(budget.snapshot().unwrap().network_requests, 0);
}

#[test]
fn fixed_consumer_headers_reject_injection_before_attempt() {
    for (user_agent, authorization) in [
        (Some("other"), None),
        (Some("Go-http-client/1.1\r\nX-Injected: yes"), None),
        (None, Some("Bearer abc")),
        (None, Some("Basic Og==\r\nX-Injected: yes")),
        (None, Some("Basic ")),
    ] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let error = run(async {
            let deadline = request_deadline(&control, Some(&budget))?;
            get_once(
                "http://127.0.0.1:1/",
                GetPolicy {
                    user_agent,
                    authorization,
                    ..policy()
                },
                &control,
                Some(&budget),
                deadline,
            )
            .await
        })
        .unwrap_err();
        assert_eq!(error.code, "records.remote_http");
        assert_eq!(budget.snapshot().unwrap().network_requests, 0);
    }
}

#[test]
fn absent_network_budget_allows_page_bound_beyond_shared_default_caps() {
    let control = OperationControl::default();
    let unrelated = Budget::controlled(
        Cost {
            network_requests: 0,
            network_bytes: 0,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    );
    // Controlled helper only: simulate three hops for each of the 2048 allowed
    // Wago pages without opening 6144 sockets. Production Wago passes None at
    // the same get_once/drain/read call sites (checked in the frozen excerpt).
    let mut attempts = 0;
    for _ in 0..2048 {
        for _ in 0..3 {
            attempt(None).unwrap();
            delivered(2048, None, &control, None).unwrap();
            attempts += 1;
        }
    }
    assert_eq!(attempts, 6144);
    // Even an observed amount above the unrelated 1GiB default is no quota
    // event when no ledger was requested; this does not allocate that amount.
    delivered((1 << 30) + 1, None, &control, None).unwrap();
    assert_eq!(unrelated.snapshot().unwrap(), Cost::default());
    assert_eq!(unrelated.logical_snapshot().unwrap(), Cost::default());
}
