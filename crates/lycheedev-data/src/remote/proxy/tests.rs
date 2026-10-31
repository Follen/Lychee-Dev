// SPDX-License-Identifier: MIT
use super::*;

fn fixture_policy(proxy: String, no_proxy: &str) -> Policy {
    Policy { proxy, http_proxy: String::new(), cgi: false,
        bypass: no_proxy.split(',').filter_map(Rule::parse).collect() }
}

fn routed(no_proxy: &str, locator: &str) -> bool {
    fixture_policy("http://proxy.invalid:8080".into(), no_proxy)
        .select(locator)
        .unwrap()
        .is_some()
}

#[test]
fn environment_nonempty_precedence_without_process_mutation() {
    for (upper, lower, expected, reads) in [
        ("", "lower", "lower", 2),
        ("upper", "lower", "upper", 1),
        ("", "", "", 2),
    ] {
        let mut names = Vec::new();
        let value = env_first_with("HTTPS_PROXY", "https_proxy", |name| {
            names.push(name.to_string());
            Ok(if name == "HTTPS_PROXY" { upper } else { lower }.into())
        })
        .unwrap();
        assert_eq!(value, expected);
        assert_eq!(names.len(), reads);
        assert!(!names.iter().any(|name| name == "ALL_PROXY"));
    }
    assert_eq!(
        env_first_with("NO_PROXY", "no_proxy", |name| Ok(if name == "NO_PROXY" {
            ""
        } else {
            "example.com"
        }
        .into()))
        .unwrap(),
        "example.com"
    );
}

#[test]
fn https_only_and_builtin_bypass() {
    for locator in [
        "http://127.0.0.1:8765/x",
        "https://localhost/x",
        "https://127.0.0.9/x",
        "https://[::1]/x",
        "https://[::ffff:127.0.0.2]/x",
    ] {
        assert!(!routed("", locator), "{locator}");
    }
    // Go's localhost special case precedes case folding, and does not trim a dot.
    assert!(routed("", "https://LOCALHOST/x"));
    assert!(routed("", "https://localhost./x"));
    assert!(routed("", "https://128.0.0.1/x"));
    assert!(
        !fixture_policy(String::new(), "")
            .select("https://cdn.example/x")
            .unwrap()
            .is_some()
    );
}

#[test]
fn domains_apex_subdomains_and_literal_ports() {
    for prefix in [".example.com", "*.example.com"] {
        assert!(routed(prefix, "https://example.com/x"));
        assert!(!routed(prefix, "https://a.example.com/x"));
        assert!(routed(prefix, "https://badexample.com/x"));
    }
    assert!(!routed(" EXAMPLE.COM ", "https://EXAMPLE.COM/x"));
    assert!(!routed("example.com:443", "https://a.example.com/x"));
    assert!(routed("example.com:8443", "https://example.com/x"));
    assert!(!routed("example.com:8443", "https://example.com:8443/x"));
    assert!(routed("example.com:0443", "https://example.com/x"));
    assert!(!routed("example.com:0443", "https://example.com:0443/x"));
    assert!(!routed("example.com:", "https://example.com:8443/x"));
    assert!(!routed("invalid, *, .unused", "https://cdn.example/x"));
    assert!(routed(":443,[bad],", "https://cdn.example/x"));
}

#[test]
fn addresses_networks_and_mapped_ipv4() {
    for (rule, yes, no) in [
        (
            "192.0.2.7:443",
            "https://192.0.2.7/x",
            "https://192.0.2.7:8443/x",
        ),
        (
            "192.0.2.129/24",
            "https://192.0.2.1/x",
            "https://192.0.3.1/x",
        ),
        (
            "2001:db8::7/32",
            "https://[2001:db8:abcd::1]/x",
            "https://[2001:db9::1]/x",
        ),
        (
            "[2001:db8::1]:443",
            "https://[2001:db8::1]/x",
            "https://[2001:db8::1]:8443/x",
        ),
        (
            "::ffff:192.0.2.1",
            "https://192.0.2.1/x",
            "https://192.0.2.2/x",
        ),
        (
            "192.0.2.0/24",
            "https://[::ffff:192.0.2.1]/x",
            "https://[::ffff:192.0.3.1]/x",
        ),
        (
            "::ffff:192.0.2.0/120",
            "https://192.0.2.1/x",
            "https://192.0.3.1/x",
        ),
    ] {
        assert!(!routed(rule, yes), "{rule} {yes}");
        assert!(routed(rule, no), "{rule} {no}");
    }
    assert!(routed("0.2.1", "https://192.0.2.1/x"));
    assert!(routed("::/0", "https://192.0.2.1/x"));
}

#[test]
fn unicode_lookup_and_simple_lowercase() {
    assert!(!routed("BÜCHER.example", "https://xn--bcher-kva.example/x"));
    assert!(!routed("bücher.example", "https://BÜCHER.example/x"));
    assert!(!routed(
        ".bücher.example",
        "https://a.xn--bcher-kva.example/x"
    ));
    assert_eq!(simple_fold_lower("İΣ"), "iσ");
    assert_eq!(ascii_host("ascii_with_underscore"), "ascii_with_underscore");
    assert_eq!(ascii_host("-bücher.example"), "-bücher.example");
}

#[test]
fn proxy_schemes_and_remote_socks_dns() {
    for (proxy, scheme, port) in [
        ("proxy.invalid:8123", "http", Some(8123)),
        ("http://user:secret@proxy.invalid:80", "http", None),
        ("https://proxy.invalid:8443", "https", Some(8443)),
        ("socks5://proxy.invalid:1081", "socks5h", Some(1081)),
        ("socks5h://proxy.invalid:1081", "socks5h", Some(1081)),
        (
            "unrecognized://user:secret@proxy.invalid:8123",
            "unrecognized",
            Some(8123),
        ),
    ] {
        let url = fixture_policy(proxy.into(), "")
            .select("https://cdn.example/x")
            .unwrap()
            .unwrap();
        assert_eq!(url.scheme(), scheme);
        assert_eq!(url.port(), port);
        assert_eq!(url.host_str(), Some("proxy.invalid"));
    }
}

#[test]
fn malformed_configuration_order_and_redaction() {
    // Go init ignores errors when both url.Parse attempts fail.
    for proxy in ["%", "proxy.invalid/%zz", "user\nsecret"] {
        assert!(
            fixture_policy(proxy.into(), "")
                .select("https://cdn.example/x")
                .unwrap()
                .is_none()
        );
    }
    // NO_PROXY avoids use of even an unsupported proxy scheme.
    assert!(
        fixture_policy("ftp://user:secret@proxy.invalid".into(), "cdn.example")
            .select("https://cdn.example/x")
            .unwrap()
            .is_none()
    );
    let selected = fixture_policy("ftp://user:secret@proxy.invalid".into(), "")
        .select_raw("https://cdn.example/x")
        .unwrap()
        .unwrap();
    assert_eq!(selected.scheme, "ftp");
    assert_eq!(selected.authority, "user:secret@proxy.invalid");
    // Go's second parse accepts this as host "http:" and path "//[bad".
    let parts = proxy_parts("http://http://[bad").unwrap();
    assert_eq!(parts.host, "http:");
    assert_eq!(
        selected_proxy_raw("http://[bad")
            .unwrap()
            .unwrap()
            .authority,
        "http:"
    );
}

#[test]
fn http_proxy_cgi_precedes_bypass_and_https_policy_stays_separate() {
    let mut p = fixture_policy(
        "http://secure-proxy.example:8081".into(),
        "bypass.example:80",
    );
    p.http_proxy = "http://plain-proxy.example:8080".into();
    assert_eq!(
        p.select("http://origin.example/a")
            .unwrap()
            .unwrap()
            .host_str(),
        Some("plain-proxy.example")
    );
    assert_eq!(
        p.select("https://origin.example/a")
            .unwrap()
            .unwrap()
            .host_str(),
        Some("secure-proxy.example")
    );
    assert!(p.select("http://bypass.example/a").unwrap().is_none());
    assert!(p.select("https://bypass.example/a").unwrap().is_some());
    p.cgi = true;
    for url in [
        "http://origin.example/a",
        "http://127.0.0.1/a",
        "http://bypass.example/a",
    ] {
        assert!(p.select(url).unwrap_err().message.contains("CGI"));
    }
    assert!(p.select("https://origin.example/a").unwrap().is_some());
    p.http_proxy.clear();
    assert!(p.select("http://origin.example/a").unwrap().is_none());
}
#[test]
fn cross_host_redirect_routes_each_origin_and_literal_default_port() {
    let mut p = fixture_policy(
        "http://secure-proxy.example:8081".into(),
        "direct.example,plain.example:80",
    );
    p.http_proxy = "http://plain-proxy.example:8080".into();
    for (url, host) in [
        ("https://initial.example/a", Some("secure-proxy.example")),
        ("https://direct.example/b", None),
        ("http://plain.example/c", None),
        ("http://plain.example:080/c", Some("plain-proxy.example")),
        ("http://other.example/d", Some("plain-proxy.example")),
        ("https://other.example/e", Some("secure-proxy.example")),
        ("http://public:credential@direct.example/f", None),
    ] {
        let selected = p.select(url).unwrap();
        assert_eq!(selected.as_ref().and_then(|u| u.host_str()), host, "{url}");
    }
}
#[test]
fn cgi_only_rejects_successfully_parsed_http_proxy_before_bypass() {
    for raw in ["%", "proxy.invalid/%zz", "user\nsecret", ""] {
        let mut p = fixture_policy(String::new(), "*");
        p.http_proxy = raw.into();
        p.cgi = true;
        assert!(
            p.select("http://origin.example/x").unwrap().is_none(),
            "{raw:?}"
        );
    }
    for raw in [
        "http://proxy.invalid:8080",
        "ftp://user:secret@proxy.invalid",
    ] {
        let mut p = fixture_policy(String::new(), "*");
        p.http_proxy = raw.into();
        p.cgi = true;
        let error = p.select("http://127.0.0.1/x").unwrap_err();
        assert!(error.message.contains("CGI"));
        assert!(!error.message.contains("secret"));
    }
}

#[test]
fn raw_selected_proxy_preserves_own_authority_and_credentials() {
    for authority in [
        "u%FF:p%FE@127.1:0080",
        "0x7f000001:80",
        "BÜCHER.example:00443",
        "b%C3%BCcher.example:8443",
        "[fe80::1%25Ethernet]:8443",
    ] {
        let selected = fixture_policy(format!("https://{authority}"), "")
            .select_raw("https://origin.example/a")
            .unwrap()
            .unwrap();
        assert_eq!(selected.scheme, "https");
        assert_eq!(selected.authority, authority);
    }
}

#[test]
fn raw_proxy_route_keeps_cgi_and_cross_origin_bypass_order() {
    let mut policy = fixture_policy("https://BÜCHER.proxy:00443".into(), ".bypass.test");
    policy.http_proxy = "http://u%FF:p@0x7f000001:0080".into();
    let selected = policy.select_raw("http://first.test/a").unwrap().unwrap();
    assert_eq!(selected.authority, "u%FF:p@0x7f000001:0080");
    assert!(
        policy
            .select_raw("http://next.bypass.test/a")
            .unwrap()
            .is_none()
    );
    policy.cgi = true;
    assert_eq!(
        policy
            .select_raw("http://next.bypass.test/a")
            .err()
            .unwrap()
            .code,
        "records.remote_http"
    );
    policy.http_proxy = "%".into();
    assert!(
        policy
            .select_raw("http://next.bypass.test/a")
            .unwrap()
            .is_none()
    );
}
