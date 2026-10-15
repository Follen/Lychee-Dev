//! Bounded Wago page walk with immutable response identities and explicit offline cache.
use super::*;
use lycheedev_core::metadata::Metadata;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const PAGE_BYTES: usize = 16 << 20;
const CACHE_SCHEMA: &str = "lycheedev.wago-page.v1";
const PARSER: &str = "lycheedev-wago-inertia-v1";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Identity {
    provider: String,
    parser: String,
    product: String,
    full_build: String,
    region: String,
    locale: String,
    search: String,
    page: i64,
}
impl Identity {
    fn new(q: &Query, page: i64) -> Self {
        Self {
            provider: "wago".into(),
            parser: PARSER.into(),
            product: q.product.clone(),
            full_build: q.full_build.clone(),
            region: q.region.clone(),
            locale: q.locale.clone(),
            search: q.effective_search(),
            page,
        }
    }
    fn key(&self, budget: &Budget) -> Result<String> {
        Ok(storage::sha256(&go_bytes(self, budget, 1 << 20)?))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    schema: String,
    identity: Identity,
    request_url: String,
    response: Value,
    captured_at: String,
    current_page: i64,
    last_page: i64,
    per_page: i64,
    total: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    next_page_url: String,
}
struct Fetched<'a> {
    page: parse::Page<'a>,
    meta: CursorPage,
    captured_at: String,
    from_cache: bool,
}
/// Private seam used by the production HTTP adapter and deterministic fixture transport.
pub(super) trait Fetcher {
    fn fetch(&self, url: &str, control: &OperationControl, budget: &Budget) -> Result<Vec<u8>>;
}
pub(super) struct PublicFetcher;
impl Fetcher for PublicFetcher {
    fn fetch(&self, url: &str, control: &OperationControl, budget: &Budget) -> Result<Vec<u8>> {
        use crate::remote::transport;
        // The whole redirect chain shares the original control and thirty-second
        // request deadline. A hop must never start another timeout or runtime.
        // Go returns ctx.Err directly only before entering client.Do.
        control.check()?;
        let deadline = transport::request_deadline(control, None).map_err(inflight_error)?;
        transport::run(fetch_http(url, control, budget, deadline)).map_err(inflight_error)
    }
}
async fn fetch_http(
    original: &str,
    control: &OperationControl,
    budget: &Budget,
    deadline: std::time::Instant,
) -> Result<Vec<u8>> {
    use crate::remote::transport::{self, BodyPolicy, GetPolicy, LocatorPolicy};
    let mut current = RedirectUrl::absolute(original)?;
    let mut referer = None;
    let mut drains = Vec::new();
    // Wago's Go contract accounts successful final pages, not HTTP hops or
    // redirect slurps. Keep the shared optional network ledger absent: its
    // 4096-request/1GiB caps would reject otherwise valid Wago page walks.
    let result = async {
        for attempt in 1..=10 {
            let authorization = current.authorization()?;
            let response = transport::get_once(
                &current.wire()?,
                GetPolicy {
                    locator: LocatorPolicy::HotfixRedirect,
                    body: BodyPolicy::GoAutomaticGzip,
                    accept: Some("text/html,application/xhtml+xml"),
                    referer: referer.as_deref(),
                    user_agent: Some("Go-http-client/1.1"),
                    authorization: authorization.as_deref(),
                },
                control,
                None,
                deadline,
            )
            .await?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                // Go parses Location before CheckRedirect and before draining.
                // Missing or empty Location returns this response unchanged.
                let location = response
                    .headers()
                    .get("Location")
                    .map(|v| v.to_str())
                    .transpose()
                    .map_err(|_| error("wago_http", 5, "non-text Location header"))?
                    .unwrap_or("");
                if !location.is_empty() {
                    let next = current.resolve(location)?;
                    let next_referer = current.referer(&next);
                    // Go decides CheckRedirect before the qualified drain. A
                    // tenth-hop slurp failure cannot replace that existing error.
                    let redirect_limit = (attempt == 10)
                        .then(|| error("wago_http", 5, "stopped after 10 redirects"));
                    let drained = transport::drain_redirect_response(
                        response,
                        BodyPolicy::GoAutomaticGzip,
                        control,
                        None,
                        deadline,
                    )
                    .await;
                    match drained {
                        Ok(Some(error)) => drains.push(error),
                        Ok(None) => {}
                        Err(error) if redirect_limit.is_some() => {
                            // Preserve the drain cause diagnostically while
                            // publishing Go's preselected CheckRedirect error.
                            drains.push(error);
                        }
                        Err(error) => return Err(error),
                    }
                    if let Some(error) = redirect_limit {
                        return Err(error);
                    }
                    current = next;
                    referer = next_referer;
                    continue;
                }
            }
            if status != 200 {
                // Go checks status before reading the terminal body.
                return Err(error("wago_http", 5, format!("status {status}")));
            }
            let raw = transport::read_response(
                response,
                BodyPolicy::GoAutomaticGzip,
                PAGE_BYTES,
                control,
                None,
                deadline,
            )
            .await?;
            // Preserve the existing successful decoded-page work total. The
            // Go private page counter is committed by read_page, not here.
            // This bounded page is admitted for work only after complete delivery;
            // no failed read invents work, and a stopped operation cannot succeed.
            budget.charge(Cost::work(raw.len()))?;
            control.check()?;
            return Ok(raw);
        }
        unreachable!("the tenth redirect returns before another request")
    }
    .await;
    result.map_err(|mut error| {
        if !drains.is_empty() {
            error.details = json!({
                "primaryDetails": error.details,
                "redirectBodyErrors": drains,
            });
        }
        error
    })
}
fn stopped(error: &ToolkitError) -> bool {
    matches!(
        error.code.as_str(),
        "command.cancelled" | "command.deadline" | "operation.cancelled"
    )
}
fn inflight_error(failure: ToolkitError) -> ToolkitError {
    if stopped(&failure) {
        let mut mapped = error("wago_http", 5, &failure.message);
        // Go's %v wrapper hides errors.Is(context.*) publicly. Preserve the
        // typed cause in diagnostic details; this never delays cancellation.
        mapped.details = json!({"originalStop": failure});
        mapped
    } else {
        http_error(failure)
    }
}
fn http_error(mut failure: ToolkitError) -> ToolkitError {
    let mapped = match failure.code.as_str() {
        "records.remote_http" => Some(error("wago_http", 5, &failure.message)),
        "records.metadata_limit" => Some(error(
            "budget",
            3,
            format!("page exceeds {PAGE_BYTES} bytes"),
        )),
        _ => None,
    };
    if let Some(mapped) = mapped {
        failure.code = mapped.code;
        failure.message = mapped.message;
        failure.exit_code = mapped.exit_code;
    }
    failure
}

// A deliberately local redirect reference, not a second HTTP implementation.
// Keep Go's authority spelling, raw query, fragment inheritance and RFC3986
// path resolution. WHATWG Url::join changes backslashes and encoded dot segments.
// The resolve_path algorithm follows Go net/url (Go BSD license in the shared
// remote/transport/GO-LICENSE); this is limited to the Wago redirect consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RedirectUrl {
    scheme: String,
    opaque: Option<String>,
    authority: Option<String>,
    path: String,
    query: Option<String>,
    fragment: String,
}
impl RedirectUrl {
    fn parse(raw: &str) -> Result<Self> {
        let (rest, fragment) = raw.split_once('#').unwrap_or((raw, ""));
        if rest.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(error("wago_http", 5, "invalid control character in URL"));
        }
        let (mut rest, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query.to_owned())),
            None => (rest, None),
        };
        let mut scheme = String::new();
        if let Some((prefix, suffix)) = rest.split_once(':') {
            if !prefix.is_empty()
                && prefix.as_bytes()[0].is_ascii_alphabetic()
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
            {
                scheme = prefix.to_ascii_lowercase();
                rest = suffix;
            } else if !prefix.contains('/') {
                return Err(error("wago_http", 5, "invalid URL scheme or relative path"));
            }
        }
        // Go leaves a rootless scheme-specific part opaque, including malformed
        // percent sequences. It must reach transport rejection after the drain.
        let opaque = (!scheme.is_empty() && !rest.starts_with('/')).then(|| rest.to_owned());
        if opaque.is_some() {
            return Ok(Self {
                scheme,
                opaque,
                authority: None,
                path: String::new(),
                query,
                fragment: url_component(fragment, true)?,
            });
        }
        let authority =
            if rest.starts_with("//") && (!scheme.is_empty() || !rest.starts_with("///")) {
                let (host, path) = rest[2..]
                    .split_once('/')
                    .map_or((&rest[2..], ""), |(h, _)| (h, &rest[h.len() + 2..]));
                rest = path;
                (!host.is_empty()).then(|| host.to_owned())
            } else {
                None
            };
        if let Some(authority) = &authority {
            validate_authority(&scheme, authority)?;
        }
        Ok(Self {
            scheme,
            opaque,
            authority,
            path: url_component(rest, false)?,
            query,
            fragment: url_component(fragment, true)?,
        })
    }
    fn absolute(raw: &str) -> Result<Self> {
        let parsed = Self::parse(raw)?;
        if parsed.scheme.is_empty() || parsed.authority.is_none() {
            return Err(error("wago_http", 5, "Hotfix URL requires HTTP or HTTPS"));
        }
        Ok(parsed)
    }
    fn resolve(&self, location: &str) -> Result<Self> {
        let mut next = Self::parse(location)?;
        let absolute = !next.scheme.is_empty() || next.authority.is_some();
        if next.scheme.is_empty() {
            next.scheme.clone_from(&self.scheme);
        }
        if absolute {
            next.path = resolve_path("", &next.path);
        } else {
            if next.path.is_empty() && next.query.is_none() {
                next.query.clone_from(&self.query);
                if next.fragment.is_empty() {
                    next.fragment.clone_from(&self.fragment);
                }
            }
            next.authority.clone_from(&self.authority);
            next.path = resolve_path(&self.path, &next.path);
        }
        Ok(next)
    }
    fn text(&self, credentials: bool) -> String {
        let authority = self.authority.as_deref().unwrap_or("");
        let authority = if credentials {
            authority
        } else {
            authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host)
        };
        let mut text = if let Some(opaque) = &self.opaque {
            format!("{}:{opaque}", self.scheme)
        } else {
            format!("{}://{authority}{}", self.scheme, self.path)
        };
        if let Some(query) = &self.query {
            text.push('?');
            text.push_str(query);
        }
        if !self.fragment.is_empty() {
            text.push('#');
            text.push_str(&self.fragment);
        }
        text
    }
    fn wire(&self) -> Result<String> {
        if self.authority.is_none() {
            return Err(error(
                "wago_http",
                5,
                "unsupported opaque or hostless redirect URL",
            ));
        }
        // URL-derived Basic is supplied from exact bytes by authorization();
        // reqwest must never decode userinfo through its UTF-8-only helper.
        let text = self.text(false);
        let parsed =
            reqwest::Url::parse(&text).map_err(|_| error("wago_http", 5, "invalid Hotfix URL"))?;
        if !matches!(self.scheme.as_str(), "http" | "https") {
            return Err(error("wago_http", 5, "Hotfix URL requires HTTP or HTTPS"));
        }
        // The current reqwest seam cannot send Go's encoded-dot request targets.
        // Reject the mismatch explicitly rather than request a different path.
        // This is a known compatibility gap, not a claim of full URL equivalence.
        let path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        if !same_endpoint(&parsed, self.authority.as_deref().unwrap_or(""))
            || parsed.path() != path
            || parsed.query() != self.query.as_deref()
        {
            return Err(error(
                "wago_http",
                5,
                "Go redirect target cannot be represented by HTTP transport",
            ));
        }
        Ok(text)
    }
    fn authorization(&self) -> Result<Option<String>> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let Some((userinfo, _)) = self.authority.as_deref().and_then(|a| a.rsplit_once('@')) else {
            return Ok(None);
        };
        let (username, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
        let mut bytes = unescape_userinfo(username)?;
        bytes.push(b':');
        bytes.extend_from_slice(&unescape_userinfo(password)?);
        Ok(Some(format!("Basic {}", STANDARD.encode(bytes))))
    }
    fn referer(&self, next: &Self) -> Option<String> {
        if self.scheme == "https" && next.scheme == "http" {
            None
        } else {
            Some(self.text(false))
        }
    }
}
fn unescape_userinfo(raw: &str) -> Result<Vec<u8>> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let pair = bytes
                .get(i + 1..i + 3)
                .filter(|p| p.iter().all(u8::is_ascii_hexdigit))
                .ok_or_else(|| error("wago_http", 5, "invalid userinfo escape"))?;
            let digit = |b: u8| {
                if b <= b'9' {
                    b - b'0'
                } else {
                    b.to_ascii_lowercase() - b'a' + 10
                }
            };
            decoded.push(digit(pair[0]) * 16 + digit(pair[1]));
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Ok(decoded)
}
// Validate Go's parse-time authority grammar without asking WHATWG to choose a
// destination. Numeric port range, DNS/IDNA and unsupported URL checks are later
// transport concerns: rejecting them here would incorrectly skip redirect drains.
fn validate_authority(scheme: &str, authority: &str) -> Result<()> {
    let (userinfo, host) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(user, host)| (Some(user), host));
    if host.rfind('[').is_some_and(|position| position != 0) {
        return Err(error("wago_http", 5, "invalid IP-literal"));
    }
    if host.starts_with('[') {
        let close = host
            .rfind(']')
            .ok_or_else(|| error("wago_http", 5, "missing ']' in host"))?;
        valid_port(&host[close + 1..])?;
        let literal = &host[1..close];
        let (address, zone) = literal
            .split_once("%25")
            .map_or((literal, None), |(ip, zone)| (ip, Some(zone)));
        host_escapes(address, false)?;
        if address.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(error("wago_http", 5, "invalid IP-literal"));
        }
        if let Some(zone) = zone {
            if zone.is_empty() {
                return Err(error("wago_http", 5, "invalid IPv6 zone"));
            }
            host_escapes(zone, true)?;
        }
    } else {
        if let Some(first) = host.find(':') {
            let colon = if matches!(scheme, "http" | "https") {
                first
            } else {
                host.rfind(':').unwrap_or(first)
            };
            valid_port(&host[colon..])?;
        }
        host_escapes(host, false)?;
    }
    // Go parses the host before validating userinfo.
    if let Some(userinfo) = userinfo {
        if !userinfo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:~!$&'()*+,;=%@".contains(&b))
        {
            return Err(error("wago_http", 5, "invalid userinfo"));
        }
        url_component(userinfo, false)?;
    }
    Ok(())
}
fn valid_port(port: &str) -> Result<()> {
    if port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|v| v.bytes().all(|b| b.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(error("wago_http", 5, "invalid port after host"))
    }
}
fn host_byte(b: u8) -> bool {
    b >= 0x80 || b.is_ascii_alphanumeric() || b"-_.~!$&'()*+,;=:[]<>\"".contains(&b)
}
fn host_escapes(host: &str, zone: bool) -> Result<()> {
    let bytes = host.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .filter(|v| v.iter().all(u8::is_ascii_hexdigit))
                .ok_or_else(|| error("wago_http", 5, "invalid URL escape in host"))?;
            let digit = |b: u8| {
                if b <= b'9' {
                    b - b'0'
                } else {
                    b.to_ascii_lowercase() - b'a' + 10
                }
            };
            let decoded = digit(hex[0]) * 16 + digit(hex[1]);
            let valid = if zone {
                decoded == b'%' || decoded == b' ' || host_byte(decoded)
            } else {
                decoded >= 0x80 || decoded == b'%'
            };
            if !valid {
                return Err(error("wago_http", 5, "invalid URL escape in host"));
            }
            i += 3;
        } else {
            if !host_byte(bytes[i]) {
                return Err(error("wago_http", 5, "invalid character in host"));
            }
            i += 1;
        }
    }
    Ok(())
}
fn same_endpoint(parsed: &reqwest::Url, authority: &str) -> bool {
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let actual = parsed.host_str().unwrap_or("");
    if let Some(literal) = host.strip_prefix('[') {
        let Some((literal, _)) = literal.split_once(']') else {
            return false;
        };
        return literal
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .zip(
                actual
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<std::net::Ipv6Addr>()
                    .ok(),
            )
            .is_some_and(|(expected, actual)| expected == actual);
    }
    let host = host.split_once(':').map_or(host, |(host, _)| host);
    // ASCII DNS case folding does not select a different endpoint. WHATWG's
    // IPv4 shorthand/octal/hex conversion does; reject rather than contact it.
    // IDNA/escaped hosts remain explicit representation gaps for this adapter.
    host.is_ascii() && host.eq_ignore_ascii_case(actual)
}
fn url_component(raw: &str, fragment: bool) -> Result<String> {
    // Go keeps valid RawPath/RawFragment verbatim; when a raw space, Unicode or
    // backslash requires re-escaping, it re-escapes the entire decoded component.
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut valid_raw = true;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .filter(|s| s.iter().all(u8::is_ascii_hexdigit))
                .ok_or_else(|| error("wago_http", 5, "invalid URL escape"))?;
            let digit = |b: u8| {
                if b <= b'9' {
                    b - b'0'
                } else {
                    b.to_ascii_lowercase() - b'a' + 10
                }
            };
            decoded.push(digit(hex[0]) * 16 + digit(hex[1]));
            i += 3;
        } else {
            valid_raw &= b.is_ascii_alphanumeric()
                || b"-_.~!$&'()*+,;=:@[]/".contains(&b)
                || (fragment && b == b'?');
            decoded.push(b);
            i += 1;
        }
    }
    if valid_raw {
        return Ok(raw.to_owned());
    }
    let mut escaped = String::new();
    for b in decoded {
        if b.is_ascii_alphanumeric()
            || b"-_.~$&+,/:;=@".contains(&b)
            || (fragment && b"?!()*".contains(&b))
        {
            escaped.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(escaped, "%{b:02X}");
        }
    }
    Ok(escaped)
}
fn resolve_path(base: &str, reference: &str) -> String {
    let full = if reference.is_empty() {
        base.to_owned()
    } else if reference.starts_with('/') {
        reference.to_owned()
    } else {
        format!(
            "{}{reference}",
            base.rsplit_once('/')
                .map_or("", |(prefix, _)| &base[..prefix.len() + 1])
        )
    };
    if full.is_empty() {
        return full;
    }
    let mut result = String::from("/");
    let mut first = true;
    let mut last = "";
    for segment in full.split('/') {
        last = segment;
        match segment {
            "." => {
                first = false;
            }
            ".." => {
                let end = result[1..].rfind('/').map_or(1, |i| i + 1);
                result.truncate(end);
                first = result.len() == 1;
            }
            _ => {
                if !first {
                    result.push('/');
                }
                result.push_str(segment);
                first = false;
            }
        }
    }
    if matches!(last, "." | "..") {
        result.push('/');
    }
    if result.starts_with("//") {
        result.remove(0);
    }
    result
}
#[cfg(test)]
mod http_tests;
fn escaped(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            use std::fmt::Write;
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}
fn url(base: &str, search: &str, page: i64) -> String {
    let mut s = format!("{}/hotfixes?page={page}", base.trim_end_matches('/'));
    if !search.is_empty() {
        s.push_str("&search=");
        s.push_str(&escaped(search));
    }
    s
}
fn corrupted(e: ToolkitError) -> ToolkitError {
    if matches!(
        e.code.as_str(),
        "command.cancelled" | "command.deadline" | "operation.cancelled"
    ) {
        e
    } else {
        error("cache_corrupt", 4, e.message)
    }
}
fn load(
    inv: &Invocation,
    identity: &Identity,
    budget: &Budget,
) -> Result<Option<(Receipt, Vec<u8>)>> {
    inv.control.check()?;
    let key = format!("hotfix-wago-page/{}", identity.key(budget)?);
    let raw = match Metadata::read_raw_controlled(&inv.home, &key, &inv.control) {
        Ok(v) => v,
        Err(e) if e.code == "vault.record_missing" => return Ok(None),
        Err(e) => return Err(e),
    };
    let receipt: Receipt = serde_json::from_slice(&raw).map_err(|_| {
        error(
            "cache_corrupt",
            4,
            format!("receipt identity mismatch for page {}", identity.page),
        )
    })?;
    if receipt.schema != CACHE_SCHEMA || receipt.identity.key(budget)? != identity.key(budget)? {
        return Err(error(
            "cache_corrupt",
            4,
            format!("receipt identity mismatch for page {}", identity.page),
        ));
    }
    let raw = evidence::read_blob_bounded(&inv.home, &receipt.response, PAGE_BYTES, &inv.control)
        .map_err(corrupted)?;
    Ok(Some((receipt, raw)))
}
fn store(
    inv: &Invocation,
    identity: Identity,
    request_url: String,
    raw: &[u8],
    page: &parse::Page,
    captured_at: String,
    budget: &Budget,
) -> Result<Receipt> {
    inv.control.check()?;
    let blob =
        evidence::publish_blob_bounded(&inv.home, raw, PAGE_BYTES, &inv.control).map_err(|e| {
            if e.exit_code == 7 {
                e
            } else {
                error("cache_write", 5, e.message)
            }
        })?;
    let key = format!("hotfix-wago-page/{}", identity.key(budget)?);
    let receipt = Receipt {
        schema: CACHE_SCHEMA.into(),
        identity,
        request_url,
        response: blob,
        captured_at,
        current_page: page.current_page,
        last_page: page.last_page,
        per_page: page.per_page,
        total: page.total,
        next_page_url: page.next_page_url.clone(),
    };
    // The fixed Go store reads one generation and updates only that generation.
    // If another writer wins the CAS, only identical content can be accepted.
    let observed = match Metadata::read_document_raw_controlled(&inv.home, &key, &inv.control) {
        Ok(v) => Some(v),
        Err(e) if e.code == "vault.record_missing" => None,
        Err(e) => return Err(e),
    };
    if let Some(document) = &observed {
        if let Ok(existing) = serde_json::from_slice::<Receipt>(&document.bytes) {
            if existing.schema == CACHE_SCHEMA
                && existing.identity.key(budget)? == receipt.identity.key(budget)?
                && existing.response == receipt.response
            {
                return Ok(receipt);
            }
        }
    }
    let mut metadata = Metadata::open(&inv.home, true)?;
    match metadata.put_controlled(
        &key,
        &receipt,
        observed.as_ref().map(|v| v.generation),
        &inv.control,
    ) {
        Ok(()) => {}
        Err(e) if e.code == "vault.generation_conflict" => {
            let winner = Metadata::read_document_raw_controlled(&inv.home, &key, &inv.control);
            inv.control.check()?;
            let same = winner
                .ok()
                .and_then(|d| serde_json::from_slice::<Receipt>(&d.bytes).ok())
                .filter(|r| r.schema == CACHE_SCHEMA && r.response == receipt.response)
                .is_some_and(|r| r.identity.key(budget).ok() == receipt.identity.key(budget).ok());
            if !same {
                return Err(error(
                    "page_drift",
                    4,
                    "concurrent pages differ for one page identity",
                ));
            }
        }
        Err(e) => {
            return Err(
                if matches!(e.code.as_str(), "command.cancelled" | "command.deadline") {
                    e
                } else {
                    error("cache_write", 5, e.message)
                },
            );
        }
    }
    Ok(receipt)
}
fn read_page<'a>(
    inv: &Invocation,
    identity: Identity,
    state: &mut FetchBudget,
    budget: &'a Budget,
    fetcher: &impl Fetcher,
    base: &str,
) -> Result<Option<Fetched<'a>>> {
    let (receipt, raw, from_cache) = if let Some((receipt, raw)) = load(inv, &identity, budget)? {
        (receipt, raw, true)
    } else {
        if inv.flag_bool("--offline") {
            return Err(error(
                "offline",
                3,
                format!("page {} is not in the verified cache", identity.page),
            ));
        }
        if state.requests >= state.max_requests || state.bytes >= state.max_bytes {
            return Ok(None);
        }
        let request_url = url(base, &identity.search, identity.page);
        let raw = fetcher.fetch(&request_url, &inv.control, budget)?;
        let captured_at = storage::utc_now();
        if raw.len() > PAGE_BYTES {
            return Err(error(
                "budget",
                3,
                format!("page exceeds {PAGE_BYTES} bytes"),
            ));
        }
        state.requests += 1;
        state.bytes += raw.len() as i64;
        if state.bytes > state.max_bytes {
            return Err(error(
                "budget",
                3,
                format!("byte budget {} exhausted", state.max_bytes),
            ));
        }
        let page = parse::parse(&raw, budget, &inv.control)?;
        if page.current_page != identity.page {
            return Err(error(
                "page_drift",
                4,
                format!(
                    "server returned page {} for page {}",
                    page.current_page, identity.page
                ),
            ));
        }
        let receipt = store(
            inv,
            identity.clone(),
            request_url,
            &raw,
            &page,
            captured_at,
            budget,
        )?;
        (receipt, raw, false)
    };
    let page = parse::parse(&raw, budget, &inv.control)
        .map_err(|e| if from_cache { corrupted(e) } else { e })?;
    if page.current_page != identity.page {
        return Err(error(
            "cache_corrupt",
            4,
            format!(
                "cached page {} stored for page {}",
                page.current_page, identity.page
            ),
        ));
    }
    let meta = CursorPage {
        page: page.current_page,
        key: identity.key(budget)?,
        sha256: receipt.response["sha256"]
            .as_str()
            .unwrap_or_default()
            .into(),
        total: page.total,
        last_page: page.last_page,
        next_page_url: page.next_page_url.clone(),
    };
    Ok(Some(Fetched {
        page,
        meta,
        captured_at: receipt.captured_at,
        from_cache,
    }))
}
fn option(inv: &Invocation, key: &str, default: i64) -> Result<i64> {
    let v = inv
        .flag(key)
        .map(str::parse::<i64>)
        .transpose()
        .map_err(|_| error("budget", 3, "budgets must be positive"))?
        .unwrap_or(0);
    Ok(if v == 0 { default } else { v })
}
pub(super) fn query(
    inv: &Invocation,
    q: Query,
    configured: FetchBudget,
    budget: &Budget,
    fetcher: &impl Fetcher,
    base: &str,
) -> Result<RemoteResult> {
    q.validate()?;
    if q.table_hash.is_some() {
        return Err(error(
            "filter_unsupported",
            2,
            "wago rows carry table names; select the table name instead",
        ));
    }
    let max_pages = option(inv, "--max-pages", 2048)?;
    if !(1..=2048).contains(&max_pages) {
        return Err(error("budget", 3, "max pages must be 1..2048"));
    }
    let mut state = FetchBudget {
        max_requests: if configured.max_requests == 0 {
            max_pages
        } else {
            configured.max_requests
        },
        max_bytes: if configured.max_bytes == 0 {
            256 << 20
        } else {
            configured.max_bytes
        },
        ..Default::default()
    };
    if state.max_requests < 1 || state.max_bytes < 1 {
        return Err(error("budget", 3, "budgets must be positive"));
    }
    let cursor = if q.cursor.is_empty() {
        None
    } else {
        Some(Cursor::decode(&q.cursor, &q, "wago")?)
    };
    let mut start = if q.page == 0 { 1 } else { q.page };
    let mut after = String::new();
    let mut anchor = None;
    let mut metas = BTreeMap::new();
    if let Some(c) = &cursor {
        if let Some(m) = &c.current {
            metas.insert(m.page, m.clone());
        }
        if let Some(m) = &c.previous {
            metas.insert(m.page, m.clone());
        }
        after = c.after.clone();
        if !q.latest {
            if q.page != 0 {
                return Err(error(
                    "query",
                    2,
                    "a resume cursor already fixes the page position",
                ));
            }
            start = c.resume_page;
            anchor = c.current.clone();
        }
    }
    if start < 1 {
        return Err(error("cursor", 2, "cursor has no resume page"));
    }
    let mut chain = cursor
        .as_ref()
        .map(|c| c.reference.clone())
        .unwrap_or_else(|| CURSOR_SCHEMA.into());
    let mut page_number = start;
    let mut walked = 0i64;
    let mut scanned = 0usize;
    let mut matched = 0usize;
    let mut hits = 0i64;
    let mut window: Vec<(Record, i64)> = Vec::new();
    let mut unknown = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut consumed = after.is_empty();
    let mut stop = "";
    let mut last: Option<CursorPage> = None;
    let mut captured = String::new();
    loop {
        budget.check()?;
        if walked >= max_pages {
            stop = "page_budget";
            break;
        }
        let Some(fetched) = read_page(
            inv,
            Identity::new(&q, page_number),
            &mut state,
            budget,
            fetcher,
            base,
        )?
        else {
            stop = "page_budget";
            break;
        };
        let page = &fetched.page;
        if walked == 0 {
            if let Some(a) = &anchor {
                if fetched.meta.sha256 != a.sha256 || fetched.meta.key != a.key {
                    return Err(error(
                        "page_drift",
                        4,
                        format!("resumed page {page_number} bytes changed"),
                    ));
                }
            } else if let Some(previous) = metas.get(&(page.current_page - 1)) {
                parse::sequence(previous, page)?;
            }
            if q.latest {
                let remaining = page.last_page - page_number + 1;
                if remaining > max_pages {
                    return Err(error(
                        "budget",
                        3,
                        format!(
                            "candidate set requires {remaining} pages from page {page_number}, limit is {max_pages}"
                        ),
                    ));
                }
            }
        } else {
            parse::sequence(last.as_ref().unwrap(), page)?;
        }
        for r in &page.records {
            budget.check()?;
            if r.id != 0 && seen.contains(&r.id) {
                return Err(error(
                    "page_drift",
                    4,
                    format!("record id {} repeats across pages", r.id),
                ));
            }
            if r.id != 0 {
                budget.charge(Cost::retained(64))?;
                seen.insert(r.id);
            }
        }
        budget.charge(Cost::retained(
            size_of::<CursorPage>()
                + fetched.meta.key.len()
                + fetched.meta.sha256.len()
                + fetched.meta.next_page_url.len()
                + 64,
        ))?;
        scanned += page.records.len();
        let mut full = false;
        for record in fetched.page.records {
            budget.check()?;
            if !(1..=4).contains(&record.status) {
                unknown.insert(record.status);
            }
            if !q.matches(&record)? {
                continue;
            }
            matched += 1;
            if !consumed {
                if record.key() == after {
                    consumed = true;
                }
                continue;
            }
            let bytes = record.payload.as_ref().map_or(0, |p| p.get().len())
                + record.table_name.len()
                + record.build.len()
                + record.locale.len()
                + record.created_at.len()
                + size_of::<Record>();
            budget.charge(Cost::retained(bytes))?;
            window.push((record, page_number));
            if !q.latest && window.len() > q.limit {
                full = true;
                break;
            }
        }
        walked += 1;
        if fetched.from_cache {
            hits += 1;
        }
        let stamp = model::parse_time(&fetched.captured_at).map_err(corrupted)?;
        if captured.is_empty() || stamp > model::parse_time(&captured)? {
            captured = fetched.captured_at;
        }
        chain = storage::sha256(
            format!("{chain}\n{}\n{}", fetched.meta.key, fetched.meta.sha256).as_bytes(),
        );
        metas.insert(fetched.meta.page, fetched.meta.clone());
        last = Some(fetched.meta);
        if full {
            stop = "limit";
            break;
        }
        if last.as_ref().unwrap().page == last.as_ref().unwrap().last_page {
            break;
        }
        page_number += 1;
    }
    let last = last.ok_or_else(|| error("budget", 3, "no page could be read"))?;
    if !consumed {
        return Err(error(
            "cursor",
            2,
            format!("cursor record {after:?} is not in the resumed source"),
        ));
    }
    // Go resolves continuation by the first matching composite key in the original window.
    // ID0 records may legitimately repeat that key across pages.
    let mut first_positions = std::collections::BTreeMap::new();
    for (record, page) in &window {
        let key = record.key();
        if !first_positions.contains_key(&key) {
            budget.charge(Cost::retained(key.len() + 64))?;
            first_positions.insert(key, *page);
        }
    }
    let mut selected = None;
    if q.latest {
        if stop == "page_budget" {
            return Err(error(
                "budget",
                3,
                "the latest batch needs the full candidate range",
            ));
        }
        selected = window.iter().map(|(r, _)| r.push).max();
        window.retain(|(r, _)| Some(r.push) == selected);
        stop = if window.len() > q.limit { "limit" } else { "" };
    }
    let mut next = String::new();
    if !stop.is_empty() {
        let (resume, after, current, previous) = if stop == "limit" && !window.is_empty() {
            let (r, _) = &window[q.limit - 1];
            let key = r.key();
            let p = first_positions[&key];
            (p, key, metas.get(&p).cloned(), metas.get(&(p - 1)).cloned())
        } else {
            (last.page + 1, String::new(), None, Some(last.clone()))
        };
        next = Cursor {
            source: "wago".into(),
            reference: chain.clone(),
            scope: q.scope_digest()?,
            resume_page: resume,
            after,
            current,
            previous,
            ..Default::default()
        }
        .encode(budget)?;
    }
    window.truncate(q.limit);
    let mut records = window.into_iter().map(|(r, _)| r).collect::<Vec<_>>();
    if let Some(definition) = &q.definition {
        super::decode::apply(&mut records, definition, budget)?;
    }
    let truncated = !stop.is_empty();
    let complete = !truncated && last.page == last.last_page;
    let mut coverage = Coverage::new("wago", &q);
    coverage.complete = complete;
    coverage.full_query_complete = complete && start == 1 && cursor.is_none();
    coverage.truncated = truncated;
    coverage.truncation_reason = stop.into();
    coverage.start_page = start;
    coverage.end_page = last.page;
    coverage.last_page = last.last_page;
    coverage.total = last.total;
    coverage.scanned = scanned;
    coverage.matched = matched;
    coverage.returned = records.len();
    coverage.selected_push = selected;
    coverage.captured_at = Some(captured);
    coverage.reference = chain;
    coverage.fetch = Some(state);
    let mut result = RemoteResult::new("wago", q, records, coverage, next)?;
    result.warnings = unknown
        .into_iter()
        .map(|s| format!("records.hotfix_unknown_status:{s}"))
        .collect();
    result.warnings.sort();
    if hits == walked {
        result.warnings.push("records.wago_cache_hit".into());
    }
    if matched == 0 && scanned > 0 {
        result.warnings.push("records.wago_search_miss".into());
    }
    Ok(result)
}
