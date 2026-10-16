//! Go net/http has distinct wire Host, dial endpoint and TLS verification names.
//! Keep those identities separate without WHATWG numeric-host interpretation.
use super::*;
use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};

#[derive(Clone, Debug)]
pub(in crate::remote::transport) struct Target {
    pub wire: Uri,
    pub dial_host: String,
    pub dial_port: String,
    pub dial: Option<Uri>,
    pub tls_host: String,
    pub connect_host: String,
    pub connect_target: String,
    pub route: String,
}
impl Target {
    pub(super) fn parse(locator: &str) -> Result<Self> {
        let parsed = redirect_url::RedirectUrl::absolute(locator)?;
        if !matches!(parsed.scheme.as_str(), "http" | "https") {
            return Err(http("remote URL requires HTTP or HTTPS"));
        }
        let authority = parsed.authority.as_deref().unwrap_or_default();
        if authority.contains('@') {
            return Err(http("raw URI requires separate authorization"));
        }
        let (host, port, bracketed) = if let Some(rest) = authority.strip_prefix('[') {
            let (host, port) = rest
                .split_once(']')
                .ok_or_else(|| http("invalid IPv6 authority"))?;
            (host, port.strip_prefix(':'), true)
        } else {
            let (host, port) = authority
                .split_once(':')
                .map_or((authority, None), |(h, p)| (h, Some(p)));
            (host, port, false)
        };
        let raw = redirect_url::unescape_userinfo(host)?;
        let host = std::str::from_utf8(&raw).map_err(|_| http("invalid UTF-8 remote hostname"))?;
        let default = if parsed.scheme == "https" {
            "443"
        } else {
            "80"
        };
        let dial_port = port.filter(|p| !p.is_empty()).unwrap_or(default);
        let lookup = if host.is_ascii() {
            host.to_owned()
        } else {
            // Transport's idnaASCIIFromURL falls back to the original name on a
            // Lookup error. Do not replace that name with a different endpoint.
            Uts46::new()
                .to_ascii(
                    host.as_bytes(),
                    AsciiDenyList::STD3,
                    Hyphens::Check,
                    DnsLength::Ignore,
                )
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| host.to_owned())
        };
        let wire_host = if bracketed {
            host.split('%').next().unwrap_or(host).to_owned()
        } else {
            punycode_host(host)?
        };
        let tls_host = if bracketed {
            lookup.split('%').next().unwrap_or(&lookup)
        } else {
            &lookup
        };
        let authority = |name: &str, port: Option<&str>| {
            let name = if bracketed {
                format!("[{name}]")
            } else {
                name.to_owned()
            };
            port.map_or(name.clone(), |port| format!("{name}:{port}"))
        };
        let mut path = if parsed.path.is_empty() {
            "/".to_owned()
        } else {
            parsed.path
        };
        if let Some(query) = parsed.query {
            path.push('?');
            path.push_str(&query);
        }
        let uri = |authority: String| {
            format!("{}://{authority}{path}", parsed.scheme)
                .parse::<Uri>()
                .map_err(|_| http("invalid remote HTTP authority or target"))
        };
        Ok(Self {
            wire: uri(authority(
                &wire_host,
                if !host.is_ascii() && port == Some("") {
                    None
                } else {
                    port
                },
            ))?,
            // HTTP Uri cannot carry a Unicode Lookup fallback. Direct DNS still
            // receives that exact name via ExactName; do not substitute wire
            // Punycode, which is a different endpoint in this case.
            dial: uri(authority(&lookup, Some(dial_port))).ok(),
            dial_host: lookup.clone(),
            dial_port: dial_port.to_owned(),
            tls_host: tls_host.to_owned(),
            connect_target: authority(&lookup, Some(dial_port)),
            connect_host: authority(
                &if bracketed {
                    tls_host.to_owned()
                } else {
                    punycode_host(&lookup)?
                },
                Some(dial_port),
            ),
            route: format!("{}://{}{path}", parsed.scheme, authority(host, port)),
        })
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
