//! One current versions observation. This transport does not select, pin or
//! cache a build; the Core availability provider owns BPSV and product mapping.
use crate::{definitions::http_agent, fault};
use lycheedev_core::{Result, control::OperationControl};
use std::{io::Read, time::Duration};

const MAX_BYTES: usize = 1 << 20;
fn locator(region: &str, product_slot: &str) -> Result<String> {
    if !["us", "eu", "cn", "kr", "tw"].contains(&region)
        || product_slot.is_empty()
        || product_slot.len() > 64
        || !product_slot.starts_with("wow")
        || !product_slot
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(fault(
            "selection.invalid_target",
            2,
            "Invalid versions region or product slot",
        ));
    }
    Ok(if region == "cn" {
        format!("https://cn.version.battlenet.com.cn/{product_slot}/versions")
    } else {
        format!("https://{region}.version.battle.net/{product_slot}/versions")
    })
}

fn read_bounded(mut input: impl Read, control: &OperationControl) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        control.check()?;
        let length = input
            .read(&mut chunk)
            .map_err(|e| fault("selection.listing_http", 5, e.to_string()))?;
        control.check()?;
        if length == 0 {
            break;
        }
        if raw.len() + length > MAX_BYTES {
            return Err(fault(
                "selection.manifest_limit",
                3,
                "Versions manifest exceeds 1 MiB",
            ));
        }
        raw.extend_from_slice(&chunk[..length]);
    }
    std::str::from_utf8(&raw).map_err(|_| {
        fault(
            "selection.invalid_manifest",
            4,
            "Versions manifest is not UTF-8",
        )
    })?;
    Ok(raw)
}

/// Exactly one HTTPS GET, no redirects, no state writes and no fallback URL.
/// The blocking transport checks cancellation before/after each read; a blocked
/// socket cannot wake immediately on AtomicBool and remains bounded by the
/// lesser of 30 seconds and this caller's existing remaining deadline.
pub fn fetch_versions(
    region: &str,
    product_slot: &str,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    let url = locator(region, product_slot)?;
    let agent = http_agent(control.remaining()?.min(Duration::from_secs(30)));
    let response = agent.get(&url).header("Accept-Encoding", "identity").call();
    control.check()?;
    let mut response = response.map_err(|e| fault("selection.listing_http", 5, e.to_string()))?;
    if response.status().as_u16() != 200 {
        return Err(fault(
            "selection.listing_http",
            5,
            format!("Versions HTTP status {}", response.status()),
        ));
    }
    if response
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(fault(
            "selection.manifest_limit",
            3,
            "Versions manifest exceeds 1 MiB",
        ));
    }
    read_bounded(response.body_mut().as_reader(), control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn exact_endpoint_and_bounded_utf8_admission() {
        assert_eq!(
            locator("cn", "wowf").unwrap(),
            "https://cn.version.battlenet.com.cn/wowf/versions"
        );
        assert_eq!(
            locator("us", "wowf").unwrap(),
            "https://us.version.battle.net/wowf/versions"
        );
        for slot in [
            "",
            "wow/../../",
            "wow?query",
            "wow#fragment",
            "wow\n",
            "WOW",
            "https://example.com",
        ] {
            assert!(locator("us", slot).is_err());
        }
        assert!(locator("../../", "wow").is_err());
        let control = OperationControl::default();
        assert_eq!(
            read_bounded(&b"Region!STRING:0\nus\n"[..], &control).unwrap(),
            b"Region!STRING:0\nus\n"
        );
        assert_eq!(
            read_bounded(&[0xff][..], &control).unwrap_err().exit_code,
            4
        );
        assert!(
            read_bounded(std::io::repeat(b'a').take((MAX_BYTES + 1) as u64), &control).is_err()
        );
    }
    #[test]
    fn cancelled_transport_never_attempts_network() {
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            fetch_versions("us", "wow", &control).unwrap_err().exit_code,
            7
        );
    }
}
