// SPDX-License-Identifier: BSD-3-Clause
use lycheedev_core::{Result, ToolkitError};
// Preserve the original Wago diagnostic contract. Other consumers map this
// local parser error at their boundary without changing redirect ordering.
fn error(kind: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    let code = format!("records.hotfix_{kind}");
    crate::fault(&code, exit, format!("{code}: {}", message.into()))
}
// A deliberately local redirect reference, not a second HTTP implementation.
// Keep Go's authority spelling, raw query, fragment inheritance and RFC3986
// path resolution. WHATWG Url::join changes backslashes and encoded dot segments.
// The resolve_path algorithm follows Go net/url (Go BSD license in the shared
// remote/transport/GO-LICENSE); this is shared only by Wago and pinned public-key redirects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedirectUrl {
    pub(crate) scheme: String,
    pub(crate) opaque: Option<String>,
    pub(crate) authority: Option<String>,
    pub(crate) path: String,
    pub(crate) query: Option<String>,
    pub(crate) fragment: String,
}
impl RedirectUrl {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
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
    pub(crate) fn absolute(raw: &str) -> Result<Self> {
        let parsed = Self::parse(raw)?;
        if parsed.scheme.is_empty() || parsed.authority.is_none() {
            return Err(error("wago_http", 5, "Hotfix URL requires HTTP or HTTPS"));
        }
        Ok(parsed)
    }
    pub(crate) fn resolve(&self, location: &str) -> Result<Self> {
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
    pub(crate) fn text(&self, credentials: bool) -> String {
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
    pub(crate) fn wire(&self) -> Result<String> {
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
    pub(crate) fn authorization(&self) -> Result<Option<String>> {
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
    pub(crate) fn referer(&self, next: &Self) -> Option<String> {
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
pub(crate) fn validate_authority(scheme: &str, authority: &str) -> Result<()> {
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
pub(crate) fn url_component(raw: &str, fragment: bool) -> Result<String> {
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
