// Immutable identity trees are shared by execution views. Serialization keeps
// the canonical key order of the original serde_json::Value representation.
#[derive(Debug)]
pub struct SourceIdentityEvidence {
    common: Arc<serde_json::Map<String, Value>>,
    execution: serde_json::Map<String, Value>,
    xml: Option<Arc<crate::xml_load::Program>>,
    // Compatibility only: research hashing/serialization borrows the typed
    // program; a caller explicitly indexing xmlProgram may request a Value.
    xml_value: std::sync::OnceLock<Value>,
}
impl Clone for SourceIdentityEvidence {
    fn clone(&self) -> Self {
        Self {
            common: Arc::clone(&self.common),
            execution: self.execution.clone(),
            xml: self.xml.clone(),
            xml_value: std::sync::OnceLock::new(),
        }
    }
}
impl SourceIdentityEvidence {
    fn empty() -> Self {
        Self {
            common: Arc::new(serde_json::Map::new()),
            execution: serde_json::Map::new(),
            xml: None,
            xml_value: std::sync::OnceLock::new(),
        }
    }
}
impl std::ops::Index<&str> for SourceIdentityEvidence {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        if key == "xmlProgram" && let Some(program) = &self.xml {
            return self.xml_value.get_or_init(|| json!(program.as_ref()));
        }
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
        let mut keys = fields.keys().copied().collect::<Vec<_>>();
        if self.xml.is_some() {
            if !fields.contains_key("xmlProgram") { keys.push("xmlProgram"); }
            keys.sort_unstable();
        }
        let mut map = serializer.serialize_map(Some(keys.len()))?;
        for key in keys {
            if key == "xmlProgram" && let Some(program) = &self.xml {
                map.serialize_entry(key, &CanonicalXmlProgram(program))?;
            } else {
                map.serialize_entry(key, fields[key])?;
            }
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

// These local views preserve json!(Program)'s recursively sorted object keys
// without cloning the XML program into a second, untyped tree. Nested Values
// and document maps already have canonical key order.
struct CanonicalXmlProgram<'a>(&'a crate::xml_load::Program);
impl Serialize for CanonicalXmlProgram<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let program = self.0;
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("actions", &CanonicalXmlActions(program.actions()))?;
        map.serialize_entry("context", &CanonicalXmlContext(&program.context))?;
        map.serialize_entry("documents", &program.documents)?;
        map.serialize_entry("gaps", &program.gaps)?;
        map.serialize_entry("manifest", &program.manifest)?;
        map.serialize_entry("rules", program.rules)?;
        map.end()
    }
}
struct CanonicalXmlContext<'a>(&'a crate::xml_load::LoadContext);
impl Serialize for CanonicalXmlContext<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let context = self.0;
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("assumption", context.assumption)?;
        map.serialize_entry("basis", &context.basis)?;
        map.serialize_entry("family", &context.family)?;
        map.serialize_entry("game", &context.game)?;
        map.serialize_entry("mode", &context.mode)?;
        map.serialize_entry("phase", context.phase)?;
        map.serialize_entry("uncertainties", &context.uncertainties)?;
        map.end()
    }
}
struct CanonicalXmlOrigin<'a>(&'a crate::xml_load::XmlOrigin);
impl Serialize for CanonicalXmlOrigin<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // Span's derived order is start/end; the old Value order is end/start.
        #[derive(Serialize)]
        struct CanonicalSpan { end: usize, start: usize }
        let origin = self.0;
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("bytes", &CanonicalSpan { end: origin.bytes.end, start: origin.bytes.start })?;
        map.serialize_entry("path", &origin.path)?;
        map.serialize_entry("sha256", &origin.sha256)?;
        map.end()
    }
}
struct CanonicalXmlReference<'a>(&'a crate::xml_load::XmlReference);
impl Serialize for CanonicalXmlReference<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("name", &self.0.name)?;
        map.serialize_entry("origin", &CanonicalXmlOrigin(&self.0.origin))?;
        map.end()
    }
}
struct CanonicalXmlHandlerTarget<'a>(&'a crate::xml_load::HandlerTarget);
impl Serialize for CanonicalXmlHandlerTarget<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use crate::xml_load::HandlerTarget;
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(if matches!(self.0, HandlerTarget::Empty) { 1 } else { 2 }))?;
        match self.0 {
            HandlerTarget::Inline { file } => {
                map.serialize_entry("file", file)?;
                map.serialize_entry("kind", "inline")?;
            }
            HandlerTarget::Method { reference } | HandlerTarget::Function { reference } => {
                map.serialize_entry("kind", if matches!(self.0, HandlerTarget::Method { .. }) { "method" } else { "function" })?;
                map.serialize_entry("reference", &CanonicalXmlReference(reference))?;
            }
            HandlerTarget::Empty => map.serialize_entry("kind", "empty")?,
            HandlerTarget::Unavailable { reason } => {
                map.serialize_entry("kind", "unavailable")?;
                map.serialize_entry("reason", reason)?;
            }
        }
        map.end()
    }
}
struct CanonicalXmlActions<'a>(&'a [crate::xml_load::Action]);
impl Serialize for CanonicalXmlActions<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for action in self.0 { seq.serialize_element(&CanonicalXmlAction(action))?; }
        seq.end()
    }
}
struct CanonicalXmlAction<'a>(&'a crate::xml_load::Action);
impl Serialize for CanonicalXmlAction<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use crate::xml_load::Action;
        use serde::ser::SerializeMap;
        let fields = match self.0 {
            Action::Lua { .. } => 2,
            Action::Object { .. } => 6,
            Action::Mixin { .. } => 3,
            Action::ParentField { .. } | Action::Handler { .. } => 5,
            Action::Definition { .. } => 4,
        };
        let mut map = serializer.serialize_map(Some(fields))?;
        match self.0 {
            Action::Lua { file } => {
                map.serialize_entry("file", file)?;
                map.serialize_entry("kind", "lua")?;
            }
            Action::Object { instance, origin, tag, name, parent } => {
                map.serialize_entry("instance", instance)?;
                map.serialize_entry("kind", "object")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("origin", &CanonicalXmlOrigin(origin))?;
                map.serialize_entry("parent", parent)?;
                map.serialize_entry("tag", tag)?;
            }
            Action::Mixin { instance, reference } => {
                map.serialize_entry("instance", instance)?;
                map.serialize_entry("kind", "mixin")?;
                map.serialize_entry("reference", &CanonicalXmlReference(reference))?;
            }
            Action::ParentField { parent, parent_reference, child, reference } => {
                map.serialize_entry("child", child)?;
                map.serialize_entry("kind", "parent_field")?;
                map.serialize_entry("parent", parent)?;
                map.serialize_entry("parent_reference", &parent_reference.as_ref().map(CanonicalXmlReference))?;
                map.serialize_entry("reference", &CanonicalXmlReference(reference))?;
            }
            Action::Handler { instance, handler, target, origin } => {
                map.serialize_entry("handler", handler)?;
                map.serialize_entry("instance", instance)?;
                map.serialize_entry("kind", "handler")?;
                map.serialize_entry("origin", &CanonicalXmlOrigin(origin))?;
                map.serialize_entry("target", &CanonicalXmlHandlerTarget(target))?;
            }
            Action::Definition { name, intrinsic, origin } => {
                map.serialize_entry("intrinsic", intrinsic)?;
                map.serialize_entry("kind", "definition")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("origin", &CanonicalXmlOrigin(origin))?;
            }
        }
        map.end()
    }
}

/// The public JSON string is produced only if a consumer asks for it. Admission
/// already serialized these immutable facts through the original byte ceiling
/// and operation control; ordinary research only needs their digest.
#[derive(Debug)]
pub struct SnapshotInputsJson {
    evidence: Arc<GraphSnapshotEvidence>,
    json: std::sync::OnceLock<String>,
    bytes: usize,
}
impl SnapshotInputsJson {
    fn new(evidence: Arc<GraphSnapshotEvidence>, control: &OperationControl, limit: usize) -> lycheedev_core::Result<(Self, String)> {
        let (sha256, bytes) = serialize_json_hash(evidence.as_ref(), control, limit)?;
        Ok((Self { evidence, json: std::sync::OnceLock::new(), bytes }, sha256))
    }
    #[cfg(test)]
    fn from_serialized(evidence: Arc<GraphSnapshotEvidence>, json: String) -> Self {
        let bytes = json.len();
        Self { evidence, json: std::sync::OnceLock::from(json), bytes }
    }
    #[cfg(test)]
    fn is_materialized(&self) -> bool { self.json.get().is_some() }
    pub fn len(&self) -> usize { self.bytes }
    pub fn is_empty(&self) -> bool { self.bytes == 0 }
    pub fn as_str(&self) -> &str {
        self.json.get_or_init(|| {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(self.bytes).expect("validated snapshot JSON allocation");
            serde_json::to_writer(&mut bytes, self.evidence.as_ref()).expect("validated immutable snapshot inputs");
            debug_assert_eq!(bytes.len(), self.bytes);
            String::from_utf8(bytes).expect("JSON is UTF8")
        })
    }
    pub fn as_bytes(&self) -> &[u8] { self.as_str().as_bytes() }
}
impl Clone for SnapshotInputsJson {
    fn clone(&self) -> Self {
        // Do not duplicate a possibly large materialization when graph facts
        // are cloned; both copies still refer to the same immutable evidence.
        Self { evidence: Arc::clone(&self.evidence), json: std::sync::OnceLock::new(), bytes: self.bytes }
    }
}
impl std::ops::Deref for SnapshotInputsJson {
    type Target = str;
    fn deref(&self) -> &str { self.as_str() }
}
impl Serialize for SnapshotInputsJson {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(test)]
#[path = "identity_scope_tests.rs"]
mod identity_scope_tests;
