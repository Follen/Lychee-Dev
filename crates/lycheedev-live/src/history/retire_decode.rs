//! Retirement errors carry Go's partially decoded record. Ordinary type errors
//! save the first error and continue; custom time.UnmarshalJSON errors stop.
//! This does not change the read-only module's existing diagnostic wording.
use super::{
    bootstrap::{ReceiverAttempt, ReceiverBindings, Signal},
    records::*,
    retire_json::{end, fields, fold},
    *,
};
use crate::go_mailbox_v1::admission::{
    ClientRecord, ConnectionTarget, RecordSeed, WindowRecord, WireRecord,
};
use serde::de::DeserializeSeed;
struct Issue {
    error: ToolkitError,
    fatal: bool,
}
type Decode = std::result::Result<(), Issue>;
trait PartialRecord: WireRecord {
    fn nested(&mut self, _key: &str, _raw: &[u8], _control: &OperationControl) -> Option<Decode> {
        None
    }
    fn scalar_error(
        &mut self,
        _key: &str,
        _raw: &[u8],
        _control: &OperationControl,
    ) -> Result<bool> {
        Ok(false)
    }
}
fn into<T: PartialRecord>(raw: &[u8], value: &mut T, control: &OperationControl) -> Decode {
    let members = fields(raw, control).map_err(|error| Issue {
        fatal: matches!(
            error.code.as_str(),
            "command.cancelled" | "command.deadline"
        ),
        error,
    })?;
    let mut first = None;
    for (key, raw) in members {
        control
            .check()
            .map_err(|error| Issue { error, fatal: true })?;
        let key = fold(&key);
        let result = if let Some(nested) = value.nested(&key, raw, control) {
            nested
        } else {
            let one = (|| -> Result<()> {
                let mut member = serde_json::to_vec(&key)?;
                member.insert(0, b'{');
                member.push(b':');
                member.extend_from_slice(raw);
                member.push(b'}');
                let normalized =
                    lycheedev_core::evidence::normalize_go_json_strings(&member, control)?;
                let mut decoder = serde_json::Deserializer::from_slice(&normalized);
                RecordSeed(&mut *value).deserialize(&mut decoder)?;
                decoder.end()?;
                Ok(())
            })();
            match one {
                Ok(()) => Ok(()),
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "command.cancelled" | "command.deadline"
                    ) =>
                {
                    Err(Issue { error, fatal: true })
                }
                Err(error) => match value.scalar_error(&key, raw, control) {
                    Ok(fatal) => Err(Issue { error, fatal }),
                    Err(error) => Err(Issue { error, fatal: true }),
                },
            }
        };
        if let Err(error) = result {
            if error.fatal {
                return Err(error);
            }
            if first.is_none() {
                first = Some(error);
            }
        }
    }
    control
        .check()
        .map_err(|error| Issue { error, fatal: true })?;
    first.map_or(Ok(()), Err)
}
// time.Time resets its receiver only after verifying the token is quoted.
// A malformed quoted time returns zero; a non-string type error preserves it.
fn time_error(timestamp: &mut Timestamp, raw: &[u8]) -> bool {
    if raw.first() == Some(&b'"') {
        *timestamp = Timestamp::default();
    }
    true
}
fn pointer<T: PartialRecord + Default>(
    raw: &[u8],
    value: &mut Option<T>,
    control: &OperationControl,
) -> Decode {
    if raw == b"null" {
        *value = None;
        return Ok(());
    }
    into(raw, value.get_or_insert_with(T::default), control)
}
impl PartialRecord for WorkRecord {
    fn nested(&mut self, key: &str, raw: &[u8], control: &OperationControl) -> Option<Decode> {
        match key {
            "intent" => Some(into(raw, &mut self.intent, control)),
            "observation" => {
                self.observation.0 = raw.to_vec();
                Some(Ok(()))
            }
            _ => None,
        }
    }
    fn scalar_error(&mut self, key: &str, raw: &[u8], _control: &OperationControl) -> Result<bool> {
        Ok(match key {
            "createdat" => time_error(&mut self.created_at, raw),
            "updatedat" => time_error(&mut self.updated_at, raw),
            _ => false,
        })
    }
}
impl PartialRecord for WorkIntent {
    fn nested(&mut self, key: &str, raw: &[u8], control: &OperationControl) -> Option<Decode> {
        match key {
            "admission" => Some(pointer(raw, &mut self.admission, control)),
            "request" => {
                self.request.0 = raw.to_vec();
                Some(Ok(()))
            }
            _ => None,
        }
    }
}
impl PartialRecord for WindowAdmission {}
impl PartialRecord for ClientRecord {}
impl PartialRecord for WindowRecord {
    fn scalar_error(&mut self, key: &str, raw: &[u8], control: &OperationControl) -> Result<bool> {
        if !matches!(key, "handle" | "processstartedat") || raw.first() != Some(&b'"') {
            return Ok(false);
        }
        let normalized = lycheedev_core::evidence::normalize_go_json_strings(raw, control)?;
        let literal: String = serde_json::from_slice(&normalized)?;
        // Go decode.literalStore saves type/number failures, but returns on
        // an invalid literal prefix or an invalid inner quoted string.
        Ok(match literal.as_bytes().first() {
            None | Some(b'n' | b't' | b'f' | b'-' | b'0'..=b'9') => false,
            Some(b'"') => !go_inner_string_valid(literal.as_bytes(), control)?,
            _ => true,
        })
    }
}
// The inner ,string literal bypasses Go's JSON scanner. unquoteBytes accepts
// \' as well as ordinary JSON escapes, permits lone surrogates, and rejects
// whitespace after the final quote. Only validity matters for a uint field.
fn go_inner_string_valid(raw: &[u8], control: &OperationControl) -> Result<bool> {
    if raw.len() < 2 || raw.first() != Some(&b'"') || raw.last() != Some(&b'"') {
        return Ok(false);
    }
    let body = &raw[1..raw.len() - 1];
    let mut at = 0;
    while at < body.len() {
        if at % 65536 == 0 {
            control.check()?;
        }
        match body[at] {
            b'\\' => {
                at += 1;
                match body.get(at) {
                    Some(b'"' | b'\\' | b'/' | b'\'' | b'b' | b'f' | b'n' | b'r' | b't') => {}
                    Some(b'u') => {
                        let Some(hex) = body.get(at + 1..at + 5) else {
                            return Ok(false);
                        };
                        if !hex.iter().all(u8::is_ascii_hexdigit) {
                            return Ok(false);
                        }
                        at += 4;
                    }
                    _ => return Ok(false),
                }
            }
            b'"' | 0..=31 => return Ok(false),
            _ => {}
        }
        at += 1;
    }
    control.check()?;
    Ok(true)
}
impl PartialRecord for ConnectionTarget {
    fn nested(&mut self, key: &str, raw: &[u8], control: &OperationControl) -> Option<Decode> {
        match key {
            "client" => Some(into(raw, &mut self.client, control)),
            "window" => Some(into(raw, &mut self.window, control)),
            _ => None,
        }
    }
}
impl PartialRecord for ReceiverAttempt {
    fn scalar_error(&mut self, key: &str, raw: &[u8], _control: &OperationControl) -> Result<bool> {
        Ok(key == "updatedat" && time_error(&mut self.updated_at, raw))
    }
}
impl PartialRecord for ReceiverBindings {}
impl PartialRecord for Signal {}
impl PartialRecord for BootstrapAttempt {
    fn nested(&mut self, key: &str, raw: &[u8], control: &OperationControl) -> Option<Decode> {
        match key {
            "target" => Some(into(raw, &mut self.target, control)),
            "receiver" => Some(into(raw, &mut self.receiver, control)),
            "bindings" => Some(into(raw, &mut self.bindings, control)),
            "business" => Some(pointer(raw, &mut self.business, control)),
            "history" => Some(history(raw, &mut self.history, control)),
            _ => None,
        }
    }
    fn scalar_error(&mut self, key: &str, raw: &[u8], _control: &OperationControl) -> Result<bool> {
        Ok(match key {
            "createdat" => time_error(&mut self.created_at, raw),
            "updatedat" => time_error(&mut self.updated_at, raw),
            _ => false,
        })
    }
}
fn history(raw: &[u8], history: &mut bootstrap::History, control: &OperationControl) -> Decode {
    if raw == b"null" {
        *history = Default::default();
        return Ok(());
    }
    if raw.first() != Some(&b'[') {
        return Err(Issue {
            error: fault("historical history requires array"),
            fatal: false,
        });
    }
    let mut at = 1;
    let mut length = 0;
    let mut first = None;
    loop {
        control
            .check()
            .map_err(|error| Issue { error, fatal: true })?;
        while raw
            .get(at)
            .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b','))
        {
            at += 1;
        }
        if raw.get(at) == Some(&b']') {
            break;
        }
        let stop = end(raw, at);
        if length == history.storage.len() {
            history.storage.push(Default::default());
        }
        history.length = history.length.max(length + 1);
        if let Err(issue) = into(&raw[at..stop], &mut history.storage[length], control) {
            if issue.fatal {
                return Err(issue);
            }
            if first.is_none() {
                first = Some(issue);
            }
        }
        length += 1;
        at = stop;
    }
    history.length = length;
    if length == 0 {
        *history = Default::default();
    }
    first.map_or(Ok(()), Err)
}
pub(super) fn work(raw: &[u8], record: &mut WorkRecord, control: &OperationControl) -> Result<()> {
    if !lycheedev_core::raw_json::is_valid(raw) {
        return Err(fault("invalid historical JSON"));
    }
    into(raw, record, control).map_err(|e| e.error)
}
pub(super) fn attempt(
    raw: &[u8],
    id: &str,
    attempt: &mut BootstrapAttempt,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    if !lycheedev_core::raw_json::is_valid(raw) {
        return Err(corrupt());
    }
    let result = into(raw, attempt, control);
    if let Err(issue) = result {
        control.check()?;
        return Err(
            if matches!(
                issue.error.code.as_str(),
                "command.cancelled" | "command.deadline"
            ) {
                issue.error
            } else {
                corrupt()
            },
        );
    }
    if attempt.schema != "lycheedev.bootstrap-receiver.v1" || attempt.id != id {
        return Err(corrupt());
    }
    Ok(())
}
