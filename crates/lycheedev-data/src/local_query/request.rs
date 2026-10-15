//! Fixed Go command.decodeDataQuery admission. Inspect raw scalar tokens before
//! serde's arbitrary-precision Value representation can interpret an object.
use super::*;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor},
};
use std::fmt;

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Parameter {
    Number(Box<serde_json::value::RawValue>),
    Other(Value),
}
impl Parameter {
    fn parse(raw: Box<serde_json::value::RawValue>) -> Result<Self> {
        let token = raw.get();
        if token.starts_with(['[', '{']) {
            return Err(invalid());
        }
        if token
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'-')
        {
            Ok(Self::Number(raw))
        } else {
            Ok(Self::Other(
                serde_json::from_str(token).map_err(|_| invalid())?,
            ))
        }
    }
    fn binding(&self) -> Result<relational::Scalar> {
        match self {
            Self::Number(raw) => relational::literal(raw.get()),
            Self::Other(value) => super::scalar(value.clone()),
        }
    }
}
#[derive(Serialize)]
pub(super) struct Request {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hotfix: Vec<String>,
    pub sql: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, Parameter>,
}
impl Request {
    pub fn bindings(&self) -> Result<BTreeMap<String, relational::Scalar>> {
        self.parameters
            .iter()
            .map(|(name, value)| Ok((name.clone(), value.binding()?)))
            .collect()
    }
    pub fn public_numbers(
        &self,
        budget: &Budget,
    ) -> Result<lycheedev_core::public_json::NumberLayout> {
        use lycheedev_core::public_json::{JsonNumber, NumberLayout};
        if self.parameters.is_empty() {
            return Ok(NumberLayout::Verbatim);
        }
        let mut numbers = BTreeMap::new();
        for (name, parameter) in &self.parameters {
            budget.check()?;
            if let Parameter::Number(raw) = parameter {
                budget.charge(Cost::retained(
                    name.len()
                        + raw.get().len().saturating_mul(2)
                        + super::number_tree_bytes::<String>(numbers.len() + 1)
                        - super::number_tree_bytes::<String>(numbers.len()),
                ))?;
                numbers.insert(
                    name.clone(),
                    NumberLayout::Exact(JsonNumber::new(raw.get())?),
                );
            }
        }
        budget.charge(Cost::retained(
            super::number_tree_bytes::<String>(1) + "parameters".len(),
        ))?;
        Ok(NumberLayout::field(
            "parameters",
            NumberLayout::Object(numbers),
        ))
    }
    pub fn admit_encoded(&self, budget: &Budget) -> Result<usize> {
        let bytes = capture::json_bytes_with_formatter(
            self,
            budget,
            1 << 20,
            || {
                fault(
                    "relational.resource_limit",
                    3,
                    "SQL request exceeds its bound",
                )
            },
            capture::GoCaptureFormatter,
        )?;
        Ok(bytes.len())
    }
    pub fn digest(&self, budget: &Budget) -> Result<String> {
        capture::json_sha256(self, budget, capture::GoCaptureFormatter)
    }
}
/// Restore only the owned parameter tokens at the archive seam. Keep the
/// existing query/result field ordering and capture formatter for ordinary
/// inputs; json.Number's -0 must not round-trip through Value normalization.
pub(super) struct CaptureBody<'a> {
    pub reading: &'a Value,
    pub request: &'a Request,
}
struct QueryProjection<'a> {
    value: &'a Value,
    request: &'a Request,
}
impl Serialize for QueryProjection<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let object = self
            .value
            .as_object()
            .ok_or_else(|| serde::ser::Error::custom("SQL query must be an object"))?;
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for (key, value) in object {
            if key == "parameters" {
                map.serialize_entry(key, &self.request.parameters)?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}
impl Serialize for CaptureBody<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let object = self
            .reading
            .as_object()
            .ok_or_else(|| serde::ser::Error::custom("SQL result must be an object"))?;
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for (key, value) in object {
            if key == "query" {
                map.serialize_entry(
                    key,
                    &QueryProjection {
                        value,
                        request: self.request,
                    },
                )?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

fn invalid() -> lycheedev_core::ToolkitError {
    fault(
        "command.invalid_arguments",
        2,
        "query JSON requires sql string, optional scalar parameters object and optional hotfix capture ID array (1..16); duplicate/unknown fields are invalid",
    )
}
fn parameters<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, Parameter>, D::Error> {
    struct Parameters;
    impl<'de> Visitor<'de> for Parameters {
        type Value = BTreeMap<String, Parameter>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("scalar parameters object")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some(name) = map.next_key::<String>()? {
                if result.contains_key(&name) {
                    return Err(serde::de::Error::custom("duplicate parameter"));
                }
                let raw = map.next_value::<Box<serde_json::value::RawValue>>()?;
                let value = Parameter::parse(raw).map_err(serde::de::Error::custom)?;
                result.insert(name, value);
            }
            Ok(result)
        }
    }
    d.deserialize_map(Parameters)
}
fn hotfix<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<String>, D::Error> {
    // encoding/json accepts null string elements as the string zero value.
    let ids = Vec::<Option<String>>::deserialize(d)?;
    if ids.is_empty() || ids.len() > 16 {
        return Err(serde::de::Error::custom("hotfix count"));
    }
    Ok(ids.into_iter().map(Option::unwrap_or_default).collect())
}
fn decode(raw: &[u8], control: &lycheedev_core::control::OperationControl) -> Result<Request> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        sql: String,
        #[serde(default, deserialize_with = "parameters")]
        parameters: BTreeMap<String, Parameter>,
        #[serde(default, deserialize_with = "hotfix")]
        hotfix: Vec<String>,
    }
    let normalized = evidence::normalize_go_json_strings(raw, control).map_err(|error| {
        if matches!(
            error.code.as_str(),
            "command.cancelled" | "command.deadline"
        ) {
            error
        } else {
            invalid()
        }
    })?;
    let wire: Wire = serde_json::from_slice(&normalized).map_err(|_| invalid())?;
    if wire.sql.is_empty() {
        return Err(invalid());
    }
    Ok(Request {
        sql: wire.sql,
        parameters: wire.parameters,
        hotfix: wire.hotfix,
    })
}
pub(super) fn read(inv: &Invocation, budget: &Budget) -> Result<Request> {
    if [
        inv.flag("--sql").is_some(),
        inv.flag("--file").is_some(),
        inv.flag_bool("--stdin"),
    ]
    .iter()
    .filter(|value| **value)
    .count()
        != 1
    {
        return Err(fault(
            "command.invalid_arguments",
            2,
            "Select exactly one --sql, --file or --stdin",
        ));
    }
    let document = inv.flag("--file").is_some_and(|path| {
        Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    });
    if document && !inv.flag_values("--param").is_empty() {
        return Err(fault(
            "command.invalid_arguments",
            2,
            "--param cannot be combined with a JSON query document",
        ));
    }
    budget.check()?;
    let raw = if let Some(sql) = inv.flag("--sql") {
        if sql.len() > 1 << 20 {
            return Err(invalid());
        }
        budget.charge(Cost::metadata(sql.len()))?;
        sql.as_bytes().to_vec()
    } else {
        let mut raw = Vec::new();
        let mut input: Box<dyn Read> = if let Some(path) = inv.flag("--file") {
            let file = std::fs::File::open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() > 1 << 20 {
                return Err(fault(
                    "command.invalid_arguments",
                    2,
                    "SQL input must be a regular file at most 1 MiB",
                ));
            }
            Box::new(file)
        } else {
            Box::new(std::io::stdin())
        };
        let _scratch = budget.reserve_retained(65536)?;
        let mut chunk = [0u8; 65536];
        loop {
            budget.check()?;
            let count = match input.read(&mut chunk) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            budget.check()?;
            if count == 0 {
                break;
            }
            if count > (1 << 20) - raw.len() {
                return Err(invalid());
            }
            budget.charge(Cost::metadata(count))?;
            raw.extend_from_slice(&chunk[..count]);
        }
        raw
    };
    let sql = std::str::from_utf8(&raw).map_err(|_| invalid())?;
    if document {
        let _parser = budget.reserve_retained(raw.len().saturating_mul(32).saturating_add(4096))?;
        let query = decode(&raw, &inv.control)?;
        budget.charge(Cost::retained(raw.len().saturating_mul(8)))?;
        budget.check()?;
        return Ok(query);
    }
    if sql.trim().is_empty() {
        return Err(invalid());
    }
    let mut parameters = BTreeMap::new();
    for item in inv.flag_values("--param") {
        budget.check()?;
        let (name, value) = item
            .split_once('=')
            .ok_or_else(|| fault("command.invalid_arguments", 2, "--param needs name=scalar"))?;
        if !definitions::valid_name(name) || parameters.contains_key(name) {
            return Err(invalid());
        }
        let _parser =
            budget.reserve_retained(value.len().saturating_mul(3).saturating_add(4096))?;
        let normalized = match evidence::normalize_go_json_strings(value.as_bytes(), &inv.control) {
            Ok(normalized) => normalized,
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "command.cancelled" | "command.deadline"
                ) =>
            {
                return Err(error);
            }
            Err(_) => Cow::Borrowed(value.as_bytes()),
        };
        budget.charge(Cost::metadata(
            value
                .len()
                .saturating_mul(4)
                .saturating_add(name.len() + 256),
        ))?;
        let value = serde_json::from_slice::<Box<serde_json::value::RawValue>>(&normalized)
            .ok()
            .and_then(|raw| Parameter::parse(raw).ok())
            .unwrap_or_else(|| Parameter::Other(json!(value)));
        parameters.insert(name.to_owned(), value);
    }
    Ok(Request {
        sql: sql.into(),
        parameters,
        hotfix: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decode(raw: &[u8]) -> Result<Request> {
        super::decode(raw, &Default::default())
    }
    #[test]
    fn go_json_explicit_hotfix_exact_integer_and_strict_tokens() {
        let query = decode(br#"{"sql":"SELECT ID FROM effective.Sample WHERE ID=:id","parameters":{"id":9007199254740993},"hotfix":["CAP-first","CAP-second"]}"#).unwrap();
        assert_eq!(query.hotfix, ["CAP-first", "CAP-second"]);
        assert_eq!(
            serde_json::to_string(&query.parameters["id"]).unwrap(),
            "9007199254740993"
        );
        for raw in [
            r#"{"sql":"s","hotfix":[]}"#,
            r#"{"sql":"s","hotfix":null}"#,
            r#"{"sql":"s","hotfix":[42]}"#,
            r#"{"sql":"s","hotfix":["a"],"hotfix":["b"]}"#,
            r#"{"sql":"s","parameters":null}"#,
            r#"{"sql":"s","parameters":{"x":1,"x":2}}"#,
            r#"{"sql":"s","parameters":{"x":[]}}"#,
            r#"{"sql":"s","parameters":{"x":{"$serde_json::private::Number":"1"}}}"#,
            r#"{"SQL":"s"}"#,
            r#"{"sql":""}"#,
            r#"{"sql":null}"#,
            r#"{"sql":"s","extra":0}"#,
            r#"{"sql":"s"} {}"#,
            r#"{"sql":"\uGGGG"}"#,
            r#"{"sql":"\u123"}"#,
        ] {
            assert_eq!(
                decode(raw.as_bytes()).err().unwrap().code,
                "command.invalid_arguments",
                "{raw}"
            );
        }
        let raw = format!(
            r#"{{"sql":"s","hotfix":[{}]}}"#,
            vec!["\"a\""; 17].join(",")
        );
        assert!(decode(raw.as_bytes()).is_err());
        assert_eq!(
            decode(br#"{"sql":" ","hotfix":[null],"parameters":{"x":"\ud800"}}"#)
                .unwrap()
                .hotfix,
            [""]
        );
    }
    #[test]
    fn go_query_digest_keeps_hotfix_order_and_number_lexemes() {
        let query = decode(
            br#"{"parameters":{"z":1e2,"a":-0},"sql":"SELECT :a, :z","hotfix":["CAP-b","CAP-a"]}"#,
        )
        .unwrap();
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(10),
            None,
        );
        let expected =
            br#"{"hotfix":["CAP-b","CAP-a"],"sql":"SELECT :a, :z","parameters":{"a":-0,"z":1e2}}"#;
        assert_eq!(
            query.digest(&budget).unwrap(),
            lycheedev_core::storage::sha256(expected)
        );
    }
    #[test]
    fn go_request_escaped_size_and_numeric_ranges_are_not_rounded() {
        let raw = format!(
            r#"{{"sql":"SELECT :text FROM T","parameters":{{"text":"{}"}}}}"#,
            "<".repeat(200_000)
        );
        let request = decode(raw.as_bytes()).unwrap();
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        assert_eq!(
            request.admit_encoded(&budget).unwrap_err().code,
            "relational.resource_limit"
        );
        for token in ["18446744073709551616", "-9223372036854775809", "1e999"] {
            let raw = format!(r#"{{"sql":"SELECT :n FROM T","parameters":{{"n":{token}}}}}"#);
            let request = decode(raw.as_bytes()).unwrap();
            assert_eq!(
                request.bindings().unwrap_err().code,
                "relational.numeric_range"
            );
        }
        let request = decode(br#"{"sql":"SELECT :zero FROM T","parameters":{"zero":-0}}"#).unwrap();
        assert_eq!(
            request.bindings().unwrap()["zero"],
            relational::Scalar::Int(0)
        );
    }
}
