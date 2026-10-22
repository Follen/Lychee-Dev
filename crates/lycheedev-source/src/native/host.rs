// Authenticated host facts are data, never investigated Lua or analyzer type overrides.
const HOST_BINDING_LIMIT: usize = 20000;
const HOST_MATERIAL_LIMIT: usize = 16;
const HOST_BYTE_LIMIT: usize = 64<<20;
const HOST_JSON_NODE_LIMIT: usize = 4000000;
const HOST_JSON_DEPTH_LIMIT: usize = 32;
const HOST_STRING_LIMIT: usize = 65536;
const HOST_SLOT_LIMIT: usize = 32;

/// Only trusted host code supplies expected hashes; repository claims are not authority.
#[derive(Clone, Debug)]
pub struct HostMaterialInput { pub name: String, pub bytes: Vec<u8>, pub expected_sha256: String }

#[derive(Clone, Debug, Serialize)]
pub struct IntrinsicDeclaration {
    pub file: String, pub source_sha256: String, pub start: u32, pub end: u32, pub name: String,
    pub rule: String, pub host_material: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HostMaterialRef { pub material: String, pub pointer: String }

/// No LuaType, diagnostic, source span, or executable configuration crosses this API.
#[derive(Clone, Debug)]
pub enum HostBindingInput {
    Callable { declaration: HostMaterialRef, rules: Option<HostMaterialRef> },
    Method { declaration: HostMaterialRef },
    Predicate { declaration: HostMaterialRef },
    EmptyObject { declaration: HostMaterialRef },
}

/// A JSON record origin. It deliberately has no file/start/end source coordinates.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct EnvironmentEvidence {
    material: String,
    material_sha256: String,
    pointer: String,
    identity: String,
}
impl EnvironmentEvidence {
    pub fn material(&self) -> &str { &self.material }
    pub fn material_sha256(&self) -> &str { &self.material_sha256 }
    pub fn pointer(&self) -> &str { &self.pointer }
    pub fn identity(&self) -> &str { &self.identity }
}

#[derive(Clone, Debug, Serialize)]
pub struct HostRuleEvidence {
    evidence: EnvironmentEvidence,
    // Original labels and metadata stay intact. A "secret" label is potential risk,
    // never a proof that issecretvalue returns true or a claim of a Lua source site.
    record: Value,
}
impl HostRuleEvidence {
    pub fn evidence(&self) -> &EnvironmentEvidence { &self.evidence }
    pub fn record(&self) -> &Value { &self.record }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum HostBindingKind {
    Callable { return_arity: Option<usize>, rules: Option<HostRuleEvidence>, effect:HostEffect, #[serde(skip_serializing_if="Option::is_none")] receiver_type:Option<String>, #[serde(skip_serializing_if="Option::is_none")] descriptor:Option<Value>, #[serde(skip)] native_descriptor:Option<Arc<EnvironmentCallable>> },
    Predicate { argument: usize, return_arity: usize },
    EmptyObject,
}
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum HostEffect { NoLuaHeapWrite, Unknown }

#[derive(Clone, Debug)]
pub struct HostBinding { data: Arc<HostBindingData> }
impl Serialize for HostBinding {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{self.data.as_ref().serialize(serializer)}
}
#[derive(Clone, Debug, Serialize)]
struct HostBindingData {
    name: String,
    identity: String,
    declaration: EnvironmentEvidence,
    #[serde(flatten)]
    kind: HostBindingKind,
}
impl HostBinding {
    pub fn name(&self) -> &str { &self.data.name }
    pub fn identity(&self) -> &str { &self.data.identity }
    pub fn declaration(&self) -> &EnvironmentEvidence { &self.data.declaration }
    pub fn kind(&self) -> &'static str {
        match self.data.kind {
            HostBindingKind::Callable { .. } => "callable",
            HostBindingKind::Predicate { .. } => "predicate",
            HostBindingKind::EmptyObject => "empty_object",
        }
    }
    /// None means unknown/open tail for a callable, never the maximum risk-rule slot.
    pub fn return_arity(&self) -> Option<usize> {
        match self.data.kind {
            HostBindingKind::Callable { return_arity, .. } => return_arity,
            HostBindingKind::Predicate { return_arity, .. } => Some(return_arity),
            HostBindingKind::EmptyObject => None,
        }
    }
    pub fn rules(&self) -> Option<&HostRuleEvidence> {
        match &self.data.kind { HostBindingKind::Callable { rules, .. } => rules.as_ref(), _ => None }
    }
    pub fn predicate_argument(&self) -> Option<usize> {
        match self.data.kind { HostBindingKind::Predicate { argument, .. } => Some(argument), _ => None }
    }
    pub fn effect(&self)->HostEffect {
        match &self.data.kind {HostBindingKind::Callable{effect,..}=>effect.clone(),_=>HostEffect::NoLuaHeapWrite}
    }
    pub fn descriptor(&self)->Option<&Value>{match &self.data.kind{HostBindingKind::Callable{descriptor,..}=>descriptor.as_ref(),_=>None}}
    pub fn native_descriptor(&self)->Option<&Arc<EnvironmentCallable>>{match &self.data.kind{HostBindingKind::Callable{native_descriptor,..}=>native_descriptor.as_ref(),_=>None}}
    pub fn receiver_type(&self)->Option<&str>{match &self.data.kind{HostBindingKind::Callable{receiver_type,..}=>receiver_type.as_deref(),_=>None}}
}

#[derive(Clone, Debug)]
struct OwnedHostMaterial {
    name: String,
    sha256: String,
    bytes: Arc<[u8]>,
}

#[derive(Clone, Debug)]
pub struct TrustedHostEnvironment {
    data: Arc<HostEnvironmentData>,
    admission_control: OperationControl,
}
#[derive(Clone, Debug)]
struct HostEnvironmentData {
    profile: String,
    closed: bool,
    materials: Vec<Value>,
    intrinsics: Vec<IntrinsicDeclaration>,
    owned_materials: Vec<OwnedHostMaterial>,
    bindings: Vec<HostBinding>,
    bindings_admitted: bool,
}

impl TrustedHostEnvironment {
    /// Arbitrary authenticated material remains supported for the existing intrinsic profile.
    /// New binding records additionally require bounded, unambiguous JSON admission below.
    pub fn from_host_material(profile: String, closed: bool, materials: Vec<HostMaterialInput>, intrinsics: Vec<IntrinsicDeclaration>, control: &OperationControl) -> lycheedev_core::Result<Self> {
        control.check()?;
        if profile.is_empty() || profile.len() > 256 || materials.len() > HOST_MATERIAL_LIMIT || intrinsics.len() > HOST_BINDING_LIMIT {
            return Err(input_error("host material bounds"));
        }
        let mut names = std::collections::BTreeSet::new();
        let mut admitted = Vec::new();
        let mut owned_materials = Vec::new();
        let mut bytes = 0usize;
        for material in materials {
            control.check()?;
            bytes = bytes.checked_add(material.bytes.len()).ok_or_else(|| input_error("host bytes overflow"))?;
            if material.name.is_empty() || material.name.len() > 256 || !valid_sha(&material.expected_sha256) || !names.insert(material.name.clone()) || bytes > HOST_BYTE_LIMIT {
                return Err(input_error("host material identity or byte bounds"));
            }
            let actual = controlled_hash(&material.bytes, control)?;
            if actual != material.expected_sha256 { return Err(input_error("trusted host material hash mismatch")); }
            admitted.push(json!({"name": material.name, "bytes": material.bytes.len(), "sha256": actual}));
            owned_materials.push(OwnedHostMaterial { name: material.name, sha256: actual, bytes: Arc::from(material.bytes) });
        }
        let mut sites = std::collections::BTreeSet::new();
        for intrinsic in &intrinsics {
            control.check()?;
            if intrinsic.file.len() > HOST_STRING_LIMIT || intrinsic.source_sha256.len() > 64 || intrinsic.host_material.len() > 256
                || !names.contains(&intrinsic.host_material) || !sites.insert((intrinsic.file.clone(), intrinsic.start, intrinsic.end))
                || !matches!(intrinsic.name.as_str(), "Mixin" | "CreateFromMixins" | "setmetatable") || intrinsic.rule != PROFILE {
                return Err(input_error("intrinsic requires an explicit supported trusted host declaration"));
            }
        }
        control.check()?;
        Ok(Self { data:Arc::new(HostEnvironmentData { profile, closed, materials: admitted, intrinsics, owned_materials, bindings: Vec::new(), bindings_admitted: false }), admission_control: control.clone() })
    }

    pub fn bindings(&self) -> &[HostBinding] { &self.data.bindings }

    /// Consumes the environment and fixes its initial bindings exactly once.
    pub fn with_bindings(mut self, inputs: Vec<HostBindingInput>, control: &OperationControl) -> lycheedev_core::Result<Self> {
        require_same_control(&self.admission_control, control)?;
        if self.data.bindings_admitted || inputs.len() > HOST_BINDING_LIMIT {
            return Err(input_error("host binding count or repeated admission"));
        }
        let mut documents = BTreeMap::new();
        let mut json_budget = HostJsonBudget { control, nodes: 0, string_bytes: 0 };
        // Parse each referenced material once, before resolving any record. Intrinsic-only
        // byte material is intentionally not forced through the JSON contract.
        for input in &inputs {
            let (declaration, rules, _) = host_input_parts(input);
            for reference in std::iter::once(declaration).chain(rules) {
                control.check()?;
                if reference.material.is_empty() || reference.material.len() > 256 || reference.pointer.len() > 1024 {
                    return Err(input_error("host reference bounds"));
                }
                if !documents.contains_key(&reference.material) {
                    let material = self.data.owned_materials.iter().find(|item| item.name == reference.material)
                        .ok_or_else(|| input_error("host reference material is not admitted"))?;
                    let document = parse_host_json(&material.bytes, &mut json_budget)?;
                    documents.insert(reference.material.clone(), document);
                }
            }
        }
        let mut checked_documents = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        let mut pointers = std::collections::BTreeSet::new();
        let mut bindings = Vec::new();
        let mut type_catalogs=BTreeMap::<String,Arc<EnvironmentTypeCatalog>>::new();
        for input in &inputs {
            control.check()?;
            let (declaration, rule_ref, collection) = host_input_parts(input);
            let document = documents.get(&declaration.material).ok_or_else(|| input_error("host declaration document missing"))?;
            if checked_documents.insert(declaration.material.clone()) { validate_host_document(document, control)?; }
            let index = host_record_index(&declaration.pointer, collection)?;
            let record = document.get(collection).and_then(Value::as_array).and_then(|items| items.get(index))
                .ok_or_else(|| input_error("host declaration pointer does not select its record kind"))?;
            let (name, identity) = host_record_identity(record)?;
            if !names.insert(name.to_owned()) || !identities.insert(identity.to_owned())
                || !pointers.insert((declaration.material.clone(), declaration.pointer.clone()))
                || self.data.intrinsics.iter().any(|intrinsic| intrinsic.name == name) {
                return Err(input_error("ambiguous host binding name, identity, or pointer"));
            }
            let kind = match input {
                HostBindingInput::Callable { .. } | HostBindingInput::Method { .. } => {
                    let return_arity = host_return_arity(record)?;
                    let rules = match rule_ref {
                        Some(reference) => {
                            if !pointers.insert((reference.material.clone(), reference.pointer.clone())) {
                                return Err(input_error("ambiguous host rule pointer"));
                            }
                            let material = self.host_material(reference)?;
                            if document.get("rules").and_then(|rules| rules.get("sha256")).and_then(Value::as_str) != Some(material.sha256.as_str()) {
                                return Err(input_error("host declaration rule material hash mismatch"));
                            }
                            if reference.pointer != format!("/calls/{name}") {
                                return Err(input_error("host rule pointer does not select the callable name"));
                            }
                            let rules_document = documents.get(&reference.material).ok_or_else(|| input_error("host rules document missing"))?;
                            let rule = rules_document.get("calls").and_then(|calls| calls.get(name)).ok_or_else(|| input_error("host rule pointer target missing"))?;
                            validate_host_rule(rule, name, return_arity, control)?;
                            Some(HostRuleEvidence { evidence: self.host_evidence(reference, identity)?, record: rule.clone() })
                        }
                        None => None,
                    };
                    let effect=if record.get("effect").and_then(Value::as_str)==Some("no_lua_heap_write"){HostEffect::NoLuaHeapWrite}else{HostEffect::Unknown};
                    let descriptor=record.get("descriptor").cloned();
                    let is_method=matches!(input,HostBindingInput::Method{..});
                    let native_descriptor=if let Some(descriptor)=&descriptor {
                        let typed:EnvironmentRecord=serde_json::from_value(descriptor.clone()).map_err(|error|input_error(&format!("host descriptor: {error}")))?;
                        let owned=typed.owner.as_ref().filter(|owner|owner.kind=="script-object");
                        let descriptor_name=if is_method{owned.map(|owner|format!("{}.{}",owner.name,typed.name))}else{Some(typed.name.clone())};
                        if typed.kind!="function" || descriptor_name.as_deref()!=Some(name){return Err(input_error("host descriptor does not identify its callable"));}
                        if owned.is_some()&&!is_method {
                            return Err(input_error("a ScriptObject method descriptor is not an initial global binding"));
                        }
                        if !type_catalogs.contains_key(&declaration.material){
                            let mut catalog=EnvironmentTypeCatalog{identity:self.host_material(declaration)?.sha256.clone(),..Default::default()};
                            if let Some(aliases)=document.get("typeAliases") {catalog.aliases=serde_json::from_value(aliases.clone()).map_err(|error|input_error(&format!("host type aliases: {error}")))?;}
                            if let Some(nominals)=document.get("nominalObjects") {catalog.nominal_objects=serde_json::from_value(nominals.clone()).map_err(|error|input_error(&format!("host nominal objects: {error}")))?;}
                            if let Some(records)=document.get("typeDeclarations") {
                                let records:Vec<EnvironmentRecord>=serde_json::from_value(records.clone()).map_err(|error|input_error(&format!("host type declarations: {error}")))?;
                                for record in records{control.check()?;catalog.declarations.entry(record.name.clone()).or_default().push(record);}
                            }
                            type_catalogs.insert(declaration.material.clone(),Arc::new(catalog));
                        }
                        let evidence=self.host_evidence(declaration,identity)?;
                        Some(Arc::new(EnvironmentCallable{origin:EnvironmentOrigin{material:evidence.material,material_sha256:evidence.material_sha256,pointer:format!("{}/descriptor",evidence.pointer),identity:evidence.identity},declaration:typed,types:type_catalogs[&declaration.material].clone()}))
                    }else{None};
                    let receiver_type=if is_method{Some(native_descriptor.as_ref().and_then(|descriptor|descriptor.receiver_type_name()).ok_or_else(||input_error("host method receiver has no nominal declaration"))?.to_owned())}else{None};
                    HostBindingKind::Callable { return_arity, rules, effect, receiver_type, descriptor, native_descriptor }
                }
                HostBindingInput::Predicate { .. } => HostBindingKind::Predicate { argument: 1, return_arity: 1 },
                HostBindingInput::EmptyObject { .. } => HostBindingKind::EmptyObject,
            };
            bindings.push(HostBinding { data:Arc::new(HostBindingData { name: name.to_owned(), identity: identity.to_owned(), declaration: self.host_evidence(declaration, identity)?, kind }) });
        }
        control.check()?;
        let data=Arc::make_mut(&mut self.data);
        data.bindings = bindings;
        data.bindings_admitted = true;
        Ok(self)
    }

    fn host_material(&self, reference: &HostMaterialRef) -> lycheedev_core::Result<&OwnedHostMaterial> {
        self.data.owned_materials.iter().find(|material| material.name == reference.material)
            .ok_or_else(|| input_error("host material is not admitted"))
    }
    fn host_evidence(&self, reference: &HostMaterialRef, identity: &str) -> lycheedev_core::Result<EnvironmentEvidence> {
        let material = self.host_material(reference)?;
        Ok(EnvironmentEvidence { material: material.name.clone(), material_sha256: material.sha256.clone(), pointer: reference.pointer.clone(), identity: identity.to_owned() })
    }
    #[cfg(test)]
    fn identity_value(&self, control: &OperationControl) -> lycheedev_core::Result<Value> {
        require_same_control(&self.admission_control, control)?;
        let mut value = json!({"profile": self.data.profile, "closed": self.data.closed, "materials": self.data.materials, "intrinsics": self.data.intrinsics});
        // Preserve the closed intrinsic-only identity shape. Actual new semantics are
        // fully present when supplied, alongside every authenticated material hash.
        if !self.data.bindings.is_empty() {
            let mut bindings = Vec::with_capacity(self.data.bindings.len());
            for binding in &self.data.bindings {
                control.check()?;
                bindings.push(serde_json::to_value(binding).map_err(|_| input_error("host bindings serialization"))?);
            }
            value["bindings"] = Value::Array(bindings);
        }
        control.check()?;
        Ok(value)
    }
}

// The old Value identity sorted every object recursively. Retain authenticated
// host data and canonicalize one record at a time instead of retaining a second
// complete bindings/descriptor tree. Control belongs to the current writer,
// never to this immutable view shared across research requests.
struct CanonicalHostEnvironment<'a>(&'a HostEnvironmentData);
impl Serialize for CanonicalHostEnvironment<'_> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error> {
        use serde::ser::SerializeMap;
        let data=self.0;
        let mut map=serializer.serialize_map(Some(if data.bindings.is_empty(){4}else{5}))?;
        if !data.bindings.is_empty(){map.serialize_entry("bindings",&CanonicalHostRecords(&data.bindings))?;}
        map.serialize_entry("closed",&data.closed)?;
        map.serialize_entry("intrinsics",&CanonicalHostRecords(&data.intrinsics))?;
        map.serialize_entry("materials",&data.materials)?;
        map.serialize_entry("profile",&data.profile)?;
        map.end()
    }
}
struct CanonicalHostRecords<'a,T>(&'a [T]);
impl<T:Serialize> Serialize for CanonicalHostRecords<'_,T> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq=serializer.serialize_seq(Some(self.0.len()))?;
        for record in self.0 {
            let canonical=serde_json::to_value(record).map_err(serde::ser::Error::custom)?;
            seq.serialize_element(&canonical)?;
        }
        seq.end()
    }
}

fn host_input_parts(input: &HostBindingInput) -> (&HostMaterialRef, Option<&HostMaterialRef>, &'static str) {
    match input {
        HostBindingInput::Callable { declaration, rules } => (declaration, rules.as_ref(), "callables"),
        HostBindingInput::Method { declaration } => (declaration, None, "methods"),
        HostBindingInput::Predicate { declaration } => (declaration, None, "predicates"),
        HostBindingInput::EmptyObject { declaration } => (declaration, None, "initialObjects"),
    }
}

fn host_record_index(pointer: &str, collection: &str) -> lycheedev_core::Result<usize> {
    let prefix = format!("/{collection}/");
    let index = pointer.strip_prefix(&prefix).ok_or_else(|| input_error("host declaration pointer kind mismatch"))?;
    let parsed = index.parse::<usize>().map_err(|_| input_error("host declaration pointer index"))?;
    if parsed >= HOST_BINDING_LIMIT || index != parsed.to_string() {
        return Err(input_error("host declaration pointer must use a bounded canonical array index"));
    }
    Ok(parsed)
}

fn host_record_identity(record: &Value) -> lycheedev_core::Result<(&str, &str)> {
    let name = record.get("name").and_then(Value::as_str).ok_or_else(|| input_error("host record name missing"))?;
    let identity = record.get("identity").and_then(Value::as_str).ok_or_else(|| input_error("host record identity missing"))?;
    if name.is_empty() || name.len() > 256 || !name.split('.').all(|part|!part.is_empty()&&part.bytes().enumerate().all(|(index, byte)| byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())))
        || identity.is_empty() || identity.len() > 256 || identity.chars().any(char::is_control) {
        return Err(input_error("host record name or identity invalid"));
    }
    Ok((name, identity))
}

fn host_return_arity(record: &Value) -> lycheedev_core::Result<Option<usize>> {
    match record.get("returnArity") {
        None => Ok(None),
        Some(value) => value.as_u64().filter(|value| *value <= HOST_SLOT_LIMIT as u64).map(|value| Some(value as usize))
            .ok_or_else(|| input_error("host return arity must be an integer from zero through 32")),
    }
}

fn validate_host_document(document: &Value, control: &OperationControl) -> lycheedev_core::Result<()> {
    if document.get("sourceExecution") != Some(&Value::Bool(false)) || document.get("configExecution") != Some(&Value::Bool(false)) {
        return Err(input_error("host declaration material must forbid source and configuration execution"));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut identities = std::collections::BTreeSet::new();
    let mut count = 0;
    for collection in ["callables", "methods", "predicates", "initialObjects"] {
        control.check()?;
        let Some(value) = document.get(collection) else { continue; };
        let records = value.as_array().ok_or_else(|| input_error("host declaration collection must be an array"))?;
        count += records.len();
        if count > HOST_BINDING_LIMIT { return Err(input_error("host declaration record count budget")); }
        for record in records {
            control.check()?;
            let (name, identity) = host_record_identity(record)?;
            if !names.insert(name) || !identities.insert(identity) {
                return Err(input_error("ambiguous host declaration names or identities"));
            }
            if record.get("sourceExecution").is_some_and(|value| value != &Value::Bool(false))
                || record.get("configExecution").is_some_and(|value| value != &Value::Bool(false)) {
                return Err(input_error("host record may not authorize source or configuration execution"));
            }
            match collection {
                "callables" | "methods" | "predicates" => {
                    let known=record.get("effect").and_then(Value::as_str)==Some("no_lua_heap_write")&&record.get("retainsArguments")==Some(&Value::Bool(false));
                    let unknown=matches!(collection,"callables"|"methods")&&record.get("effect").and_then(Value::as_str)==Some("unknown")&&record.get("retainsArguments").is_none_or(Value::is_null);
                    if !known&&!unknown {
                        return Err(input_error("host callable effect or argument retention is unsupported"));
                    }
                    let arity = host_return_arity(record)?;
                    if matches!(collection,"callables"|"methods") && record.get("sourceExecution") != Some(&Value::Bool(false)) {
                        return Err(input_error("host callable must explicitly forbid source execution"));
                    }
                    if collection == "predicates" && (record.get("argument").and_then(Value::as_u64) != Some(1) || arity != Some(1) || name != "issecretvalue") {
                        return Err(input_error("unsupported host predicate contract"));
                    }
                }
                "initialObjects" => {
                    if !record.get("fields").and_then(Value::as_array).is_some_and(Vec::is_empty) {
                        return Err(input_error("host object requires explicit empty fields"));
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    control.check()?;
    Ok(())
}

fn validate_host_rule(rule: &Value, name: &str, arity: Option<usize>, control: &OperationControl) -> lycheedev_core::Result<()> {
    if rule.get("name").and_then(Value::as_str) != Some(name) { return Err(input_error("host rule record name mismatch")); }
    for (collection, label) in [("returns", "state"), ("arguments", "secret")] {
        control.check()?;
        let Some(value) = rule.get(collection) else { continue; };
        let slots = value.as_object().ok_or_else(|| input_error("host rule slots must be an object"))?;
        if slots.len() > HOST_SLOT_LIMIT { return Err(input_error("host rule slot count budget")); }
        for (key, slot) in slots {
            control.check()?;
            let index = key.parse::<usize>().map_err(|_| input_error("host rule slot index"))?;
            if index == 0 || index > HOST_SLOT_LIMIT || key != &index.to_string() || (collection == "returns" && arity.is_some_and(|total| index > total)) {
                return Err(input_error("host rule slot exceeds trusted arity or slot bounds"));
            }
            let state = slot.get(label).and_then(Value::as_str).ok_or_else(|| input_error("host rule risk label missing"))?;
            let valid = if collection == "returns" { matches!(state, "never" | "secret" | "possible" | "unknown") }
                else { matches!(state, "allowed" | "forbidden" | "conditional" | "unknown") };
            if !valid || slot.get("condition").is_some_and(|condition| !condition.is_string()) {
                return Err(input_error("host rule risk label or condition invalid"));
            }
        }
    }
    Ok(())
}

// The visitor rejects duplicate JSON keys rather than silently taking the last
// declaration. Node, nesting and decoded-string quotas cover all parsed materials.
struct HostJsonBudget<'a> { control: &'a OperationControl, nodes: usize, string_bytes: usize }
impl HostJsonBudget<'_> {
    fn node<E: serde::de::Error>(&mut self, depth: usize) -> std::result::Result<(), E> {
        self.control.check().map_err(E::custom)?;
        self.nodes += 1;
        if depth > HOST_JSON_DEPTH_LIMIT || self.nodes > HOST_JSON_NODE_LIMIT { return Err(E::custom("host JSON node or depth budget")); }
        Ok(())
    }
    fn string<E: serde::de::Error>(&mut self, value: &str) -> std::result::Result<(), E> {
        self.control.check().map_err(E::custom)?;
        self.string_bytes += value.len();
        if value.len() > HOST_STRING_LIMIT || self.string_bytes > HOST_BYTE_LIMIT { return Err(E::custom("host JSON string budget")); }
        Ok(())
    }
}

struct HostJsonSeed<'a, 'b> { budget: &'a mut HostJsonBudget<'b>, depth: usize }
impl<'de> serde::de::DeserializeSeed<'de> for HostJsonSeed<'_, '_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<Value, D::Error> {
        self.budget.node::<D::Error>(self.depth)?;
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for HostJsonSeed<'_, '_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str("bounded unambiguous host JSON") }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> std::result::Result<Value, E> { Ok(Value::Bool(value)) }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> std::result::Result<Value, E> { Ok(Value::Number(value.into())) }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> std::result::Result<Value, E> { Ok(Value::Number(value.into())) }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(value).map(Value::Number).ok_or_else(|| E::custom("nonfinite host JSON number"))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Value, E> {
        self.budget.string::<E>(value)?;
        Ok(Value::String(value.to_owned()))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> std::result::Result<Value, E> {
        self.budget.string::<E>(&value)?;
        Ok(Value::String(value))
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Value, E> { Ok(Value::Null) }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(HostJsonSeed { budget: self.budget, depth: self.depth + 1 })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        loop {
            self.budget.control.check().map_err(serde::de::Error::custom)?;
            let Some(key) = map.next_key::<String>()? else { break; };
            self.budget.string::<A::Error>(&key)?;
            if values.contains_key(&key) { return Err(serde::de::Error::custom("duplicate host JSON object key")); }
            let value = map.next_value_seed(HostJsonSeed { budget: self.budget, depth: self.depth + 1 })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

// Also check the control while serde scans whitespace, escaped strings, keys and
// numbers, rather than waiting until a complete JSON value reaches the visitor.
struct HostJsonReader<'a> { bytes: &'a [u8], offset: usize, control: &'a OperationControl }
impl std::io::Read for HostJsonReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.control.check().map_err(std::io::Error::other)?;
        let count = output.len().min(self.bytes.len() - self.offset).min(1024);
        output[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}

fn parse_host_json(bytes: &[u8], budget: &mut HostJsonBudget<'_>) -> lycheedev_core::Result<Value> {
    use serde::de::DeserializeSeed;
    budget.control.check()?;
    let reader = HostJsonReader { bytes, offset: 0, control: budget.control };
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let result = HostJsonSeed { budget, depth: 0 }.deserialize(&mut deserializer);
    budget.control.check()?;
    let value = result.map_err(|_| input_error("malformed, ambiguous, or over-budget host JSON material"))?;
    let end = deserializer.end();
    budget.control.check()?;
    end.map_err(|_| input_error("trailing content in host JSON material"))?;
    if !value.is_object() { return Err(input_error("host JSON material root must be an object")); }
    Ok(value)
}
