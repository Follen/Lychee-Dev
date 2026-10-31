//! Go's optional pinned-definition decoding. CLI remote queries do not invent a definition.
use super::*;
use crate::schema::{Definition, Field};
use std::collections::BTreeMap;
fn fields(message: impl Into<String>) -> ToolkitError {
    error("fields", 4, message)
}
fn scalar(value: Value, field: &Field) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    let number = || match &value {
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.clone()),
        _ => Err(fields("expected number")),
    };
    match field.kind.as_str() {
        "int" => {
            let text = number()?;
            if field.signed {
                let v = text
                    .parse::<i64>()
                    .map_err(|_| fields(format!("expected integer, got {text:?}")))?;
                if field.bits < 64
                    && (field.bits == 0
                        || v < -(1i64 << (field.bits - 1))
                        || v > (1i64 << (field.bits - 1)) - 1)
                {
                    return Err(fields(format!("value {v} does not fit int{}", field.bits)));
                }
                Ok(json!(v))
            } else {
                let v = text
                    .parse::<u64>()
                    .map_err(|_| fields(format!("expected unsigned integer, got {text:?}")))?;
                if field.bits < 64 && v >= (1u64 << field.bits) {
                    return Err(fields(format!("value {v} does not fit uint{}", field.bits)));
                }
                Ok(json!(v))
            }
        }
        "float" => {
            let text = number()?;
            let v = text
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| fields(format!("expected finite number, got {text:?}")))?;
            Ok(json!(v))
        }
        "string" | "locstring" if value.is_string() => Ok(value),
        "string" | "locstring" => Err(fields("expected string")),
        _ => Err(fields(format!(
            "unsupported definition kind {:?}",
            field.kind
        ))),
    }
}
fn positional_with_check(
    raw: &str,
    definition: &Definition,
    budget: &Budget,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<BTreeMap<String, Value>> {
    check()?;
    budget.check()?;
    if definition.fields.is_empty() {
        return Err(fields("records.hotfix_fields"));
    }
    let decoded = serde_json::from_str::<Value>(raw)
        .map_err(|e| fields(format!("positional payload is invalid JSON: {e}")));
    let value = finish_stop(decoded, check())?;
    if let Value::Object(object) = value {
        return Ok(object.into_iter().collect());
    }
    let Value::Array(columns) = value else {
        return Err(fields("expected positional array or object"));
    };
    if columns.len() > definition.fields.len() {
        return Err(fields(format!(
            "{} values for {} definition fields",
            columns.len(),
            definition.fields.len()
        )));
    }
    let mut result = BTreeMap::new();
    for (value, field) in columns.into_iter().zip(&definition.fields) {
        check()?;
        budget.check()?;
        let converted = if value.is_null() {
            Value::Null
        } else if field.array {
            let Value::Array(values) = value else {
                return Err(fields(format!(
                    "{}: array field carries non-array",
                    field.name
                )));
            };
            if values.len() != field.elements as usize {
                return Err(fields(format!(
                    "{}: array length {} does not match {} elements",
                    field.name,
                    values.len(),
                    field.elements
                )));
            }
            Value::Array(
                values
                    .into_iter()
                    .map(|v| {
                        check()?;
                        budget.check()?;
                        scalar(v, field)
                    })
                    .collect::<Result<Vec<_>>>()?,
            )
        } else {
            scalar(value, field)?
        };
        result.insert(field.name.clone(), converted);
    }
    Ok(result)
}
pub(super) fn apply_with_check(
    records: &mut [Record],
    definition: &Definition,
    budget: &Budget,
    mut check: impl FnMut() -> Result<()>,
) -> Result<()> {
    for record in records {
        check()?;
        budget.check()?;
        record.decode_state = "no_payload".into();
        let Some(payload) = &record.payload else {
            continue;
        };
        if record.status != 1 {
            record.decode_state = "not_valid".into();
            continue;
        }
        record.fields = Some(
            positional_with_check(payload.get(), definition, budget, &mut check).map_err(
                |mut e| {
                    e.message = format!("record id {}: {}", record.id, e.message);
                    e
                },
            )?,
        );
        record.decode_state = "decoded".into();
    }
    check()?;
    budget.check()
}
