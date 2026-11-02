//! Keep the typed result through every frame. Never route RawReport through
//! serde_json::Value: that destroys duplicate keys and can change number text.
use super::*;
use std::io::Write;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Fault {
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
    code: String,
    message: String,
    stage: String,
    retryable: bool,
    resume_operation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<Value>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    schema: &'static str,
    ok: bool,
    operation_id: &'a str,
    context: &'a BTreeMap<String, Value>,
    result: &'a Option<LiveResult>,
    captures: [Value; 0],
    warnings: &'a [Value],
    error: Option<Fault>,
}
fn envelope(response: &Response) -> Envelope<'_> {
    let error = response.error.as_ref().map(|error| {
        let mut details = error.details.clone();
        if let Some(map) = details.as_object_mut() {
            for private in ["stage", "__commandContext", "location"] {
                map.remove(private);
            }
        }
        let stage = response
            .context
            .get("stage")
            .and_then(Value::as_str)
            .filter(|_| !response.operation_id.is_empty())
            .unwrap_or_else(|| error.details["stage"].as_str().unwrap_or("dispatch"));
        let resumable = !response.operation_id.is_empty() && response.context.contains_key("stage");
        let retryable = (response.context.get("transport").and_then(Value::as_str)
            == Some(TRANSPORT)
            && error.exit_code == 6)
            || resumable && (error.code == "journal.resource_busy" || error.exit_code == 6);
        Fault {
            details: (!details.is_null() && !details.as_object().is_some_and(|m| m.is_empty()))
                .then_some(details),
            code: error.code.clone(),
            message: error.message.clone(),
            stage: stage.into(),
            retryable,
            resume_operation_id: if resumable {
                response.operation_id.clone()
            } else {
                String::new()
            },
            location: error.details.get("location").cloned(),
        }
    });
    Envelope {
        schema: "lycheedev.result.v1",
        ok: response.error.is_none(),
        operation_id: &response.operation_id,
        context: &response.context,
        result: &response.result,
        captures: [],
        warnings: &response.warnings,
        error,
    }
}
#[derive(Serialize)]
struct Record<'a> {
    result: &'a Option<LiveResult>,
    r#type: &'static str,
}
#[derive(Serialize)]
struct End<'a> {
    envelope: Envelope<'a>,
    r#type: &'static str,
}
pub fn render(out: &mut impl Write, format: &str, response: &Response) -> Result<()> {
    use lycheedev_core::public_json::{
        write_pretty_serializable_line as pretty, write_serializable_line as line,
    };
    match format {
        "json" => line(out, &envelope(response))?,
        "jsonl" => {
            // Go's map frames sort keys; the enclosed typed structs preserve
            // their declared order and embedded raw reports remain opaque JSON.
            line(out, &json!({"type":"begin","schema":"lycheedev.result.v1"}))?;
            if response.error.is_some() {
                line(
                    out,
                    &End {
                        envelope: envelope(response),
                        r#type: "error",
                    },
                )?;
            } else {
                line(
                    out,
                    &Record {
                        result: &response.result,
                        r#type: "record",
                    },
                )?;
                line(
                    out,
                    &End {
                        envelope: envelope(response),
                        r#type: "end",
                    },
                )?;
            }
        }
        _ => match &response.error {
            Some(error) => writeln!(out, "{}: {}", error.code, error.message)?,
            None => pretty(out, &response.result)?,
        },
    }
    out.flush()?;
    Ok(())
}
