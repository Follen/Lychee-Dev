//! Fixed-Go json.Marshal persistence, distinct from public output encoding.
//! Typed fields normalize through the existing Go decoder; RawMessage payloads
//! retain their original bytes, duplicates and number spellings.
use super::{
    records::{WorkIntent, WorkRecord},
    *,
};
use lycheedev_core::control::OperationControl;

fn ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\r' | b'\t')
}
pub(super) fn end(raw: &[u8], start: usize) -> usize {
    let mut at = start;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escape = false;
    while at < raw.len() {
        let b = raw[at];
        if quoted {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                quoted = false;
                if depth == 0 {
                    return at + 1;
                }
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        return at + 1;
                    }
                }
                b',' | b'}' | b']' if depth == 0 => return at,
                b if depth == 0 && ws(b) => return at,
                _ => {}
            }
        }
        at += 1;
    }
    at
}
pub(super) fn fields<'a>(
    raw: &'a [u8],
    control: &OperationControl,
) -> Result<Vec<(String, &'a [u8])>> {
    control.check()?;
    if !lycheedev_core::raw_json::is_valid(raw) {
        return Err(fault("invalid historical JSON"));
    }
    let mut at = raw.iter().position(|b| !ws(*b)).unwrap_or(raw.len());
    if raw.get(at) == Some(&b'n') {
        return Ok(Vec::new());
    }
    if raw.get(at) != Some(&b'{') {
        return Err(fault("historical record requires object"));
    }
    at += 1;
    let mut out = Vec::new();
    loop {
        control.check()?;
        while raw.get(at).is_some_and(|b| ws(*b) || *b == b',') {
            at += 1;
        }
        if raw.get(at) == Some(&b'}') {
            return Ok(out);
        }
        let stop = end(raw, at);
        let key = lycheedev_core::evidence::normalize_go_json_strings(&raw[at..stop], control)?;
        let key: String = serde_json::from_slice(&key)?;
        at = stop;
        while raw.get(at).is_some_and(|b| ws(*b) || *b == b':') {
            at += 1;
        }
        let stop = end(raw, at);
        out.push((key, &raw[at..stop]));
        at = stop;
    }
}
pub(super) fn fold(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            'ſ' => 's',
            'K' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
pub(super) fn preserve_raw(
    record: &mut WorkRecord,
    raw: &[u8],
    control: &OperationControl,
) -> Result<()> {
    for (key, value) in fields(raw, control)? {
        match fold(&key).as_str() {
            "intent" => {
                for (key, value) in fields(value, control)? {
                    if fold(&key) == "request" {
                        record.intent.request.0 = value.to_vec();
                    }
                }
            }
            "observation" => record.observation.0 = value.to_vec(),
            _ => {}
        }
    }
    Ok(())
}
/// json.Marshal's appendCompact: quoted invalid UTF-8 is deliberately copied;
/// escaping applies to HTML bytes and the literal UTF-8 U+2028/U+2029 sequences.
pub(super) fn compact(raw: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    control.check()?;
    if !lycheedev_core::raw_json::is_valid(raw) {
        return Err(fault("invalid historical JSON"));
    }
    let mut out = Vec::with_capacity(raw.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut at = 0;
    while at < raw.len() {
        if at % 65536 == 0 {
            control.check()?;
        }
        let b = raw[at];
        if quoted {
            if escaped {
                out.push(b);
                escaped = false;
            } else {
                match b {
                    b'\\' => {
                        out.push(b);
                        escaped = true;
                    }
                    b'"' => {
                        out.push(b);
                        quoted = false;
                    }
                    b'<' => out.extend_from_slice(b"\\u003c"),
                    b'>' => out.extend_from_slice(b"\\u003e"),
                    b'&' => out.extend_from_slice(b"\\u0026"),
                    0xe2 if raw.get(at + 1) == Some(&0x80)
                        && matches!(raw.get(at + 2), Some(0xa8 | 0xa9)) =>
                    {
                        out.extend_from_slice(if raw[at + 2] == 0xa8 {
                            b"\\u2028"
                        } else {
                            b"\\u2029"
                        });
                        at += 2;
                    }
                    _ => out.push(b),
                }
            }
        } else if b == b'"' {
            out.push(b);
            quoted = true;
        } else if !ws(b) {
            out.push(b);
        }
        at += 1;
    }
    control.check()?;
    Ok(out)
}
fn value(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(value)?)
}
fn member(out: &mut Vec<u8>, key: &str, bytes: &[u8]) {
    if out.len() > 1 {
        out.push(b',');
    }
    out.push(b'"');
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(b"\":");
    out.extend_from_slice(bytes);
}
fn scalar(out: &mut Vec<u8>, key: &str, v: &impl Serialize) -> Result<()> {
    member(out, key, &value(v)?);
    Ok(())
}
fn raw_or_null(raw: &[u8]) -> &[u8] {
    if raw.is_empty() { b"null" } else { raw }
}
pub(super) fn intent_bytes(
    intent: &WorkIntent,
    empty_goal: bool,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    let mut out = vec![b'{'];
    for (key, text) in [
        ("kind", &intent.kind),
        ("resource", &intent.resource),
        ("snapshot", &intent.snapshot),
        ("session", &intent.session),
    ] {
        scalar(&mut out, key, text)?;
    }
    member(&mut out, "request", raw_or_null(&intent.request.0));
    for (key, text) in [
        ("requestKey", &intent.request_key),
        ("requestDigest", &intent.request_digest),
        ("goal", &intent.goal),
    ] {
        if !text.is_empty() && !(empty_goal && key == "goal") {
            scalar(&mut out, key, text)?;
        }
    }
    if let Some(admission) = &intent.admission {
        #[derive(Serialize)]
        struct Admission<'a> {
            parent: &'a str,
            #[serde(rename = "workspaceId")]
            workspace: &'a str,
        }
        scalar(
            &mut out,
            "admission",
            &Admission {
                parent: &admission.parent,
                workspace: &admission.workspace_id,
            },
        )?;
    }
    out.push(b'}');
    compact(&out, control)
}
pub(super) fn work_bytes(record: &WorkRecord, control: &OperationControl) -> Result<Vec<u8>> {
    let mut out = vec![b'{'];
    scalar(&mut out, "schema", &record.schema)?;
    scalar(&mut out, "operationId", &record.operation_id)?;
    scalar(&mut out, "generation", &record.generation)?;
    member(
        &mut out,
        "intent",
        &intent_bytes(&record.intent, false, control)?,
    );
    scalar(&mut out, "stage", &record.stage)?;
    scalar(&mut out, "status", &record.status)?;
    scalar(&mut out, "createdAt", &record.created_at)?;
    scalar(&mut out, "updatedAt", &record.updated_at)?;
    member(&mut out, "observation", raw_or_null(&record.observation.0));
    out.push(b'}');
    compact(&out, control)
}
pub(super) fn bootstrap_bytes(
    raw: &[u8],
    timestamp: &Timestamp,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    let mut fields: std::collections::BTreeMap<String, Vec<u8>> = fields(raw, control)?
        .into_iter()
        .map(|(k, v)| (k, v.to_vec()))
        .collect();
    fields.insert("phase".into(), b"\"abandoned\"".to_vec());
    fields.insert("updatedAt".into(), value(timestamp)?);
    let mut out = vec![b'{'];
    for (key, raw) in fields {
        if out.len() > 1 {
            out.push(b',');
        }
        out.extend_from_slice(&value(&key)?);
        out.push(b':');
        out.extend_from_slice(&raw);
    }
    out.push(b'}');
    compact(&out, control)
}
