//! Current typed selection documents.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    #[serde(default)]
    parent: String,
    #[serde(default)]
    source: Option<SourcePin>,
    #[serde(default)]
    data: Option<DataPin>,
    #[serde(default)]
    changes: Option<ChangePin>,
}
impl From<Spec> for SelectionSpec {
    fn from(value: Spec) -> Self {
        Self { parent: value.parent, source: value.source, data: value.data, changes: value.changes }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    schema: String,
    id: String,
    #[serde(default)]
    parent: String,
    #[serde(default)]
    source: Option<SourcePin>,
    #[serde(default)]
    data: Option<DataPin>,
    #[serde(default)]
    changes: Option<ChangePin>,
}
pub(super) struct Decoded { pub spec: SelectionSpec }
pub(super) fn decode(raw: &[u8], control: &OperationControl) -> Result<Decoded> {
    control.check()?;
    let value: Spec = serde_json::from_slice(raw).map_err(|e| failed(e.to_string()))?;
    let mut spec = value.into();
    normalize_time(&mut spec)?;
    control.check()?;
    Ok(Decoded { spec })
}
pub(super) fn stored(raw: &[u8], control: &OperationControl) -> Result<PinnedSet> {
    control.check()?;
    let value: Stored = serde_json::from_slice(raw).map_err(|e| failed(e.to_string()))?;
    let mut spec = SelectionSpec { parent: value.parent, source: value.source, data: value.data, changes: value.changes };
    normalize_time(&mut spec)?;
    control.check()?;
    Ok(PinnedSet { schema: value.schema, id: value.id, spec })
}
fn normalize_time(spec: &mut SelectionSpec) -> Result<()> {
    if let Some(change) = &mut spec.changes {
        let time = storage::parse_timestamp(&change.fetched_at).ok_or_else(|| failed("invalid selection timestamp"))?;
        change.fetched_at = storage::timestamp_offset(time);
    }
    Ok(())
}
