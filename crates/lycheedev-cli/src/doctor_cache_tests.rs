use super::*;
#[test]
fn failed_cache_verify_uses_existing_envelope_warning_and_report_fields() {
    let report = json!({"ok":false,"checked":1,"bytes":3,"missing":[],"corrupt":["a".repeat(64)],"errors":[]});
    let error=ToolkitError::new("vault.cache_integrity","vault.cache_integrity",4).with_details(json!({"result":report,"warnings":["Managed cache integrity failed; inspect missing and corrupt lists and decide on an explicit prune. Nothing was repaired or deleted."]}));
    let value = envelope(&Err(error));
    assert_eq!(value["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(value["result"], report);
    assert_eq!(value["error"]["code"], "vault.cache_integrity");
    assert_eq!(value["ok"], false);
}
#[test]
fn non_string_internal_warning_value_cannot_change_public_envelope_shape() {
    let error = ToolkitError::invalid("fixture")
        .with_details(json!({"warnings":["partial",7],"result":{"x":1}}));
    let value = envelope(&Err(error));
    assert_eq!(value["warnings"], json!([]));
    assert_eq!(value["error"]["details"]["warnings"], json!(["partial", 7]));
}
