//! Retention is the same permissive Go typed object as the capture manifest;
//! unknown fields disappear, duplicates merge, null preserves a prior scalar.
use super::*;
use crate::storage;
use serde_json::{Value, json};
#[derive(Default)]
struct Retention {
    schema: String,
    capture_id: String,
    blob: EmbeddedBlob,
    kept_at: GoTimestamp,
}
impl GoObject for Retention {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(map, &mut self.schema),
            "captureid" => scalar(map, &mut self.capture_id),
            "blob" => map.next_value_seed(ObjectSeed(&mut self.blob)),
            "keptat" => scalar(map, &mut self.kept_at),
            _ => ignore(map),
        }
    }
}
deserialize_object!(Retention);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Wire<'a> {
    schema: &'a str,
    capture_id: &'a str,
    blob: &'a EmbeddedBlob,
    kept_at: &'a str,
}
fn wire(record: &Retention) -> Result<Wire<'_>> {
    Ok(Wire {
        schema: &record.schema,
        capture_id: &record.capture_id,
        blob: &record.blob,
        kept_at: record.kept_at.canonical.as_deref().ok_or_else(|| {
            ToolkitError::invalid("retention timestamp offset cannot be marshaled by Go")
        })?,
    })
}
pub(in crate::evidence) fn new_record(
    id: &str,
    capture: &GoCaptureDecode,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    let now = storage::utc_now();
    let value = Wire {
        schema: "lycheedev.evidence-retention.v1",
        capture_id: id,
        blob: &capture.0.blob,
        kept_at: &now,
    };
    let result = storage::identity_json(&value);
    crate::selection::selection_finish(result, control.check())
}
pub(in crate::evidence) fn decode_retention(
    raw: &[u8],
    capture: &GoCaptureDecode,
    require_schema: bool,
    control: &OperationControl,
) -> Result<Value> {
    let record = checked_record(raw, capture, require_schema, control)?;
    let result = serde_json::to_value(wire(&record)?).map_err(Into::into);
    crate::selection::selection_finish(result, control.check())
}

// Remove validates typed fields but never marshals the ignored retention time.
pub(in crate::evidence) fn validate_removal_retention(
    raw: &[u8],
    capture: &GoCaptureDecode,
    control: &OperationControl,
) -> Result<()> {
    checked_record(raw, capture, false, control).map(|_| ())
}
fn checked_record(
    raw: &[u8],
    capture: &GoCaptureDecode,
    require_schema: bool,
    control: &OperationControl,
) -> Result<Retention> {
    control.check()?;
    let parsed = (|| {
        if crate::raw_json::validate_with_check(raw, || control.check())?.is_err() {
            return Err(ToolkitError::invalid("invalid retention JSON"));
        }
        let raw = normalize_go_strings_with_time_key(raw, control, "keptat")?;
        let result = serde_json::from_slice::<Retention>(&raw).map_err(ToolkitError::from);
        crate::selection::selection_finish(result, control.check())
    })();
    let record = match parsed {
        Ok(record) => record,
        Err(error)
            if matches!(
                error.code.as_str(),
                "command.cancelled" | "command.deadline"
            ) =>
        {
            return Err(error);
        }
        Err(error) => {
            return crate::selection::selection_finish(
                Err(ToolkitError::new(
                    "evidence.invalid_retention",
                    "evidence.invalid_retention",
                    4,
                )
                .with_details(json!({"cause":error}))),
                control.check(),
            );
        }
    };
    if require_schema && record.schema != "lycheedev.evidence-retention.v1"
        || record.capture_id != capture.0.id
        || record.blob.sha256 != capture.0.blob.sha256
        || record.blob.bytes != capture.0.blob.bytes
    {
        return crate::selection::selection_finish(
            Err(ToolkitError::new(
                "evidence.invalid_retention",
                "evidence.invalid_retention",
                4,
            )),
            control.check(),
        );
    }
    Ok(record)
}
