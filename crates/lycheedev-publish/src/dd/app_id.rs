//! appId interpretation belongs to Publish; Windows owns only the fixed SDK ABI.
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use std::path::Path;

pub fn request(program_dir: &Path, control: &OperationControl) -> Result<String> {
    control.check()?;
    let response = lycheedev_windows::urs_sdk::request_app_id(program_dir)
        .map_err(|message| ToolkitError::new("publish.dd_urs_sdk", message, 3));
    // The synchronous SDK has already freed its buffers/objects and unloaded its
    // image. Its task owner must enforce cancellation during a native call.
    control.check()?;
    parse(&response?)
}
fn parse(response: &str) -> Result<String> {
    let invalid = || {
        ToolkitError::new(
            "publish.dd_urs_app_id",
            "URS SDK did not return a valid application identity",
            4,
        )
    };
    if response.len() > 65536 {
        return Err(invalid());
    }
    let value = crate::input::value(response.as_bytes()).map_err(|_| invalid())?;
    value
        .get("appId")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 4096 && !s.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_json_projects_only_app_id_and_rejects_ambiguous_or_invalid_identity_without_echo() {
        assert_eq!(
            parse(r#"{"appId":"合成-app-id","opaque":"private-sdk-value"}"#).unwrap(),
            "合成-app-id"
        );
        for text in [
            r#"{"appId":"private-sdk-value","appId":"another"}"#,
            r#"{"appId":123,"opaque":"private-sdk-value"}"#,
            r#"{"appId":"\u0000private-sdk-value"}"#,
            r#"{"appId":" "}"#,
            r#"["private-sdk-value"]"#,
            "private-sdk-value",
        ] {
            let error = parse(text).unwrap_err();
            assert_eq!(error.exit_code, 4);
            assert!(!error.to_string().contains("private-sdk-value"));
        }
        assert!(parse(&format!("{{\"appId\":\"{}\"}}", "x".repeat(4097))).is_err());
    }
    #[test]
    fn cancelled_request_never_opens_a_native_sdk() {
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            request(Path::new("nonexistent-relative"), &control)
                .unwrap_err()
                .exit_code,
            7
        );
    }
}
