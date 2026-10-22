//! Current project and lock documents.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Project { schema: String, product: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock { schema: String, selection: Box<serde_json::value::RawValue> }
pub(super) fn project<C: FnMut() -> Result<()>>(raw: &[u8], scope: &mut Scope<'_, C>) -> Result<String> {
    scope.check()?;
    let value: Project = serde_json::from_slice(raw).map_err(|_| format())?;
    if value.schema != "lycheedev.project.v1" || !valid_product(&value.product) { return Err(format()); }
    scope.check()?;
    Ok(value.product)
}
pub(super) fn lock<C: FnMut() -> Result<()>>(raw: &[u8], scope: &mut Scope<'_, C>) -> Result<PinnedSet> {
    scope.check()?;
    let value: Lock = serde_json::from_slice(raw).map_err(|_| format())?;
    if value.schema != "lycheedev.project-lock.v1" { return Err(format()); }
    let pin: PinnedSet = serde_json::from_str(value.selection.get()).map_err(|_| format())?;
    let pin = super::super::file::stored_pin(value.selection.get().as_bytes(), &pin.id, scope.control).map_err(|cause| ToolkitError::new("selection.project_format", format!("selection.project_format: {}", cause.message), 4))?;
    scope.check()?;
    Ok(pin)
}
