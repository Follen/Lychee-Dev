//! Fixed Go listfile HTTP policy over the shared cancellable transport. A single
//! 120-second request deadline covers every redirect and decoded body read.
use super::*;
use crate::remote::transport::{
    self, BodyPolicy, GetPolicy, LocatorPolicy, redirect_url::RedirectUrl,
};
use std::time::{Duration, Instant};

trait Sender {
    async fn get(
        &self,
        url: &str,
        policy: GetPolicy<'_>,
        control: &OperationControl,
        deadline: Instant,
    ) -> Result<reqwest::Response>;
}
struct Network;
impl Sender for Network {
    async fn get(
        &self,
        url: &str,
        policy: GetPolicy<'_>,
        control: &OperationControl,
        deadline: Instant,
    ) -> Result<reqwest::Response> {
        transport::get_once(url, policy, control, None, deadline).await
    }
}
fn allowed(url: &RedirectUrl) -> bool {
    url.scheme == "https"
        && url
            .authority
            .as_deref()
            .is_some_and(|host| !host.is_empty() && !host.contains('@'))
}
fn query() -> ToolkitError {
    fault("records.listfile_query", 2)
}
fn limit() -> ToolkitError {
    fault("records.listfile_limit", 3)
}
fn transport_error(error: ToolkitError) -> ToolkitError {
    match error.code.as_str() {
        "records.remote_http" | "records.remote_range" | "records.hotfix_wago_http" => {
            let mut result = unavailable(error.message.clone());
            result.details = serde_json::json!({"transportCause":error});
            result
        }
        "records.metadata_limit" => ToolkitError {
            code: "records.listfile_limit".into(),
            ..error
        },
        _ => error,
    }
}
pub(super) fn fetch(locator: &str, maximum: usize, control: &OperationControl) -> Result<Vec<u8>> {
    fetch_with(locator, maximum, control, &Network)
}
fn fetch_with(
    locator: &str,
    maximum: usize,
    control: &OperationControl,
    sender: &impl Sender,
) -> Result<Vec<u8>> {
    control.check()?;
    let original = RedirectUrl::absolute(locator).map_err(|_| query())?;
    if !allowed(&original) || maximum == 0 || maximum == usize::MAX {
        return Err(query());
    }
    let deadline = control
        .deadline()
        .min(Instant::now() + Duration::from_secs(120));
    let result = transport::run(fetch_http(original, maximum, sender, control, deadline))
        .map_err(transport_error);
    // The shared reader already observes the original tracked scope. Preserve a
    // completed I/O/codec cause if a stop arrives before returning to the caller.
    finish_stop(result, control.check())
}
async fn fetch_http(
    mut current: RedirectUrl,
    maximum: usize,
    sender: &impl Sender,
    control: &OperationControl,
    deadline: Instant,
) -> Result<Vec<u8>> {
    let mut referer = None;
    let mut drains = Vec::new();
    let result = async {
        for attempt in 1..=5 {
            control.check()?;
            let response = sender
                .get(
                    &current.wire()?,
                    GetPolicy {
                        locator: LocatorPolicy::HttpsOnly,
                        body: BodyPolicy::GoAutomaticGzip,
                        accept: None,
                        referer: referer.as_deref(),
                        user_agent: Some("Go-http-client/1.1"),
                        authorization: None,
                    },
                    control,
                    deadline,
                )
                .await?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .headers()
                    .get("Location")
                    .map(|v| v.to_str())
                    .transpose()
                    .map_err(|_| unavailable("invalid redirect Location"))?
                    .unwrap_or("");
                if !location.is_empty() {
                    // Go resolves Location before CheckRedirect and before drain.
                    let next = current.resolve(location)?;
                    // CheckRedirect itself does not require a host. A hostless or
                    // opaque HTTPS URL is rejected later by the transport.
                    let rejected = attempt == 5
                        || next.scheme != "https"
                        || next
                            .authority
                            .as_deref()
                            .is_some_and(|host| host.contains('@'));
                    let next_referer = current.referer(&next);
                    match transport::drain_redirect_response(
                        response,
                        BodyPolicy::GoAutomaticGzip,
                        control,
                        None,
                        deadline,
                    )
                    .await
                    {
                        Ok(Some(error)) => drains.push(error),
                        Ok(None) => {}
                        Err(error) if rejected => drains.push(error),
                        Err(error) => return Err(error),
                    }
                    if rejected {
                        return Err(query());
                    }
                    current = next;
                    referer = next_referer;
                    continue;
                }
            }
            if status != 200 {
                return Err(unavailable(format!("HTTP {status}")));
            }
            // Go removes Content-Length only when its transport automatically
            // installs the gzip reader; the decoded limit then governs delivery.
            let gzip = response.content_length() != Some(0)
                && response
                    .headers()
                    .get("Content-Encoding")
                    .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"gzip"));
            if !gzip
                && response
                    .content_length()
                    .is_some_and(|n| n > maximum as u64)
            {
                return Err(limit());
            }
            let raw = transport::read_response_prefix(
                response,
                BodyPolicy::GoAutomaticGzip,
                maximum + 1,
                control,
                None,
                deadline,
            )
            .await?;
            if raw.len() > maximum {
                return Err(limit());
            }
            if raw.is_empty() {
                return Err(unavailable("empty listfile response"));
            }
            return Ok(raw);
        }
        unreachable!("fifth redirect rejects")
    }
    .await;
    result.map_err(|mut error| {
        if !drains.is_empty() {
            error.details =
                serde_json::json!({"primaryDetails":error.details,"redirectBodyErrors":drains});
        }
        error
    })
}
#[cfg(test)]
mod tests;
