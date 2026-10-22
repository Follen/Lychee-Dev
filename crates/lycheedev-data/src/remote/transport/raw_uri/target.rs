// SPDX-License-Identifier: BSD-3-Clause
// Copyright 2009 The Go Authors.
// Go compatibility rules; complete notice retained in ../GO-LICENSE.
//! Fixed Go wire Host, canonical endpoint and TLS identities. A private URI
//! placeholder is plumbing only; no placeholder can become a dial destination.
use super::*;
use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};

#[derive(Clone, Debug)]
pub(in crate::remote::transport) struct Target {
    pub wire: Uri,
    pub host_header: String,
    pub forward_target: String,
    pub projection: bool,
    pub write_error: Option<&'static str>,
    pub invalid_host: bool,
    pub dial_host: String,
    pub dial_bytes: Vec<u8>,
    pub dial_port: String,
    pub tls_host: String,
    pub connect_host: String,
    pub connect_target: String,
    pub connect_error: Option<&'static str>,
    pub route: String,
}
impl Target {
    pub(super) fn parse(locator: &str) -> Result<Self> {
        let parsed = redirect_url::RedirectUrl::absolute(locator)?;
        if !matches!(parsed.scheme.as_str(), "http" | "https") {
            return Err(http("remote URL requires HTTP or HTTPS"));
        }
        let original_authority = parsed.authority.as_deref().unwrap_or_default();
        if original_authority.contains('@') {
            return Err(http("raw URI requires separate authorization"));
        }
        let (host, port, bracketed) = if let Some(rest) = original_authority.strip_prefix('[') {
            let (host, port) = rest
                .rsplit_once(']')
                .ok_or_else(|| http("invalid IPv6 authority"))?;
            (host, port.strip_prefix(':'), true)
        } else {
            let (host, port) = original_authority
                .split_once(':')
                .map_or((original_authority, None), |(h, p)| (h, Some(p)));
            (host, port, false)
        };
        let raw = redirect_url::unescape_userinfo(host)?;
        let default = if parsed.scheme == "https" {
            "443"
        } else {
            "80"
        };
        let dial_port = port.filter(|p| !p.is_empty()).unwrap_or(default);
        let lookup = if raw.is_ascii() {
            raw.clone()
        } else {
            Uts46::new()
                .to_ascii(&raw, AsciiDenyList::STD3, Hyphens::Check, DnsLength::Ignore)
                .map(|s| s.into_owned().into_bytes())
                .unwrap_or_else(|_| raw.clone())
        };
        // Go's Windows GetAddrInfoW receives UTF16FromString, which replaces
        // each invalid UTF-8 byte with RuneError. SOCKS retains the raw bytes.
        let dial_host = windows_lossy_text(&lookup);
        // Direct Go addTLS receives SplitHostPort's host including its zone.
        // The tunneled layer adds brackets separately via connectMethod.tlsHost.
        let tls_host = dial_host.clone();
        let authority = |name: &[u8], port: Option<&str>| {
            let mut out = Vec::new();
            if bracketed {
                out.push(b'[');
            }
            out.extend_from_slice(name);
            if bracketed {
                out.push(b']');
            }
            if let Some(port) = port {
                out.push(b':');
                out.extend_from_slice(port.as_bytes());
            }
            out
        };
        let raw_authority = authority(&raw, port);
        let wire_host = punycode_authority(&raw_authority, &raw, port);
        let write_error = wire_host.as_ref().err().copied();
        let raw_wire_host = wire_host.unwrap_or_default();
        // Go validates Host before removing a zone. Invalid direct Host becomes
        // empty; forward-proxy writes reject it after acquiring the connection.
        let invalid_host = !valid_host(&raw_wire_host);
        let host_header = if invalid_host {
            String::new()
        } else {
            remove_zone(&raw_wire_host)
        };
        let canonical = authority(&lookup, Some(dial_port));
        let connect_host = punycode_authority(&canonical, &lookup, Some(dial_port));
        let connect_error = connect_host.as_ref().err().copied();
        let connect_host = connect_host
            .map(|host| {
                if valid_host(&host) {
                    remove_zone(&host)
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        let mut path = if parsed.path.is_empty() {
            "/".to_owned()
        } else {
            parsed.path
        };
        if let Some(query) = parsed.query {
            path.push('?');
            path.push_str(&query);
        }
        let forward_target = format!("{}://{host_header}{path}", parsed.scheme);
        let represented = forward_target
            .parse::<Uri>()
            .ok()
            .filter(|u| u.host().is_some_and(|host| !host.is_empty()));
        let projection = represented.is_none();
        let wire = represented.unwrap_or(
            format!("{}://lycheedev-wire.invalid{path}", parsed.scheme)
                .parse()
                .map_err(|_| http("invalid HTTP request target"))?,
        );
        Ok(Self {
            wire,
            host_header,
            forward_target,
            projection,
            write_error,
            invalid_host,
            dial_host,
            dial_bytes: lookup,
            dial_port: dial_port.to_owned(),
            tls_host,
            connect_host,
            connect_target: windows_lossy_text(&canonical),
            connect_error,
            route: format!(
                "{}://{}{path}",
                parsed.scheme,
                windows_lossy_text(&raw_authority)
            ),
        })
    }
}
// This is net/http/httpguts.PunycodeHostPort's actual split/whole-authority
// distinction, including non-ASCII IPv6 zones and an explicit empty port.
fn punycode_authority(
    raw: &[u8],
    host: &[u8],
    port: Option<&str>,
) -> std::result::Result<String, &'static str> {
    if raw.is_ascii() {
        return Ok(String::from_utf8(raw.to_vec()).unwrap());
    }
    let (bytes, port) = port.map_or((raw, ""), |port| (host, port));
    let text = std::str::from_utf8(bytes).map_err(|_| "invalid punycode host label")?;
    let host = punycode_host(text).map_err(|_| "invalid punycode host label")?;
    if port.is_empty() {
        return Ok(host);
    }
    Ok(if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    })
}
fn valid_host(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"!$%&()*+,-.:;='[]_~".contains(&b))
}
fn remove_zone(value: &str) -> String {
    if value.starts_with('[')
        && let Some(end) = value.rfind(']')
        && let Some(start) = value[..end].rfind('%')
    {
        return format!("{}{}", &value[..start], &value[end..]);
    }
    value.to_owned()
}
fn windows_lossy_text(mut bytes: &[u8]) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(bytes) {
            Ok(rest) => {
                out.push_str(rest);
                return out;
            }
            Err(error) => {
                let n = error.valid_up_to();
                out.push_str(std::str::from_utf8(&bytes[..n]).unwrap());
                out.push('\u{fffd}');
                bytes = &bytes[n + 1..];
            }
        }
    }
}
fn punycode_host(host: &str) -> Result<String> {
    // httpguts.PunycodeHostPort deliberately skips IDNA processing for an
    // entirely ASCII authority. Non-ASCII uses the raw Punycode profile, not
    // the Lookup profile used for dialing. The codec remains the official idna
    // crate; these are just Go's label/profile selection rules.
    if host.is_ascii() {
        return Ok(host.to_owned());
    }
    if host.contains('\u{fffd}') {
        return Err(http("invalid punycode host label"));
    }
    host.split('.')
        .map(|label| {
            let decoded;
            let label = if let Some(encoded) = label.strip_prefix("xn--") {
                decoded = idna::punycode::decode_to_string(encoded)
                    .ok_or_else(|| http("invalid punycode host label"))?;
                if !decoded.is_empty() && decoded.is_ascii() {
                    return Err(http("invalid punycode host label"));
                }
                decoded.as_str()
            } else {
                label
            };
            if label.contains('\u{fffd}') {
                return Err(http("invalid punycode host label"));
            }
            if label.is_ascii() {
                Ok(label.to_owned())
            } else {
                idna::punycode::encode_str(label)
                    .map(|v| format!("xn--{v}"))
                    .ok_or_else(|| http("invalid punycode host label"))
            }
        })
        .collect::<Result<Vec<_>>>()
        .map(|labels| labels.join("."))
}
