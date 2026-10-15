// SPDX-License-Identifier: MIT
//! Go environment-proxy policy for explicitly admitted HTTP(S) requests. Sources are frozen in the R4
//! upstream manifest (Go 1.27.1 httpproxy/proxy.go and net/url/url.go).
//! Do not use reqwest's system matcher: ALL_PROXY and NO_PROXY semantics differ.
use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};
use lycheedev_core::{Result, ToolkitError};
use reqwest::{ClientBuilder, Proxy, Url};
use std::net::IpAddr;

pub(super) fn install(builder: ClientBuilder, locator: &str) -> Result<ClientBuilder> {
    let is_http = locator.starts_with("http://");
    let policy = Policy {
        proxy: env_first("HTTPS_PROXY", "https_proxy")?,
        http_proxy: if is_http {
            env_first("HTTP_PROXY", "http_proxy")?
        } else {
            String::new()
        },
        cgi: is_http && std::env::var_os("REQUEST_METHOD").is_some_and(|v| !v.is_empty()),
        bypass: env_first("NO_PROXY", "no_proxy")?
            .split(',')
            .filter_map(Rule::parse)
            .collect(),
    };
    let builder = builder.no_proxy();
    match policy.select(locator)? {
        None => Ok(builder),
        Some(url) => Ok(builder.proxy(Proxy::all(url).map_err(|_| configuration_error())?)),
    }
}

fn configuration_error() -> ToolkitError {
    // Neither upstream error text nor the environment value may expose userinfo.
    crate::fault(
        "records.remote_http",
        5,
        "unsupported or invalid HTTP proxy configuration",
    )
}

fn env_first(upper: &str, lower: &str) -> Result<String> {
    env_first_with(upper, lower, |name| match std::env::var_os(name) {
        None => Ok(String::new()),
        Some(value) => value.into_string().map_err(|_| configuration_error()),
    })
}

fn env_first_with(
    upper: &str,
    lower: &str,
    mut get: impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    for name in [upper, lower] {
        let value = get(name)?;
        if !value.is_empty() {
            return Ok(value);
        }
    }
    Ok(String::new())
}

struct Policy {
    proxy: String,
    http_proxy: String,
    cgi: bool,
    bypass: Vec<Rule>,
}

impl Policy {
    fn new(proxy: String, no_proxy: &str) -> Self {
        Self {
            proxy,
            http_proxy: String::new(),
            cgi: false,
            bypass: no_proxy.split(',').filter_map(Rule::parse).collect(),
        }
    }

    fn select(&self, locator: &str) -> Result<Option<Url>> {
        let (rest, proxy, default_port) = if let Some(rest) = locator.strip_prefix("https://") {
            (rest, &self.proxy, "443")
        } else if let Some(rest) = locator.strip_prefix("http://") {
            if self.cgi
                && parsed_proxy(&self.http_proxy, &format!("http://{}", self.http_proxy)).is_some()
            {
                return Err(crate::fault(
                    "records.remote_http",
                    5,
                    "refusing to use HTTP_PROXY value in CGI environment; see golang.org/s/cgihttpproxy",
                ));
            }
            (rest, &self.http_proxy, "80")
        } else {
            return Ok(None);
        };
        if proxy.is_empty() {
            return Ok(None);
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        // URL userinfo belongs to the origin request, never proxy routing.
        let authority = authority.rsplit('@').next().unwrap_or_default();
        let (host, port) =
            target_host_port(authority, default_port).ok_or_else(configuration_error)?;
        let host = ascii_host(host);
        let ip = target_ip(&host);
        if host == "localhost" || ip.is_some_and(|ip| normalized_ip(ip).is_loopback()) {
            return Ok(None);
        }
        let host = go_lower(go_trim(&host));
        if self.bypass.iter().any(|rule| rule.matches(&host, port, ip)) {
            return Ok(None);
        }
        selected_proxy(proxy)
    }
}

fn target_host_port<'a>(authority: &'a str, default_port: &'a str) -> Option<(&'a str, &'a str)> {
    if authority.contains('@') || authority.is_empty() {
        return None;
    }
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, rest) = bracketed.split_once(']')?;
        let port = if rest.is_empty() {
            default_port
        } else {
            rest.strip_prefix(':')?
        };
        return Some((host, if port.is_empty() { default_port } else { port }));
    }
    match authority.split_once(':') {
        Some((host, port)) => Some((host, if port.is_empty() { default_port } else { port })),
        None => Some((authority, default_port)),
    }
}

fn target_ip(host: &str) -> Option<IpAddr> {
    // Go netip.ParseAddr accepts IPv6 zones; net.IP(nip.AsSlice()) drops the zone.
    let host = if host.contains(':') {
        host.split('%').next()?
    } else {
        host
    };
    host.parse().ok()
}

fn normalized_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}

enum Rule {
    All,
    Network {
        address: IpAddr,
        bits: u8,
    },
    Ip {
        address: IpAddr,
        port: String,
    },
    Domain {
        suffix: String,
        apex: bool,
        port: String,
    },
}

impl Rule {
    fn parse(value: &str) -> Option<Self> {
        let value = go_lower(go_trim(value));
        if value.is_empty() {
            return None;
        }
        if value == "*" {
            return Some(Self::All);
        }
        if let Some((address, bits)) = value.split_once('/') {
            if let (Ok(address), Ok(bits)) = (address.parse::<IpAddr>(), bits.parse::<u8>()) {
                // Go accepts only decimal prefix lengths, not signs.
                if !bits_string_is_decimal(&value) {
                    return None;
                }
                if bits <= if address.is_ipv4() { 32 } else { 128 } {
                    return Some(Self::Network { address, bits });
                }
            }
        }
        let (host, port) = split_rule_host_port(&value).unwrap_or((&value, ""));
        if host.is_empty() {
            return None;
        }
        if let Ok(address) = host.parse::<IpAddr>() {
            return Some(Self::Ip {
                address: normalized_ip(address),
                port: port.into(),
            });
        }
        let host = host
            .strip_prefix("*.")
            .map(|s| format!(".{s}"))
            .unwrap_or_else(|| host.into());
        let apex = !host.starts_with('.');
        let suffix = if apex { format!(".{host}") } else { host };
        Some(Self::Domain {
            suffix: ascii_host(&suffix),
            apex,
            port: port.into(),
        })
    }

    fn matches(&self, host: &str, target_port: &str, ip: Option<IpAddr>) -> bool {
        match self {
            Self::All => true,
            Self::Network { address, bits } => {
                ip.is_some_and(|ip| network_contains(*address, *bits, ip))
            }
            Self::Ip { address, port } => {
                ip.map(normalized_ip) == Some(*address) && (port.is_empty() || port == target_port)
            }
            Self::Domain { suffix, apex, port } => {
                ip.is_none()
                    && (host.ends_with(suffix) || (*apex && host == &suffix[1..]))
                    && (port.is_empty() || port == target_port)
            }
        }
    }
}

fn bits_string_is_decimal(value: &str) -> bool {
    value
        .rsplit_once('/')
        .is_some_and(|(_, bits)| !bits.is_empty() && bits.bytes().all(|b| b.is_ascii_digit()))
}

fn network_contains(network: IpAddr, bits: u8, target: IpAddr) -> bool {
    // net.IPNet.Contains treats IPv4-mapped IPv6 networks at /96 or longer
    // as IPv4 networks. Shorter IPv6 networks do not contain IPv4 addresses.
    let (network, bits) = match network {
        IpAddr::V6(v6) if bits >= 96 => match v6.to_ipv4_mapped() {
            Some(v4) => (IpAddr::V4(v4), bits - 96),
            None => (network, bits),
        },
        _ => (network, bits),
    };
    match (network, normalized_ip(target)) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mask = u32::MAX.checked_shl(u32::from(32 - bits)).unwrap_or(0);
            u32::from(a) & mask == u32::from(b) & mask
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let mask = u128::MAX.checked_shl(u32::from(128 - bits)).unwrap_or(0);
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    }
}

fn split_rule_host_port(value: &str) -> Option<(&str, &str)> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        if host.contains(['[', ']']) || port.contains([':', '[', ']']) {
            return None;
        }
        return Some((host, port));
    }
    let (host, port) = value.split_once(':')?;
    if host.contains(['[', ']']) || port.contains([':', '[', ']']) {
        return None;
    }
    Some((host, port))
}

fn go_lower(value: &str) -> String {
    value
        .chars()
        .map(|c| crate::unicode_simple::map(c, false))
        .collect()
}

fn go_trim(value: &str) -> &str {
    value.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{0085}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'))
}

fn ascii_host(value: &str) -> String {
    if value.is_ascii() {
        return value.into();
    }
    Uts46::new()
        .to_ascii(
            value.as_bytes(),
            AsciiDenyList::STD3,
            Hyphens::Check,
            DnsLength::Ignore,
        )
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| value.into())
}

// Only the proxy URL's scheme/authority reach net/http's connection method.
// Validate Go's path/fragment escaping too, even though neither affects routing.
// Keep parse failure separate from an accepted URL reqwest cannot represent:
// Go httpproxy ignores the former; the latter must never silently become direct.
struct ProxyParts<'a> {
    scheme: String,
    authority: &'a str,
    host: &'a str,
}

fn parsed_proxy<'a>(value: &'a str, prefixed: &'a str) -> Option<ProxyParts<'a>> {
    if value.is_empty() {
        return None;
    }
    let original = proxy_parts(value);
    if original
        .as_ref()
        .is_none_or(|p| p.scheme.is_empty() || p.host.is_empty())
    {
        proxy_parts(prefixed).or(original)
    } else {
        original
    }
}
fn selected_proxy(value: &str) -> Result<Option<Url>> {
    let prefixed = format!("http://{value}");
    let parsed = parsed_proxy(value, &prefixed);
    let Some(parsed) = parsed else {
        return Ok(None);
    };
    let scheme = match parsed.scheme.as_str() {
        "http" => "http",
        "https" => "https",
        // Go socks5 and socks5h both send the original hostname to the proxy.
        "socks5" | "socks5h" => "socks5h",
        // Go net/http also uses plaintext CONNECT for other proxy schemes.
        // They have no default port (transport.go schemePort), so only an
        // explicit nonempty port can be represented without inventing one.
        _ if parsed.host.rsplit_once(':').is_some_and(|(_, port)| {
            !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
        }) =>
        {
            "http"
        }
        _ => return Err(configuration_error()),
    };
    let url = Url::parse(&format!("{scheme}://{}", parsed.authority))
        .map_err(|_| configuration_error())?;
    if url.host_str().is_none() {
        return Err(configuration_error());
    }
    Ok(Some(url))
}

fn proxy_parts(value: &str) -> Option<ProxyParts<'_>> {
    let (main, fragment) = value.split_once('#').unwrap_or((value, ""));
    if !valid_escapes(fragment) || main.bytes().any(|b| b < b' ' || b == 127) {
        return None;
    }
    let mut scheme = "";
    let mut rest = main;
    for (i, byte) in main.bytes().enumerate() {
        match byte {
            b':' if i == 0 => return None,
            b':' => {
                scheme = &main[..i];
                rest = &main[i + 1..];
                break;
            }
            b'a'..=b'z' | b'A'..=b'Z' => (),
            b'0'..=b'9' | b'+' | b'-' | b'.' if i > 0 => (),
            _ => break,
        }
    }
    rest = rest.split_once('?').map_or(rest, |(path, _)| path);
    if !rest.starts_with('/') && !scheme.is_empty() {
        return Some(ProxyParts {
            scheme: scheme.to_ascii_lowercase(),
            authority: "",
            host: "",
        });
    }
    if scheme.is_empty() && rest.split('/').next()?.contains(':') {
        return None;
    }
    let (authority, path) =
        if rest.starts_with("//") && (!scheme.is_empty() || !rest.starts_with("///")) {
            rest[2..].split_once('/').unwrap_or((&rest[2..], ""))
        } else {
            ("", rest)
        };
    if !valid_escapes(path) {
        return None;
    }
    let host = if let Some((user, host)) = authority.rsplit_once('@') {
        if !valid_escapes(user)
            || !user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._:~!$&'()*+,;=%@".contains(&b))
        {
            return None;
        }
        host
    } else {
        authority
    };
    if !valid_proxy_host(host, &scheme.to_ascii_lowercase()) {
        return None;
    }
    Some(ProxyParts {
        scheme: scheme.to_ascii_lowercase(),
        authority,
        host,
    })
}

fn valid_escapes(value: &str) -> bool {
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%'
            && !(bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                && bytes.next().is_some_and(|b| b.is_ascii_hexdigit()))
        {
            return false;
        }
    }
    true
}

fn valid_proxy_host(host: &str, scheme: &str) -> bool {
    if !valid_escapes(host) {
        return false;
    }
    let mut bytes = host.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let a = bytes.next().unwrap_or_default();
            let b = bytes.next().unwrap_or_default();
            // Go allows escaped non-ASCII bytes and %25 in hostnames.
            if a < b'8' && !(a == b'2' && b == b'5') {
                return false;
            }
        } else if byte.is_ascii()
            && !(byte.is_ascii_alphanumeric() || b"-_.~!$&'()*+,;=:[]<>\"".contains(&byte))
        {
            return false;
        }
    }
    if let Some(rest) = host.strip_prefix('[') {
        let Some((address, suffix)) = rest.rsplit_once(']') else {
            return false;
        };
        if !suffix.is_empty()
            && !suffix
                .strip_prefix(':')
                .is_some_and(|p| p.bytes().all(|b| b.is_ascii_digit()))
        {
            return false;
        }
        let address = address.split("%25").next().unwrap_or(address);
        return address.parse::<std::net::Ipv6Addr>().is_ok();
    }
    if host.contains('[') {
        return false;
    }
    if let Some((_, port)) = if matches!(scheme, "http" | "https") {
        host.split_once(':')
    } else {
        host.rsplit_once(':')
    } {
        return port.bytes().all(|b| b.is_ascii_digit());
    }
    true
}

#[cfg(test)]
mod tests;
