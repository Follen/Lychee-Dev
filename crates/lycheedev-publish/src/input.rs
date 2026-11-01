//! JSON at the author boundary must retain absence and reject ambiguous keys.
use lycheedev_core::{Result, ToolkitError};
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, Visitor},
};
use serde_json::{Map, Value, value::RawValue};
use std::fmt;

pub(crate) fn invalid(path: &str, message: &str) -> ToolkitError {
    ToolkitError::new("publish.input", message, 4).with_details(serde_json::json!({"path":path}))
}

pub(crate) fn document(bytes: &[u8]) -> Result<Map<String, Value>> {
    match value(bytes)? {
        Value::Object(object) => Ok(object),
        _ => Err(invalid("$", "publishing input must be an object")),
    }
}
pub(crate) fn value(bytes: &[u8]) -> Result<Value> {
    let raw: &RawValue = serde_json::from_slice(bytes)
        .map_err(|_| invalid("$", "publishing JSON must be one valid UTF-8 value"))?;
    checked(raw, 0)
}

struct Members<'a>(Vec<(String, &'a RawValue)>);
impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Members<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("object")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut members = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    members.push((key, map.next_value::<&RawValue>()?));
                }
                Ok(Members(members))
            }
        }
        de.deserialize_map(Object)
    }
}

fn checked(raw: &RawValue, depth: usize) -> Result<Value> {
    if depth >= 128 {
        return Err(invalid(
            "$",
            "publishing input exceeds the JSON nesting limit",
        ));
    }
    match raw.get().as_bytes().first() {
        Some(b'{') => {
            let Members(members) = serde_json::from_str(raw.get())
                .map_err(|_| invalid("$", "invalid publishing object"))?;
            let mut object = Map::new();
            for (key, value) in members {
                if object.contains_key(&key) {
                    return Err(invalid("$", "duplicate JSON member in publishing input"));
                }
                object.insert(key, checked(value, depth + 1)?);
            }
            Ok(Value::Object(object))
        }
        Some(b'[') => {
            let items: Vec<&RawValue> = serde_json::from_str(raw.get())
                .map_err(|_| invalid("$", "invalid publishing array"))?;
            items
                .into_iter()
                .map(|item| checked(item, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array)
        }
        _ => serde_json::from_str(raw.get()).map_err(|_| invalid("$", "invalid publishing scalar")),
    }
}

/// Hold admitted UTF-8 author text against replacement until its request ends.
pub(crate) fn locked_text(
    path: &std::path::Path,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(std::fs::File, String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let (mut file, sha256) =
        lycheedev_core::storage::locked_sha256(path, 32 * 1024 * 1024, control)?;
    let result = (|| {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        control.check()?;
        let text =
            String::from_utf8(bytes).map_err(|_| invalid("$.file", "text file must be UTF-8"))?;
        if text.trim().trim_matches('\u{feff}').trim().is_empty() {
            return Err(invalid("$.file", "text file must not be blank"));
        }
        Ok(text)
    })();
    match result {
        Ok(text) => Ok((file, sha256, text)),
        Err(error) => lycheedev_core::storage::finish_file(file, Err(error), control),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicate_members_at_any_depth_without_exposing_values() {
        for raw in [
            r#"{"schema":"x","file":"private","f\u0069le":"secret"}"#,
            r#"{"relations":{"projects":[{"slug":"x","slug":"secret"}]}}"#,
        ] {
            let error = document(raw.as_bytes()).unwrap_err();
            assert_eq!(error.code, "publish.input");
            let public = serde_json::to_string(&error).unwrap();
            assert!(!public.contains("secret"));
            assert!(!public.contains("private"));
        }
        assert!(document(b"{} {}").is_err());
        assert!(document(b"{\"name\":\"\xff\"}").is_err());
    }
    #[test]
    fn preserves_exact_numbers_and_present_empty_values() {
        let value = document(br#"{"id":9007199254740993,"zero":0,"null":null,"false":false,"empty":[],"decimal":0.00000000000000000001}"#).unwrap();
        assert_eq!(value["id"].to_string(), "9007199254740993");
        assert_eq!(value["zero"], 0);
        assert!(value["null"].is_null());
        assert_eq!(value["false"], false);
        assert_eq!(value["empty"], serde_json::json!([]));
        assert_eq!(value["decimal"].to_string(), "0.00000000000000000001");
        assert!(!value.contains_key("omitted"));
    }
    #[test]
    fn serde_private_marker_names_remain_ordinary_user_objects() {
        let parsed=document(br#"{"id":{"$serde_json::private::Number":"719"},"layout":[{"$serde_json::private::RawValue":"[1,2]"}]}"#).unwrap();
        assert!(parsed["id"].is_object());
        assert_eq!(parsed["id"]["$serde_json::private::Number"], "719");
        assert!(parsed["layout"][0].is_object());
        assert_eq!(
            parsed["layout"][0]["$serde_json::private::RawValue"],
            "[1,2]"
        );
    }
}
