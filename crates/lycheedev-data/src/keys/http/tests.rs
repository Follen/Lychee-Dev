//! Scripted responses exercise the real redirect consumer and shared readers.
//! No sockets, external endpoints, subprocesses or stalled-server runner.
use super::*;
use crate::resource::Cost;
use std::{cell::RefCell, collections::VecDeque, time::Duration};
#[derive(Debug)]
struct Seen { url: String, referer: Option<String>, authorization: Option<String> }
struct Script { replies: RefCell<VecDeque<reqwest::Response>>, seen: RefCell<Vec<Seen>> }
impl Script {
    fn new(replies: Vec<reqwest::Response>) -> Self {
        Self { replies: RefCell::new(replies.into()), seen: RefCell::new(Vec::new()) }
    }
}
impl Sender for Script {
    async fn get(&self, url: &str, policy: GetPolicy<'_>, control: &OperationControl,
        budget: Option<&Budget>, _: Instant) -> Result<reqwest::Response> {
        control.check()?;
        if let Some(budget) = budget { budget.charge(Cost { network_requests: 1, ..Cost::default() })?; }
        assert!(matches!(policy.body, BodyPolicy::GoAutomaticGzip));
        assert!(matches!(policy.locator, LocatorPolicy::HttpsOnly));
        assert_eq!(policy.user_agent, Some("Go-http-client/1.1"));
        assert_eq!(policy.accept, None);
        self.seen.borrow_mut().push(Seen { url: url.into(), referer: policy.referer.map(str::to_owned), authorization: policy.authorization.map(str::to_owned) });
        Ok(self.replies.borrow_mut().pop_front().expect("only admitted requests"))
    }
}
fn response(status: u16, location: Option<&str>, body: Vec<u8>) -> reqwest::Response {
    let mut builder = ureq::http::Response::builder().status(status).header("Content-Length", body.len());
    if let Some(location) = location { builder = builder.header("Location", location); }
    reqwest::Response::from(builder.body(body).unwrap())
}
fn scope() -> (OperationControl, Budget) {
    let deadline = Instant::now() + Duration::from_secs(10);
    (OperationControl::uncancelable(deadline), Budget::new(Cost::defaults(), deadline, None))
}
fn run(script: &Script, control: &OperationControl, budget: &Budget) -> Result<Vec<u8>> {
    transport::run(fetch_http(URL, script, control, Some(budget), control.deadline())).map_err(map_error)
}
#[test]
fn public_http_two_relative_redirects_charge_actual_hops_and_decoded_drains() {
    let script = Script::new(vec![response(302, Some("../first"), b"abc".to_vec()),
        response(307, Some("/final?q=1"), b"de".to_vec()), response(200, None, vec![b'x'; PUBLIC_BYTES])]);
    let (control, budget) = scope();
    assert_eq!(run(&script, &control, &budget).unwrap().len(), PUBLIC_BYTES);
    let seen = script.seen.borrow();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[1].url, "https://raw.githubusercontent.com/wowdev/TACTKeys/first");
    assert_eq!(seen[1].referer.as_deref(), Some(URL));
    assert_eq!(seen[2].url, "https://raw.githubusercontent.com/final?q=1");
    let usage = budget.snapshot().unwrap();
    assert_eq!(usage.network_requests, 3);
    assert_eq!(usage.network_bytes, PUBLIC_BYTES as u64 + 5);
}
#[test]
fn public_http_third_redirect_is_rejected_after_qualified_drain() {
    let script = Script::new((0..3).map(|_| response(302, Some("/again"), b"abc".to_vec())).collect());
    let (control, budget) = scope();
    assert_eq!(run(&script, &control, &budget).unwrap_err().code, "records.invalid_key_document");
    assert_eq!(script.seen.borrow().len(), 3);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 9);
}
#[test]
fn public_http_redirect_guard_uses_exact_go_host_not_normalized_host() {
    for location in ["http://raw.githubusercontent.com/x", "https://RAW.githubusercontent.com/x",
        "https://raw.githubusercontent.com:443/x", "https://raw.githubusercontent.com./x", "https://else.test/x"] {
        let script = Script::new(vec![response(302, Some(location), b"abc".to_vec())]);
        let (control, budget) = scope();
        assert_eq!(run(&script, &control, &budget).unwrap_err().code, "records.invalid_key_document", "{location}");
        assert_eq!(script.seen.borrow().len(), 1);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    }
}
#[test]
fn public_http_url_userinfo_is_basic_and_referer_excludes_credentials() {
    let script = Script::new(vec![response(302, Some("https://user:pass@raw.githubusercontent.com/a"), vec![]),
        response(302, Some("/b"), vec![]), response(200, None, vec![b'x'; PUBLIC_BYTES])]);
    let (control, budget) = scope();
    run(&script, &control, &budget).unwrap();
    let seen = script.seen.borrow();
    assert_eq!(seen[1].authorization.as_deref(), Some("Basic dXNlcjpwYXNz"));
    assert_eq!(seen[2].authorization.as_deref(), Some("Basic dXNlcjpwYXNz"));
    assert_eq!(seen[2].referer.as_deref(), Some("https://raw.githubusercontent.com/a"));
    assert!(!seen[1].url.contains('@'));
}
#[test]
fn public_http_read_first_limit_plus_one_does_not_count_extra_tail() {
    let script = Script::new(vec![response(200, None, vec![b'x'; PUBLIC_BYTES + 17])]);
    let (control, budget) = scope();
    assert_eq!(run(&script, &control, &budget).unwrap_err().code, "records.invalid_key_document");
    assert_eq!(budget.snapshot().unwrap().network_bytes, PUBLIC_BYTES as u64 + 1);
}
#[test]
fn public_http_status_and_missing_location_do_not_read_terminal_bodies() {
    for status in [201, 302, 404, 500] {
        let script = Script::new(vec![response(status, None, vec![b'x'; 99])]);
        let (control, budget) = scope();
        assert_eq!(run(&script, &control, &budget).unwrap_err().code, "records.invalid_key_document");
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}
#[test]
fn public_http_malformed_location_precedes_drain_and_budget_denial_stays_typed() {
    let script = Script::new(vec![response(302, Some("/%zz"), b"abc".to_vec())]);
    let (control, budget) = scope();
    assert_eq!(run(&script, &control, &budget).unwrap_err().code, "records.invalid_key_document");
    assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    let denied = Budget::new(Cost { network_requests: 0, ..Cost::defaults() }, control.deadline(), None);
    let script = Script::new(vec![]);
    assert_eq!(run(&script, &control, &denied).unwrap_err().code, "records.query_resource_budget");
    assert!(script.seen.borrow().is_empty());
}
#[test]
fn public_http_original_cancellation_prevents_any_request() {
    let (control, budget) = scope();
    control.tracked_cancel_handle().unwrap().cancel();
    let script = Script::new(vec![]);
    assert_eq!(run(&script, &control, &budget).unwrap_err().code, "command.cancelled");
    assert!(script.seen.borrow().is_empty());
}
#[test]
fn public_http_automatic_gzip_counts_decoded_document_bytes() {
    use std::io::Write;
    let raw = vec![b'x'; PUBLIC_BYTES];
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&raw).unwrap();
    let compressed = encoder.finish().unwrap();
    assert!(compressed.len() < PUBLIC_BYTES);
    let response = reqwest::Response::from(ureq::http::Response::builder().status(200)
        .header("Content-Encoding", "gzip").header("Content-Length", compressed.len())
        .body(compressed).unwrap());
    let script = Script::new(vec![response]);
    let (control, budget) = scope();
    assert_eq!(run(&script, &control, &budget).unwrap(), raw);
    assert_eq!(budget.snapshot().unwrap().network_bytes, PUBLIC_BYTES as u64);
}
#[test]
fn public_http_known_large_redirect_body_is_not_consumed() {
    let script = Script::new(vec![response(302, Some("/next"), vec![b'x'; 2049]),
        response(200, None, vec![b'x'; PUBLIC_BYTES])]);
    let (control, budget) = scope();
    run(&script, &control, &budget).unwrap();
    assert_eq!(budget.snapshot().unwrap().network_bytes, PUBLIC_BYTES as u64);
}
#[test]
fn public_http_short_success_body_is_invalid_after_actual_read() {
    let script = Script::new(vec![response(200, None, b"short".to_vec())]);
    let (control, budget) = scope();
    assert_eq!(run(&script, &control, &budget).unwrap_err().code, "records.invalid_key_document");
    assert_eq!(budget.snapshot().unwrap().network_bytes, 5);
}

struct OnSend<F> { action: F, calls: std::cell::Cell<usize> }
impl<F> Sender for OnSend<F>
where F: Fn(&OperationControl, Instant) -> Result<reqwest::Response> {
    async fn get(&self, _: &str, _: GetPolicy<'_>, control: &OperationControl,
        _: Option<&Budget>, deadline: Instant) -> Result<reqwest::Response> {
        self.calls.set(self.calls.get() + 1);
        (self.action)(control, deadline)
    }
}
fn sleep_past(deadline: Instant) {
    std::thread::sleep(deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(2));
}
fn transport_failure() -> ToolkitError {
    let mut error = fault("records.remote_http", 5, "scripted transport failure");
    error.details = json!({"redirectBodyErrors":[{"code":"records.remote_http","message":"scripted drain cause","exit_code":5,"details":{"cleanup":"preserved"}}]});
    error
}
#[test]
fn public_fetcher_preserves_original_first_stop_in_both_orders_and_transport_causes() {
    for cancel_first in [false, true] {
        let deadline = Instant::now() + Duration::from_millis(200);
        let control = OperationControl::uncancelable(deadline);
        let cancel = control.tracked_cancel_handle().unwrap();
        let sender = OnSend { calls: std::cell::Cell::new(0), action: |actual: &OperationControl, _: Instant| {
            // Real PublicFetcher must pass the original tracked scope, not an
            // unordered child or a fresh cancellation source.
            assert!(actual.tracked_cancel_handle().is_ok());
            if cancel_first { cancel.cancel(); }
            sleep_past(deadline);
            if !cancel_first { cancel.cancel(); }
            Err(transport_failure())
        }};
        let failure = PublicFetcher.fetch_with(&sender, &control, None, deadline).unwrap_err();
        assert_eq!(sender.calls.get(), 1);
        assert_eq!(failure.code, if cancel_first { "command.cancelled" } else { "command.deadline" });
        assert_eq!(failure.code, control.check().unwrap_err().code);
        assert_eq!(failure.details["transportCause"]["details"]["redirectBodyErrors"][0]["details"]["cleanup"], "preserved");
    }
}
#[test]
fn public_fetcher_original_stop_precedes_independent_budget_stop_but_keeps_both() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let deadline = Instant::now() + Duration::from_secs(10);
    let control = OperationControl::uncancelable(deadline);
    let cancel = control.tracked_cancel_handle().unwrap();
    let budget_cancel = Arc::new(AtomicBool::new(false));
    let budget = Budget::new(Cost::defaults(), deadline, Some(budget_cancel.clone()));
    let sender = OnSend { calls: std::cell::Cell::new(0), action: |_: &OperationControl, _: Instant| {
        budget_cancel.store(true, Ordering::Release);
        cancel.cancel();
        Err(transport_failure())
    }};
    let failure = PublicFetcher.fetch_with(&sender, &control, Some(&budget), deadline).unwrap_err();
    assert_eq!(failure.code, "command.cancelled");
    assert_eq!(failure.details["secondaryStops"][0]["code"], "operation.cancelled");
    assert_eq!(failure.details["transportCause"]["message"], "scripted transport failure");
}
#[test]
fn public_fetcher_independent_budget_stop_precedes_transport_classification() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let (control, _) = scope();
    let cancel = Arc::new(AtomicBool::new(false));
    let budget = Budget::new(Cost::defaults(), control.deadline(), Some(cancel.clone()));
    let sender = OnSend { calls: std::cell::Cell::new(0), action: |_: &OperationControl, _: Instant| {
        cancel.store(true, Ordering::Release);
        Err(transport_failure())
    }};
    let failure = PublicFetcher.fetch_with(&sender, &control, Some(&budget), control.deadline()).unwrap_err();
    assert_eq!(failure.code, "operation.cancelled");
    assert_eq!(failure.details["transportCause"]["code"], "records.remote_http");
    assert!(control.check().is_ok());
}
#[test]
fn public_fetcher_fixed_request_timeout_is_typed_without_retiring_original_scope() {
    let (control, budget) = scope();
    let request_deadline = Instant::now() + Duration::from_millis(100);
    let sender = OnSend { calls: std::cell::Cell::new(0), action: |_: &OperationControl, actual_deadline: Instant| {
        assert_eq!(actual_deadline, request_deadline);
        sleep_past(actual_deadline);
        Err(transport_failure())
    }};
    let failure = PublicFetcher.fetch_with(&sender, &control, Some(&budget), request_deadline).unwrap_err();
    assert_eq!((failure.code.as_str(), failure.exit_code), ("command.deadline", 5));
    assert_eq!(failure.details["transportCause"]["message"], "scripted transport failure");
    assert!(control.check().is_ok());
    assert!(budget.check().is_ok());
}
#[test]
fn public_fetcher_uses_one_capped_deadline_for_all_actual_consumer_hops() {
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(60));
    let before = Instant::now();
    let deadlines = RefCell::new(Vec::new());
    let sender = OnSend { calls: std::cell::Cell::new(0), action: |_: &OperationControl, deadline: Instant| {
        let mut seen = deadlines.borrow_mut();
        seen.push(deadline);
        Ok(if seen.len() == 1 { response(302, Some("/next"), vec![]) }
            else { response(200, None, vec![b'x'; PUBLIC_BYTES]) })
    }};
    assert_eq!(PublicFetcher.fetch_with(&sender, &control, None, control.deadline()).unwrap().len(), PUBLIC_BYTES);
    let after = Instant::now();
    let deadlines = deadlines.borrow();
    assert_eq!(deadlines.len(), 2);
    assert_eq!(deadlines[0], deadlines[1]);
    assert!(deadlines[0] >= before + Duration::from_secs(30));
    assert!(deadlines[0] <= after + Duration::from_secs(30));
    assert!(deadlines[0] < control.deadline());
}
#[test]
fn public_fetcher_known_legal_encoded_dot_redirect_gap_remains_explicit() {
    let next = RedirectUrl::absolute(URL).unwrap().resolve("/a/%2e/b").unwrap();
    assert!(allowed(&next)); // Fixed Go keys CheckRedirect admits this URL.
    assert!(next.text(false).ends_with("/a/%2e/b"));
    let script = Script::new(vec![response(302, Some("/a/%2e/b"), b"abc".to_vec())]);
    let (control, budget) = scope();
    let failure = PublicFetcher.fetch_with(&script, &control, Some(&budget), control.deadline()).unwrap_err();
    // This asserts an acknowledged incompatibility, not a passing Go parity
    // vector: reqwest cannot yet emit this escaped-dot target unchanged.
    assert_eq!(failure.code, "records.invalid_key_document");
    assert_eq!(script.seen.borrow().len(), 1);
    assert_eq!(budget.snapshot().unwrap().network_bytes, 3);
    assert!(failure.details["transportCause"]["message"].as_str().unwrap().contains("cannot be represented"));
}
#[test]
fn public_context_memoizes_corrected_public_fetcher_first_failure_with_all_causes() {
    struct OwnedHome(std::path::PathBuf);
    impl Drop for OwnedHome { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let home = OwnedHome(std::env::temp_dir().join(format!("lycheedev-public-first-stop-{}-{}",
        std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))));
    lycheedev_core::workspace::initialize(&home.0).unwrap();
    let deadline = Instant::now() + Duration::from_millis(200);
    let control = OperationControl::uncancelable(deadline);
    let cancel = control.tracked_cancel_handle().unwrap();
    let inv = Invocation { command: "records file".into(), positional: None,
        flags: std::collections::BTreeMap::new(), home: home.0.clone(), cwd: home.0.clone(), control: control.clone() };
    let mut context = Context::prepare(&inv, deadline).unwrap();
    let sender = OnSend { calls: std::cell::Cell::new(0), action: |_: &OperationControl, _: Instant| {
        sleep_past(deadline);
        cancel.cancel();
        Err(transport_failure())
    }};
    struct ThroughPublic<'a, F>(&'a OnSend<F>);
    impl<F: Fn(&OperationControl, Instant) -> Result<reqwest::Response>> Fetcher for ThroughPublic<'_, F> {
        fn fetch(&self, control: &OperationControl, budget: Option<&Budget>, deadline: Instant) -> Result<Vec<u8>> {
            PublicFetcher.fetch_with(self.0, control, budget, deadline)
        }
    }
    let fetcher = ThroughPublic(&sender);
    let first = context.ensure_public_with(&fetcher).unwrap_err();
    let second = context.ensure_public_with(&fetcher).unwrap_err();
    assert_eq!(first.code, "command.deadline");
    assert_eq!(serde_json::to_value(&first).unwrap(), serde_json::to_value(&second).unwrap());
    assert_eq!(first.details["transportCause"]["details"]["redirectBodyErrors"][0]["details"]["cleanup"], "preserved");
    assert_eq!(context.source.state, "unavailable");
    assert_eq!(sender.calls.get(), 1);
}
