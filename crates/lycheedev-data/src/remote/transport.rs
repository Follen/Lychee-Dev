// SPDX-License-Identifier: MIT
//! Strict 206 byte ranges. No automatic redirects, whole-object fallback or
//! transparent decompression. Each fetch owns one current-thread async runtime;
//! cancellation drops its in-flight HTTP future and its connection driver.
//! OS resolver blocking work may finish independently, without an HTTP request.
use super::{catalog::decimal, limit};
use crate::{
    fault,
    resource::{Budget, Cost},
};
use futures_util::TryStreamExt;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use reqwest::header::HeaderMap;
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::StreamReader;

mod raw_uri;
pub(crate) mod redirect_url;

pub const MAX_RANGE_BYTES: i64 = 64 << 20;
pub const MAX_OBJECT_BYTES: i64 = 1 << 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeRequest {
    pub offset: i64,
    pub length: i64,
    /// Zero means unknown. It never means that a missing header is acceptable.
    pub total: i64,
}
impl RangeRequest {
    fn end(self) -> Result<i64> {
        if self.length < 1 || self.length > MAX_RANGE_BYTES {
            return Err(limit());
        }
        if self.offset < 0 || self.total < 0 {
            return Err(range("negative range"));
        }
        let end = self
            .offset
            .checked_add(self.length - 1)
            .ok_or_else(|| range("range overflow"))?;
        if self.total > MAX_OBJECT_BYTES {
            return Err(limit());
        }
        if self.total != 0 && self.total <= end {
            return Err(range("range exceeds total"));
        }
        Ok(end)
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct RangeReply {
    pub bytes: Vec<u8>,
    pub object_bytes: i64,
}

fn range(message: impl Into<String>) -> ToolkitError {
    fault("records.remote_range", 4, message)
}
fn http(message: impl Into<String>) -> ToolkitError {
    fault("records.remote_http", 5, message)
}

fn url(locator: &str) -> Result<ureq::http::Uri> {
    let uri: ureq::http::Uri = locator.parse().map_err(|_| http("invalid remote URL"))?;
    let secure = uri.scheme_str() == Some("https");
    #[cfg(test)]
    let secure = secure || (uri.scheme_str() == Some("http") && uri.host() == Some("127.0.0.1"));
    if !secure
        || uri
            .authority()
            .is_none_or(|a| a.as_str().contains('@') || a.host().is_empty())
    {
        return Err(http("remote URL requires HTTPS without user information"));
    }
    Ok(uri)
}
fn check(control: &OperationControl, budget: Option<&Budget>) -> Result<()> {
    control.check()?;
    if let Some(budget) = budget {
        budget.check()?;
    }
    Ok(())
}
pub(crate) fn request_deadline(
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<Instant> {
    check(control, budget)?;
    let mut deadline = control
        .deadline()
        .min(Instant::now() + Duration::from_secs(30));
    if let Some(budget) = budget {
        deadline = deadline.min(budget.deadline);
    }
    Ok(deadline)
}
fn client(deadline: Instant) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .http1_only()
        .pool_max_idle_per_host(0)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .timeout(deadline.saturating_duration_since(Instant::now()));
    // The route was selected once before this direct-only fallback. Never
    // re-read proxy environment or enable reqwest's different system matcher.
    builder
        .no_proxy()
        .build()
        .map_err(|error| http(error.to_string()))
}
/// No spawned request is shared with another operation. Runtime shutdown drops
/// all async connection drivers. Already-started OS DNS blocking work is not
/// forcibly joined; it cannot advance into an HTTP connection after shutdown.
struct OwnedRuntime(Option<tokio::runtime::Runtime>);
impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_timeout(Duration::ZERO);
        }
    }
}
pub(crate) fn run<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(fault(
            "records.internal",
            8,
            "Synchronous remote fetch entered an async runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| http(error.to_string()))?;
    let runtime = OwnedRuntime(Some(runtime));
    let result = runtime.0.as_ref().expect("owned runtime").block_on(work);
    drop(runtime);
    result
}
async fn controlled<F: Future>(
    future: F,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<F::Output> {
    tokio::pin!(future);
    loop {
        check(control, budget)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| http("remote request timeout"))?;
        tokio::select! {
            biased;
            result = &mut future => return Ok(result),
            _ = tokio::time::sleep(remaining.min(Duration::from_millis(10))) => {},
        }
    }
}
fn attempt(budget: Option<&Budget>) -> Result<()> {
    if let Some(budget) = budget {
        budget.charge(Cost {
            network_requests: 1,
            ..Default::default()
        })?;
    }
    Ok(())
}
fn values<'a>(headers: &'a HeaderMap, name: &str) -> Result<Vec<&'a str>> {
    headers
        .get_all(name)
        .iter()
        .map(|v| v.to_str().map_err(|_| range(format!("non-text {name}"))))
        .collect()
}
fn with_secondary(mut primary: ToolkitError, secondary: Option<ToolkitError>) -> ToolkitError {
    if let Some(secondary) = secondary {
        primary.details = serde_json::json!({"primaryDetails":primary.details,"secondary":{"code":secondary.code,"message":secondary.message,"details":secondary.details}});
    }
    primary
}
fn read_amount(maximum: usize, used: usize, budget: Option<&Budget>) -> Result<usize> {
    let mut amount = (maximum - used).min(8191) + 1;
    if let Some(budget) = budget {
        amount = amount.min(budget.remaining_network_bytes()?.min(8191) as usize + 1);
    }
    Ok(amount)
}
// Observed decoded/raw delivery is never lost to a concurrent stop or I/O error.
fn delivered(
    count: usize,
    mut error: Option<ToolkitError>,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<()> {
    if (count != 0 || error.is_none())
        && let Some(budget) = budget
    {
        if let Err(mut charge) = budget.charge_observed_network_bytes(count as u64) {
            charge.details =
                serde_json::json!({"primaryDetails":charge.details,"observedReadBytes":count});
            error = Some(with_secondary(charge, error));
        }
    }
    if let Err(stop) = check(control, budget) {
        error = Some(with_secondary(stop, error));
    }
    error.map_or(Ok(()), Err)
}
fn append(raw: &mut Vec<u8>, bytes: &[u8], maximum: usize) -> Result<()> {
    if bytes.len() > maximum - raw.len() {
        return Err(limit());
    }
    raw.try_reserve(bytes.len()).map_err(|_| {
        fault(
            "records.query_resource_budget",
            3,
            "Remote response allocation failed",
        )
    })?;
    raw.extend_from_slice(bytes);
    Ok(())
}
#[derive(Clone, Copy)]
enum ReadLimit {
    Complete(usize),
    Prefix(usize),
}
impl ReadLimit {
    fn maximum(self) -> usize {
        match self {
            Self::Complete(n) | Self::Prefix(n) => n,
        }
    }
    fn done(self, used: usize) -> bool {
        matches!(self, Self::Prefix(n) if used == n)
    }
    fn amount(self, used: usize, budget: Option<&Budget>) -> Result<usize> {
        match self {
            Self::Complete(n) => read_amount(n, used, budget),
            Self::Prefix(n) => {
                let mut amount = (n - used).min(8192);
                if let Some(budget) = budget {
                    amount = amount.min(budget.remaining_network_bytes()?.min(8191) as usize + 1);
                }
                Ok(amount)
            }
        }
    }
}
async fn collect_admitted(
    mut body: impl AsyncRead + Unpin,
    limit: ReadLimit,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
    admit: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        if limit.done(raw.len()) {
            return Ok(raw);
        }
        check(control, budget)?;
        let amount = limit.amount(raw.len(), budget)?;
        let read = controlled(body.read(&mut buffer[..amount]), control, budget, deadline).await?;
        let (count, error) = match read {
            Ok(count) => (count, None),
            Err(error) => (0, Some(http(error.to_string()))),
        };
        delivered(count, error, control, budget)?;
        if count == 0 {
            return Ok(raw);
        }
        // Network delivery is already recorded, including on a stop. The
        // consumer admits only allocation/work before these bytes are retained.
        admit(count)?;
        append(&mut raw, &buffer[..count], limit.maximum())?;
    }
}
#[derive(Clone, Copy)]
pub(crate) enum BodyPolicy {
    Identity,
    GoAutomaticGzip,
}
fn automatic_gzip(response: &reqwest::Response, policy: BodyPolicy) -> bool {
    matches!(policy, BodyPolicy::GoAutomaticGzip)
        && response.content_length() != Some(0)
        && response
            .headers()
            .get("Content-Encoding")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("gzip"))
}
pub(crate) async fn read_response(
    response: reqwest::Response,
    policy: BodyPolicy,
    maximum: usize,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    collect_response(
        response,
        policy,
        ReadLimit::Complete(maximum),
        control,
        budget,
        deadline,
    )
    .await
}
/// Read at most `maximum` delivered bytes without probing for EOF beyond it.
/// Used by Go io.LimitReader consumers; the consumer validates exact length.
pub(crate) async fn read_response_prefix(
    response: reqwest::Response,
    policy: BodyPolicy,
    maximum: usize,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    collect_response(
        response,
        policy,
        ReadLimit::Prefix(maximum),
        control,
        budget,
        deadline,
    )
    .await
}
/// Same Go LimitReader prefix contract, with per-delivery consumer admission.
/// The callback never owns network accounting and is not invoked on a failed
/// delivery, at EOF, or after the requested prefix is complete.
pub(crate) async fn read_response_prefix_with_admission(
    response: reqwest::Response,
    policy: BodyPolicy,
    maximum: usize,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
    admit: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<Vec<u8>> {
    collect_response_admitted(
        response,
        policy,
        ReadLimit::Prefix(maximum),
        control,
        budget,
        deadline,
        admit,
    )
    .await
}
async fn collect_response(
    response: reqwest::Response,
    policy: BodyPolicy,
    limit: ReadLimit,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    collect_response_admitted(
        response,
        policy,
        limit,
        control,
        budget,
        deadline,
        &mut |_| Ok(()),
    )
    .await
}
async fn collect_response_admitted(
    response: reqwest::Response,
    policy: BodyPolicy,
    limit: ReadLimit,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
    admit: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<Vec<u8>> {
    let gzip = automatic_gzip(&response, policy);
    let reader = StreamReader::new(response.bytes_stream().map_err(std::io::Error::other));
    tokio::pin!(reader);
    if gzip {
        gzip::collect_admitted(reader, limit, control, budget, deadline, admit).await
    } else {
        collect_admitted(reader, limit, control, budget, deadline, admit).await
    }
}
/// The Go redirect branch discards at most2KiB, without an extra limit probe.
/// Its caller ignores body-read failure for redirect choice, but receives it
/// here so observedReadBytes and concurrent stop/decode causes are not erased.
/// Every subsequent send must still check the original control and deadline.
pub(crate) async fn drain_redirect_response(
    response: reqwest::Response,
    policy: BodyPolicy,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<Option<ToolkitError>> {
    if !automatic_gzip(&response, policy) && response.content_length().is_some_and(|n| n > 2048) {
        return Ok(None);
    }
    let result = collect_response(
        response,
        policy,
        ReadLimit::Prefix(2048),
        control,
        budget,
        deadline,
    )
    .await;
    match result {
        Ok(_) => Ok(None),
        Err(error) if error.code == "records.remote_http" => Ok(Some(error)),
        Err(error) => Err(error),
    }
}

/// The Hotfix redirect policy alone admits ordinary HTTP origins. Other remote
/// consumers retain their existing HTTPS-only validation.
#[derive(Clone, Copy)]
pub(crate) enum LocatorPolicy {
    HttpsOnly,
    HotfixRedirect,
}
pub(crate) struct GetPolicy<'a> {
    pub locator: LocatorPolicy,
    pub body: BodyPolicy,
    pub accept: Option<&'a str>,
    pub referer: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    /// Caller-owned per-request header; never copied between redirect URLs.
    pub authorization: Option<&'a str>,
}
pub(crate) async fn get_once(
    locator: &str,
    policy: GetPolicy<'_>,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<reqwest::Response> {
    check(control, budget)?;
    if Instant::now() >= deadline {
        return Err(http("remote request timeout"));
    }
    let mut route_locator = locator.to_owned();
    match policy.locator {
        LocatorPolicy::HttpsOnly => {
            url(locator)?;
        }
        LocatorPolicy::HotfixRedirect => {
            let parsed = redirect_url::RedirectUrl::parse(locator)?;
            // Go lowercases the scheme but retains authority spelling for its
            // localhost special case. Do not reuse WHATWG host normalization.
            if let Some((_, rest)) = locator.split_once(':') {
                route_locator = format!("{}:{rest}", parsed.scheme);
            }
        }
    }
    let encoding = match policy.body {
        BodyPolicy::Identity => "identity",
        BodyPolicy::GoAutomaticGzip => "gzip",
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        "Accept-Encoding",
        reqwest::header::HeaderValue::from_static(encoding),
    );
    for (name, value) in [("Accept", policy.accept), ("Referer", policy.referer)] {
        if let Some(value) = value {
            headers.insert(
                name,
                reqwest::header::HeaderValue::from_str(value)
                    .map_err(|_| http(format!("invalid {name} header")))?,
            );
        }
    }
    if let Some(user_agent) = policy.user_agent {
        if user_agent != "Go-http-client/1.1" {
            return Err(http("unsupported fixed user agent"));
        }
        headers.insert(
            "User-Agent",
            reqwest::header::HeaderValue::from_static("Go-http-client/1.1"),
        );
    }
    if let Some(authorization) = policy.authorization {
        authorization
            .strip_prefix("Basic ")
            .filter(|value| {
                !value.is_empty()
                    && value.len() % 4 == 0
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
            })
            .ok_or_else(|| http("invalid fixed Basic authorization"))?;
        let mut value = reqwest::header::HeaderValue::from_str(authorization)
            .map_err(|_| http("invalid authorization header"))?;
        value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    send_once(locator, &route_locator, headers, control, budget, deadline).await
}

async fn send_once(
    locator: &str,
    _route_locator: &str,
    headers: HeaderMap,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<reqwest::Response> {
    // Request construction/parse has succeeded. Go BudgetTransport observes
    // this RoundTrip even if route selection, missing Host or dialing fails.
    check(control, budget)?;
    if Instant::now() >= deadline {
        return Err(http("remote request timeout"));
    }
    attempt(budget)?;
    if let Some(prepared) = raw_uri::prepare(locator, &headers)? {
        let response = controlled(prepared.send(), control, budget, deadline).await?;
        check(control, budget)?;
        return response;
    }
    let request = client(deadline)?.get(locator).headers(headers);
    let response = controlled(request.send(), control, budget, deadline).await?;
    check(control, budget)?;
    response.map_err(|error| http(error.without_url().to_string()))
}

/// Bounded metadata body with Go automatic gzip and no redirect behavior.
pub fn fetch_metadata(
    locator: &str,
    maximum: usize,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<Vec<u8>> {
    check(control, budget)?;
    if maximum == 0 || maximum > 4 << 20 {
        return Err(limit());
    }
    let uri = url(locator)?;
    let deadline = request_deadline(control, budget)?;
    run(async {
        let response = get_once(
            locator,
            GetPolicy {
                locator: LocatorPolicy::HttpsOnly,
                body: BodyPolicy::GoAutomaticGzip,
                accept: None,
                referer: None,
                user_agent: None,
                authorization: None,
            },
            control,
            budget,
            deadline,
        )
        .await?;
        if response.status().as_u16() != 200 {
            return Err(http(format!(
                "{}{} status {}",
                uri.host().unwrap_or(""),
                uri.path(),
                response.status().as_u16()
            )));
        }
        if !automatic_gzip(&response, BodyPolicy::GoAutomaticGzip)
            && response
                .content_length()
                .is_some_and(|n| n > maximum as u64)
        {
            return Err(limit());
        }
        read_response(
            response,
            BodyPolicy::GoAutomaticGzip,
            maximum,
            control,
            budget,
            deadline,
        )
        .await
    })
}

pub fn fetch_range(
    locator: &str,
    request: RangeRequest,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<RangeReply> {
    check(control, budget)?;
    let end = request.end()?;
    url(locator)?;
    let deadline = request_deadline(control, budget)?;
    run(async {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Range",
            reqwest::header::HeaderValue::from_str(&format!("bytes={}-{}", request.offset, end))
                .map_err(|_| range("invalid range header"))?,
        );
        headers.insert(
            "Accept-Encoding",
            reqwest::header::HeaderValue::from_static("identity"),
        );
        let response = send_once(locator, locator, headers, control, budget, deadline).await?;
        match response.status().as_u16() {
            404 => {
                return Err(fault(
                    "records.remote_object_missing",
                    3,
                    "records.remote_object_missing",
                ));
            }
            200 => return Err(range("status 200 would download the entire archive")),
            206 => (),
            code @ 300..=399 => return Err(range(format!("redirect status {code}"))),
            code => return Err(http(format!("status {code}"))),
        }
        let object_bytes = validate_headers(response.headers(), request)?;
        let bytes = match read_response(
            response,
            BodyPolicy::Identity,
            request.length as usize,
            control,
            budget,
            deadline,
        )
        .await
        {
            Err(error) if error.code == "records.metadata_limit" => {
                return Err(range(format!(
                    "response body exceeds {} bytes",
                    request.length
                )));
            }
            result => result?,
        };
        if bytes.len() as i64 != request.length {
            return Err(range(format!(
                "response body length {}, want {}",
                bytes.len(),
                request.length
            )));
        }
        Ok(RangeReply {
            bytes,
            object_bytes,
        })
    })
}

fn validate_headers(headers: &HeaderMap, request: RangeRequest) -> Result<i64> {
    let end = request.end()?;
    let received = values(headers, "Content-Range")?;
    let content_error = || {
        let total = if request.total == 0 {
            "*".to_owned()
        } else {
            request.total.to_string()
        };
        range(format!(
            "invalid Content-Range: wanted {:?}, received {:?}",
            format!("bytes {}-{end}/{total}", request.offset),
            received.join(", ")
        ))
    };
    let (start, response_end, total) = content_range(&received).map_err(|_| content_error())?;
    if total > MAX_OBJECT_BYTES {
        return Err(limit());
    }
    if start != request.offset
        || response_end != end
        || total <= response_end
        || (request.total != 0 && total != request.total)
    {
        return Err(content_error());
    }
    let encoding = values(headers, "Content-Encoding")?;
    if encoding.len() > 1
        || encoding.first().is_some_and(|value| {
            !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("identity")
        })
    {
        return Err(range("compressed response"));
    }
    if values(headers, "Content-Type")?
        .first()
        .is_some_and(|value| value.trim().to_ascii_lowercase().starts_with("multipart/"))
    {
        return Err(range("multipart response"));
    }
    let length = values(headers, "Content-Length")?;
    if !length.is_empty() {
        if length.len() != 1 || !decimal(length[0]) {
            return Err(range("invalid Content-Length"));
        }
        if length[0].parse::<i64>().ok() != Some(request.length) {
            return Err(range("mismatched Content-Length"));
        }
    }
    Ok(total)
}
pub(crate) fn content_range(values: &[&str]) -> Result<(i64, i64, i64)> {
    if values.len() != 1 {
        return Err(range("Content-Range cardinality"));
    }
    let value = values[0]
        .trim()
        .strip_prefix("bytes ")
        .ok_or_else(|| range("Content-Range unit"))?;
    let (span, total) = value
        .split_once('/')
        .ok_or_else(|| range("Content-Range total"))?;
    let (start, end) = span
        .split_once('-')
        .ok_or_else(|| range("Content-Range span"))?;
    if ![start, end, total].iter().all(|v| decimal(v)) {
        return Err(range("Content-Range decimal"));
    }
    let number = |v: &str| {
        v.parse::<i64>()
            .map_err(|_| range("Content-Range overflow"))
    };
    let (start, end, total) = (number(start)?, number(end)?, number(total)?);
    if start > end {
        return Err(range("Content-Range reversed"));
    }
    Ok((start, end, total))
}

mod gzip;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod redirect_tests;

#[cfg(test)]
mod admission_tests;
