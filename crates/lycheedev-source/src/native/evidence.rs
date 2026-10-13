// Immutable identity trees are shared by execution views. Serialization keeps
// the canonical key order of the original serde_json::Value representation.
#[derive(Clone, Debug)]
pub struct SourceIdentityEvidence {
    common: Arc<serde_json::Map<String, Value>>,
    execution: serde_json::Map<String, Value>,
}
impl SourceIdentityEvidence {
    fn empty() -> Self {
        Self {
            common: Arc::new(serde_json::Map::new()),
            execution: serde_json::Map::new(),
        }
    }
}
impl std::ops::Index<&str> for SourceIdentityEvidence {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        self.execution
            .get(key)
            .or_else(|| self.common.get(key))
            .unwrap_or(&Value::Null)
    }
}
impl Serialize for SourceIdentityEvidence {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let fields = self
            .common
            .iter()
            .chain(self.execution.iter())
            .map(|(key, value)| (key.as_str(), value))
            .collect::<BTreeMap<_, _>>();
        let mut map = serializer.serialize_map(Some(fields.len()))?;
        for (key, value) in fields {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

#[derive(Clone, Debug)]
pub struct GraphSnapshotEvidence {
    admission: Arc<SourceIdentityEvidence>,
    common: Arc<serde_json::Map<String, Value>>,
    execution: serde_json::Map<String, Value>,
}
impl GraphSnapshotEvidence {
    pub fn admission(&self) -> &SourceIdentityEvidence {
        &self.admission
    }
}
impl Serialize for GraphSnapshotEvidence {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let fields = self
            .common
            .iter()
            .chain(self.execution.iter())
            .map(|(key, value)| (key.as_str(), value))
            .collect::<BTreeMap<_, _>>();
        let mut keys = fields.keys().copied().collect::<Vec<_>>();
        keys.push("admission");
        keys.sort_unstable();
        let mut map = serializer.serialize_map(Some(keys.len()))?;
        for key in keys {
            if key == "admission" {
                map.serialize_entry(key, self.admission.as_ref())?;
            } else {
                map.serialize_entry(key, fields[key])?;
            }
        }
        map.end()
    }
}

fn evidence_fields(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => unreachable!("identity fields are an object"),
    }
}
