//! Fixed Go 3.2.0 historical records. No capture, keyboard, native connection,
//! or replay capability is available in this module.
#![forbid(unsafe_code)]
use lycheedev_core::{Result, ToolkitError, control::OperationControl, metadata::Metadata};
use serde::{Deserialize, Serialize};
use std::path::Path;
mod bootstrap;
pub use bootstrap::BootstrapAttempt;

/// Go time.Time's layout fallback, adapted from the existing core capture
/// decoder. None is a parseable offset which Go refuses to marshal.
#[derive(Debug, Clone)]
struct Timestamp {
    canonical: Option<String>,
    zero: bool,
}
impl Default for Timestamp {
    fn default() -> Self {
        Self {
            canonical: Some("0001-01-01T00:00:00Z".into()),
            zero: true,
        }
    }
}
impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.canonical.as_deref().ok_or_else(|| {
            <S::Error as serde::ser::Error>::custom("json: error calling MarshalJSON for type time.Time: Time.MarshalJSON: timezone hour outside of range [0,23]")
        })?)
    }
}
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let token = Box::<serde_json::value::RawValue>::deserialize(d)?;
        let raw = token
            .get()
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .filter(|s| !s.contains('\\'))
            .ok_or_else(|| <D::Error as serde::de::Error>::custom("invalid RFC3339 time"))?;
        parse_timestamp(raw)
            .ok_or_else(|| <D::Error as serde::de::Error>::custom("invalid RFC3339 time"))
    }
}
fn parse_timestamp(value: &str) -> Option<Timestamp> {
    fn number(raw: &[u8]) -> Option<u32> {
        if raw.is_empty() || !raw.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(raw.iter().fold(0, |n, ch| n * 10 + u32::from(*ch - b'0')))
    }
    let raw = value.as_bytes();
    if raw.len() < 19
        || raw.get(4) != Some(&b'-')
        || raw.get(7) != Some(&b'-')
        || raw.get(10) != Some(&b'T')
    {
        return None;
    }
    let year = number(&raw[..4])?;
    let month = number(&raw[5..7])?;
    let day = number(&raw[8..10])?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days {
        return None;
    }
    let hour_length = if raw.get(12) == Some(&b':') { 1 } else { 2 };
    let colon = 11 + hour_length;
    if raw.get(colon) != Some(&b':') || raw.get(colon + 3) != Some(&b':') {
        return None;
    }
    let hour = number(raw.get(11..colon)?)?;
    let minute = number(raw.get(colon + 1..colon + 3)?)?;
    let second = number(raw.get(colon + 4..colon + 6)?)?;
    if hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    let mut at = colon + 6;
    let mut fraction = "";
    if matches!(raw.get(at), Some(b'.' | b',')) {
        at += 1;
        let start = at;
        while raw.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return None;
        }
        fraction = std::str::from_utf8(&raw[start..at.min(start + 9)])
            .ok()?
            .trim_end_matches('0');
    }
    let zone = raw.get(at..)?;
    let mut offset = 0;
    if zone != b"Z" {
        if zone.len() != 6 || !matches!(zone[0], b'+' | b'-') || zone[3] != b':' {
            return None;
        }
        let hour = number(&zone[1..3])?;
        let minute = number(&zone[4..6])?;
        if hour > 24 || minute > 60 {
            return None;
        }
        offset = hour * 60 + minute;
    }
    let signed_minutes = if zone.first() == Some(&b'-') {
        -(i64::from(offset))
    } else {
        i64::from(offset)
    };
    let local = chrono::NaiveDate::from_ymd_opt(year as i32, month, day)?
        .and_hms_opt(hour, minute, second)?;
    let zero = fraction.is_empty()
        && local.checked_sub_signed(chrono::Duration::minutes(signed_minutes))
            == chrono::NaiveDate::from_ymd_opt(1, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0));
    // Go can parse a >=24h offset but refuses to marshal it into an identity.
    if offset >= 24 * 60 {
        return Some(Timestamp {
            canonical: None,
            zero,
        });
    }
    let mut canonical = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if !fraction.is_empty() {
        canonical.push('.');
        canonical.push_str(fraction);
    }
    if offset == 0 {
        canonical.push('Z');
    } else {
        canonical.push(char::from(zone[0]));
        canonical.push_str(&format!("{:02}:{:02}", offset / 60, offset % 60));
    }
    Some(Timestamp {
        canonical: Some(canonical),
        zero,
    })
}

pub fn inspect_bootstrap(
    home: &Path,
    id: &str,
    control: &OperationControl,
) -> Result<BootstrapAttempt> {
    control.check()?;
    // The raw read is bounded and checked before normalization; it preserves
    // Go's invalid-UTF-8 replacement behavior and duplicate numeric tokens.
    let raw = Metadata::read_raw_controlled(home, &format!("bootstrap/attempt/{id}"), control)
        .map_err(decode_error)?;
    control.check()?;
    decode_bootstrap(&raw, id, control)
}
fn decode_bootstrap(raw: &[u8], id: &str, control: &OperationControl) -> Result<BootstrapAttempt> {
    control.check()?;
    if raw.len() > 1_048_576 || !lycheedev_core::raw_json::is_valid(raw) {
        return Err(corrupt());
    }
    let view =
        lycheedev_core::evidence::normalize_go_json_strings(raw, control).map_err(decode_error)?;
    let value: BootstrapAttempt =
        crate::go_mailbox_v1::admission::decode_wire_record(&view).map_err(|_| corrupt())?;
    control.check()?;
    if value.schema != "lycheedev.bootstrap-receiver.v1" || value.id != id {
        return Err(corrupt());
    }
    Ok(value)
}
fn decode_error(error: ToolkitError) -> ToolkitError {
    if error.code == "input.invalid" {
        corrupt()
    } else {
        error
    }
}
fn corrupt() -> ToolkitError {
    ToolkitError::new("command.failed", "live.bootstrap_attempt_corrupt", 5)
}

#[cfg(test)]
mod tests;

impl Timestamp {
    fn is_zero(&self) -> bool {
        self.zero
    }
}

mod display;
mod pins;
mod records;
mod session;
mod signal;
mod status;
mod store;
pub use session::HistoricalConnection;
pub use status::{HistoricalOutcome, HistoricalRead};
use store::{Captured, DiskRead, RawDocument, ReadStore};
fn fault(message: &str) -> ToolkitError {
    ToolkitError::new("command.failed", message, 5)
}
fn decode_record<T: crate::go_mailbox_v1::admission::WireRecord + Default>(
    raw: &[u8],
    control: &OperationControl,
) -> Result<T> {
    control.check()?;
    if !lycheedev_core::raw_json::is_valid(raw) {
        return Err(fault("invalid historical JSON"));
    }
    let normalized = lycheedev_core::evidence::normalize_go_json_strings(raw, control)?;
    let value = crate::go_mailbox_v1::admission::decode_wire_record(&normalized);
    control.check()?;
    value.map_err(|error| fault(&error.to_string()))
}
fn domain_decode<T>(result: Result<T>, code: &str, control: &OperationControl) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            control.check()?;
            Err(
                if matches!(
                    error.code.as_str(),
                    "command.cancelled" | "command.deadline" | "live.deadline_pending"
                ) {
                    error
                } else {
                    fault(code)
                },
            )
        }
    }
}
pub fn inspect_operation(home: &Path, id: &str, control: &OperationControl) -> HistoricalRead {
    status::read_status_outcome(&DiskRead(home), id, control)
}
pub fn inspect_session(
    home: &Path,
    id: &str,
    control: &OperationControl,
) -> Result<HistoricalConnection> {
    Ok(session::read_session(&DiskRead(home), id, control)?.connection())
}
#[cfg(test)]
mod status_tests;
