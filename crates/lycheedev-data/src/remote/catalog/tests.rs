use super::*;

const HEADER: &str =
    "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:0|VersionsName!STRING:0\n";
const KEY_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn row(region: &str, build: &str) -> String {
    format!(
        "{region}|{KEY_A}|{KEY_B}|{}|{build}\n",
        build.rsplit('.').next().unwrap()
    )
}
#[test]
fn historical_exact_build_and_numerical_latest_are_distinct() {
    let input = format!(
        "{HEADER}{}{}{}",
        row("us", "12.1.0.9"),
        row("us", "12.1.0.100"),
        row("us", "12.1.0.20")
    );
    assert_eq!(
        release(input.as_bytes(), "us", "12.1.0.9")
            .unwrap()
            .full_build,
        "12.1.0.9"
    );
    assert_eq!(
        release(input.as_bytes(), "us", "").unwrap().full_build,
        "12.1.0.100"
    );
    assert_eq!(
        release(input.as_bytes(), "us", "12.1.0.10")
            .unwrap_err()
            .code,
        "records.build_unavailable"
    );
    assert_eq!(
        release(input.as_bytes(), "cn", "").unwrap_err().code,
        "records.build_unavailable"
    );
}
#[test]
fn exact_duplicates_and_numerically_equal_current_rows_are_ambiguous() {
    let duplicated = format!("{HEADER}{}{}", row("us", "12.1.0.9"), row("us", "12.1.0.9"));
    for expected in ["", "12.1.0.9"] {
        assert_eq!(
            release(duplicated.as_bytes(), "us", expected)
                .unwrap_err()
                .code,
            "records.build_ambiguous"
        );
    }
    let equal = format!(
        "{HEADER}{}{}",
        row("us", "12.1.0.9"),
        row("us", "12.1.0.009")
    );
    assert_eq!(
        release(equal.as_bytes(), "us", "").unwrap_err().code,
        "records.build_ambiguous"
    );
    assert_eq!(
        release(equal.as_bytes(), "us", "12.1.0.009")
            .unwrap()
            .full_build,
        "12.1.0.009"
    );
}
#[test]
fn validates_selected_row_only_and_preserves_single_invalid_row_error() {
    let input = format!(
        "\u{feff}# generated\r\n{HEADER}eu|bad|bad|bad|bad\nus|bad|bad|bad|bad\n{}",
        row("us", "12.1.0.9")
    );
    assert!(release(input.as_bytes(), "us", "").is_ok());
    let invalid = format!("{HEADER}us|bad|bad|bad|bad\n");
    assert_eq!(
        release(invalid.as_bytes(), "us", "").unwrap_err().code,
        "records.invalid_metadata"
    );
    let invalid = format!("{invalid}us|bad|bad|bad|also-bad\n");
    assert_eq!(
        release(invalid.as_bytes(), "us", "").unwrap_err().code,
        "records.build_ambiguous"
    );
    for build in ["+12.1.0.9", "12.1.0.4294967296", "12.1.0.-9", "12.1.0."] {
        assert!(build_components(build).is_none());
    }
}
#[test]
fn missing_fields_bad_types_duplicate_headers_and_row_width_are_rejected() {
    for text in [
        "Region!STRING:0|Region!STRING:0\nus|us",
        "Region!STRING|BuildConfig!HEX:16",
        "Region!STRING:+1",
        "Region!STRING:0\nus\rbroken",
        "Region!STRING:0\nus|extra",
        "Region!STRING:0\nus\0",
    ] {
        assert!(parse(text.as_bytes(), &["Region"]).is_err(), "{text:?}");
    }
    assert!(parse(&[0xff], &[]).is_err());
    assert!(parse(b"# no header", &[]).is_err());
    assert!(release(b"Region!STRING:0\nus", "us", "").is_err());
}
#[test]
fn catalog_limits_are_independent() {
    assert_eq!(
        parse(&vec![b'x'; MAX_BYTES + 1], &[]).err().unwrap().code,
        "records.metadata_limit"
    );
    let header = (0..129)
        .map(|i| format!("C{i}!STRING:0"))
        .collect::<Vec<_>>()
        .join("|");
    assert_eq!(
        parse(header.as_bytes(), &[]).err().unwrap().code,
        "records.metadata_limit"
    );
    let input = format!("Region!STRING:0\n{}", "us\n".repeat(4096));
    assert_eq!(parse(input.as_bytes(), &[]).unwrap().rows.len(), 4096);
    assert_eq!(
        parse(format!("{input}us\n").as_bytes(), &[])
            .err()
            .unwrap()
            .code,
        "records.metadata_limit"
    );
}
#[test]
fn routes_validate_only_exact_region_and_never_allow_url_authorities() {
    let header = "Name!STRING:0|Path!STRING:0|Hosts!STRING:0\n";
    let good = format!(
        "{header}us|tpr/wow/12_1|cdn01.example.com cdn02.example.com\neu|invalid//path|bad host\n"
    );
    let got = distribution(good.as_bytes(), "us").unwrap();
    assert_eq!(
        got.config_url("cdn01.example.com", KEY_A).unwrap(),
        format!("https://cdn01.example.com/tpr/wow/12_1/config/aa/aa/{KEY_A}")
    );
    assert!(got.data_url("evil.example.com", KEY_A).is_err());
    for path in [
        "",
        "/tpr",
        "tpr/",
        "tpr//wow",
        "../wow",
        "tpr/../wow",
        "tpr?x",
        "tpr%2fwow",
    ] {
        assert!(!valid_path(path), "{path}");
    }
    for host in [
        "localhost",
        "127.0.0.1",
        "999.1.2.3",
        "[::1]",
        "cdn.example.com:443",
        "user@cdn.example.com",
        "https://cdn.example.com",
        "cdn.example.com.",
        "-cdn.example.com",
        "cdn_.example.com",
    ] {
        assert!(!valid_host(host), "{host}");
    }
    assert!(
        distribution(
            format!("{header}us|tpr/wow|CDN.example.com cdn.example.com").as_bytes(),
            "us"
        )
        .is_err()
    );
    assert_eq!(
        distribution(
            format!("{header}us|tpr/wow|cdn.example.com\nus|tpr/wow|cdn.example.com").as_bytes(),
            "us"
        )
        .unwrap_err()
        .code,
        "records.build_ambiguous"
    );
}
