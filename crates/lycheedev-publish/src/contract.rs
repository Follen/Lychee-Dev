//! Author input-shape metadata. The provider's typed form/state admission also
//! owns dependent choices, current-record projection, files and wire semantics.
use crate::input::invalid;
use crate::provider::Provider;
use lycheedev_core::{Result, ToolkitError};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
    Identifier,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    #[serde(rename = "type")]
    pub kind: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default)]
    pub choices: Vec<Value>,
    #[serde(default)]
    pub nonempty: bool,
    pub max_length: Option<usize>,
    pub max_items: Option<usize>,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    #[serde(default)]
    pub local_file: bool,
    #[serde(default)]
    pub description: String,
}

impl Field {
    fn admit(&self, name: &str, value: &Value) -> Result<()> {
        let path = format!("$.{name}");
        if value.is_null() {
            return if self.nullable {
                Ok(())
            } else {
                Err(invalid(&path, "null is not allowed"))
            };
        }
        let kind_matches = match self.kind {
            FieldType::String => value.is_string(),
            FieldType::Integer => value.is_i64() || value.is_u64(),
            FieldType::Number => value.is_number(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Array => value.is_array(),
            FieldType::Object => value.is_object(),
            FieldType::Identifier => value.is_string() || value.is_i64() || value.is_u64(),
        };
        if !kind_matches {
            return Err(invalid(&path, "field has the wrong JSON type"));
        }
        if !self.choices.is_empty() && !self.choices.contains(value) {
            return Err(invalid(&path, "field is outside the declared choices"));
        }
        let size = value
            .as_str()
            .map(|s| s.chars().count())
            .or_else(|| value.as_array().map(Vec::len))
            .or_else(|| value.as_object().map(serde_json::Map::len));
        if self.nonempty && size == Some(0) {
            return Err(invalid(&path, "field must not be empty"));
        }
        if self
            .max_length
            .is_some_and(|maximum| size.is_some_and(|n| n > maximum))
            || self
                .max_items
                .is_some_and(|maximum| size.is_some_and(|n| n > maximum))
        {
            return Err(invalid(&path, "field exceeds its declared size"));
        }
        if self.minimum.is_some_and(|min| {
            value.as_i64().is_some_and(|n| n < min)
                || value.as_u64().is_some_and(|n| min > 0 && n < min as u64)
        }) || self.maximum.is_some_and(|max| {
            value.as_i64().is_some_and(|n| n > max)
                || value.as_u64().is_some_and(|n| max < 0 || n > max as u64)
        }) {
            return Err(invalid(
                &path,
                "field is outside its declared numeric range",
            ));
        }
        Ok(())
    }
    fn schema(&self) -> Value {
        let mut types = match self.kind {
            FieldType::String => vec!["string"],
            FieldType::Integer => vec!["integer"],
            FieldType::Number => vec!["number"],
            FieldType::Boolean => vec!["boolean"],
            FieldType::Array => vec!["array"],
            FieldType::Object => vec!["object"],
            FieldType::Identifier => vec!["integer", "string"],
        };
        if self.nullable {
            types.push("null");
        }
        let mut result = json!({"type": types});
        if !self.choices.is_empty() {
            let mut choices = self.choices.clone();
            if self.nullable && !choices.contains(&Value::Null) {
                choices.push(Value::Null);
            }
            result["enum"] = json!(choices);
        }
        if !self.description.is_empty() {
            result["description"] = json!(self.description);
        }
        if let Some(value) = self.minimum {
            result["minimum"] = json!(value);
        }
        if let Some(value) = self.maximum {
            result["maximum"] = json!(value);
        }
        if let Some(value) = self.max_length {
            result["maxLength"] = json!(value);
        }
        if let Some(value) = self.max_items {
            result["maxItems"] = json!(value);
        }
        if self.nonempty {
            let keyword = match self.kind {
                FieldType::String | FieldType::Identifier => Some("minLength"),
                FieldType::Array => Some("minItems"),
                FieldType::Object => Some("minProperties"),
                _ => None,
            };
            if let Some(keyword) = keyword {
                result[keyword] = json!(1);
            }
        }
        if self.local_file {
            result["x-local-file"] = json!(true);
        }
        result
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputShape {
    pub provider: Provider,
    pub resource: String,
    pub action: String,
    pub name: String,
    pub fields: BTreeMap<String, Field>,
}

impl InputShape {
    pub(crate) fn admit(&self, value: &serde_json::Map<String, Value>) -> Result<()> {
        if value.get("schema").and_then(Value::as_str) != Some(&self.name) {
            return Err(invalid(
                "$.schema",
                "publishing input schema does not match this operation",
            ));
        }
        if value
            .keys()
            .any(|key| key != "schema" && !self.fields.contains_key(key))
        {
            return Err(invalid("$", "unknown publishing input field"));
        }
        for (name, field) in &self.fields {
            match value.get(name) {
                Some(value) => field.admit(name, value)?,
                None if field.required => {
                    return Err(invalid(&format!("$.{name}"), "field is required"));
                }
                None => {}
            }
        }
        Ok(())
    }
    /// This is the shared field projection, not remote validation or a claim
    /// that dependent selections are valid. Provider admission composes it with
    /// its nested request/state contracts before execution.
    pub fn schema(&self) -> Value {
        let mut properties = BTreeMap::new();
        properties.insert("schema".to_owned(), json!({"const": self.name}));
        let mut required = vec!["schema"];
        for (name, field) in &self.fields {
            properties.insert(name.clone(), field.schema());
            if field.required {
                required.push(name);
            }
        }
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"object","properties":properties,"required":required,
            "additionalProperties":false,"x-validation-scope":"field-shape",
            "x-presence":"Omitted fields preserve the current value on an edit/update; explicit null or empty values only clear fields that admit them."})
    }
}

fn input_shapes() -> &'static [InputShape] {
    static SHAPES: OnceLock<Vec<InputShape>> = OnceLock::new();
    SHAPES.get_or_init(|| {
        serde_json::from_str(include_str!("input-shapes.json"))
            .expect("embedded author input shapes must be valid")
    })
}

pub fn input_shape(
    provider: Provider,
    resource: &str,
    action: &str,
) -> Result<&'static InputShape> {
    input_shapes()
        .iter()
        .find(|shape| {
            shape.provider == provider && shape.resource == resource && shape.action == action
        })
        .ok_or_else(|| {
            ToolkitError::new(
                "publish.input_contract",
                "unknown publishing input contract",
                2,
            )
            .with_details(json!({"provider":provider.id(),"resource":resource,"action":action}))
        })
}
