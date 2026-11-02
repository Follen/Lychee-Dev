//! Pinned public-key HTTP consumer. The shared transport owns cancellation,
//! proxy selection, gzip framing and observed decoded-byte accounting.
use super::*;
use crate::remote::transport::{self, BodyPolicy, GetPolicy, LocatorPolicy, redirect_url::RedirectUrl};
use crate::resource::Budget;
const URL: &str = "https://raw.githubusercontent.com/wowdev/TACTKeys/71b75360752840dd62a412013a38b5a972524a50/WoW.txt";
pub(super) trait Fetcher {
    fn fetch(&self, control: &OperationControl, budget: Option<&Budget>, deadline: Instant) -> Result<Vec<u8>>;
}
pub(super) struct PublicFetcher;
impl Fetcher for PublicFetcher {
    fn fetch(&self, control: &OperationControl, budget: Option<&Budget>, deadline: Instant) -> Result<Vec<u8>> {
        self.fetch_with(&Network, control, budget, deadline)
    }
}
impl PublicFetcher {
    // The same entry point is used by production and deterministic response
    // senders. Never replace the caller with an unordered child: that loses
    // tracked first-stop history when cancellation arrives after its deadline.
    fn fetch_with(&self, sender: &impl Sender, control: &OperationControl,
        budget: Option<&Budget>, total_deadline: Instant) -> Result<Vec<u8>> {
        let deadline = transport::request_deadline(control, budget)?.min(total_deadline);
        let result = transport::run(fetch_http(URL, sender, control, budget, deadline));
        finish_request(result, control, budget, deadline)
    }
}
fn finish_request(result: Result<Vec<u8>>, control: &OperationControl,
    budget: Option<&Budget>, deadline: Instant) -> Result<Vec<u8>> {
    // Preserve original causal ordering first, then the independent Data stop.
    // Query and transport deadlines remain fixed across redirects; an elapsed
    // request timeout has a typed cause even if the shared transport reports I/O.
    let caller_stop = control.check().err();
    let budget_stop = budget.and_then(|budget| budget.check().err());
    let request_stop = (Instant::now() >= deadline)
        .then(|| fault("command.deadline", 5, "Public key request deadline exceeded"));
    if caller_stop.is_none() && budget_stop.is_none() && request_stop.is_none() {
        return result.map_err(map_error);
    }
    let mut causes = caller_stop.into_iter().chain(budget_stop).chain(request_stop);
    let mut primary = causes.next().expect("at least one terminal stop");
    let secondary: Vec<_> = causes.collect();
    // Retain complete typed failures, including nested redirect drain/cleanup
    // causes. Selecting a stop must not discard the transport diagnostic tree.
    primary.details = json!({"primaryDetails":primary.details,
        "secondaryStops":secondary,"transportCause":result.err()});
    Err(primary)
}
trait Sender {
    async fn get(&self, url: &str, policy: GetPolicy<'_>, control: &OperationControl,
        budget: Option<&Budget>, deadline: Instant) -> Result<reqwest::Response>;
}
struct Network;
impl Sender for Network {
    async fn get(&self, url: &str, policy: GetPolicy<'_>, control: &OperationControl,
        budget: Option<&Budget>, deadline: Instant) -> Result<reqwest::Response> {
        transport::get_once(url, policy, control, budget, deadline).await
    }
}
fn allowed(url: &RedirectUrl) -> bool {
    url.scheme == "https" && url.authority.as_deref()
        .map(|a| a.rsplit_once('@').map_or(a, |(_, host)| host)) == Some("raw.githubusercontent.com")
}
fn map_error(error: ToolkitError) -> ToolkitError {
    match error.code.as_str() {
        "records.remote_http" | "records.remote_range" | "records.hotfix_wago_http" => {
            let mut mapped = invalid();
            mapped.details = json!({"transportCause": error});
            mapped
        }
        _ => error,
    }
}
async fn fetch_http(original: &str, sender: &impl Sender, control: &OperationControl,
    budget: Option<&Budget>, deadline: Instant) -> Result<Vec<u8>> {
    let mut current = RedirectUrl::absolute(original)?;
    if !allowed(&current) { return Err(invalid()); }
    let mut referer = None;
    let mut drains = Vec::new();
    let result = async {
        for attempt in 1..=3 {
            let authorization = current.authorization()?;
            let response = sender.get(&current.wire()?, GetPolicy {
                locator: LocatorPolicy::HttpsOnly,
                body: BodyPolicy::GoAutomaticGzip,
                accept: None,
                referer: referer.as_deref(),
                user_agent: Some("Go-http-client/1.1"),
                authorization: authorization.as_deref(),
            }, control, budget, deadline).await?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = response.headers().get("Location")
                    .map(|v| v.to_str()).transpose().map_err(|_| invalid())?.unwrap_or("");
                if !location.is_empty() {
                    // Parse before CheckRedirect; malformed Location does not drain.
                    let next = current.resolve(location)?;
                    let rejected = attempt == 3 || !allowed(&next);
                    let next_referer = current.referer(&next);
                    let drained = transport::drain_redirect_response(response,
                        BodyPolicy::GoAutomaticGzip, control, budget, deadline).await;
                    match drained {
                        Ok(Some(error)) => drains.push(error),
                        Ok(None) => {},
                        Err(error) if rejected => drains.push(error),
                        Err(error) => return Err(error),
                    }
                    if rejected { return Err(invalid()); }
                    current = next;
                    referer = next_referer;
                    continue;
                }
            }
            if status != 200 { return Err(invalid()); }
            let raw = transport::read_response_prefix(response,
                BodyPolicy::GoAutomaticGzip, PUBLIC_BYTES + 1,
                control, budget, deadline).await?;
            if raw.len() != PUBLIC_BYTES { return Err(invalid()); }
            return Ok(raw);
        }
        unreachable!("third redirect is rejected")
    }.await;
    result.map_err(|mut error| {
        if !drains.is_empty() {
            error.details = json!({"primaryDetails":error.details,"redirectBodyErrors":drains});
        }
        error
    })
}
#[cfg(test)]
mod tests;
