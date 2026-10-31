// Lifecycle entry points retain the caller's original absolute deadline.
#[derive(Serialize)]
#[serde(transparent)]
pub struct Graph {
    facts: GraphFacts,
    #[serde(skip)]
    inputs: Arc<SourceInputs>,
    #[serde(skip)]
    control: OperationControl,
    #[serde(skip)]
    owner: Arc<()>,
    // Populated only after Builder has moved its completed facts into this
    // immutable Graph. Graph has no Clone, mutable dereference or facts setter;
    // every constructor starts empty. Shared JSON never outlives its consumers.
    #[serde(skip)]
    research_control: std::sync::OnceLock<Arc<Value>>,
}
impl std::ops::Deref for Graph {
    type Target = GraphFacts;
    fn deref(&self) -> &GraphFacts {
        &self.facts
    }
}
impl Graph {
    fn research_control(&self) -> Arc<Value> {
        Arc::clone(self.research_control.get_or_init(|| {
            Arc::new(json!(self.facts.native_invocations.as_ref().map(|facts| &facts.control)))
        }))
    }
    pub fn inputs(&self) -> &SourceInputs {
        &self.inputs
    }
    pub fn check(&self) -> lycheedev_core::Result<()> {
        self.control.check()
    }
}

/// Text conversion shares the caller's original cancellation/deadline checks.
/// The owned-process hard deadline still bounds any individual synchronous stage.
/// No executable/config loader or implicit standard library is used here.
pub fn prepare(
    inputs: Arc<SourceInputs>,
    control: &OperationControl,
) -> lycheedev_core::Result<PreparedSnapshot> {
    prepare_observed(inputs, control, |_| {})
}
/// Observe completed preparation stages without changing admission or deadlines.
pub fn prepare_observed(
    inputs: Arc<SourceInputs>,
    control: &OperationControl,
    mut observer: impl FnMut(&'static str),
) -> lycheedev_core::Result<PreparedSnapshot> {
    require_same_control(&inputs.admission_control, control)?;
    control.check()?;
    let mut analysis = EmmyLuaAnalysis::new();
    analysis.update_config(Arc::new(inputs.corpus.config.clone()));
    analysis.add_main_workspace(inputs.corpus.root.clone());
    let mut files = Vec::with_capacity(inputs.corpus.files.len());
    for file in &inputs.corpus.files {
        control.check()?;
        files.push((
            inputs.corpus.root.join(&file.path),
            Some(file.analysis.clone()),
        ));
    }
    // Only this local analysis is mutated. Cancellation propagates its original
    // ToolkitError and drops all incomplete parser/index state before returning.
    // The public batch API retains its own text allocation. Admission covers
    // all input bytes before copying; no subset, per-file rebuild, new deadline
    // or second analysis graph is used. Parsing/indexing remain synchronous.
    let ingested = lycheedev_emmy_adapter::ingest_public_batch(
        &mut analysis,
        files,
        inputs.corpus.limits.input_bytes,
        || control.check(),
    )
    .map_err(|failure| match failure {
        lycheedev_emmy_adapter::IngestionFailure::Interrupted(error) => error,
        lycheedev_emmy_adapter::IngestionFailure::InvalidInput(message) => input_error(message),
        lycheedev_emmy_adapter::IngestionFailure::Allocation(error) => {
            ToolkitError::new("source.native_budget", error.to_string(), 3)
        }
    })?;
    debug_assert!(ingested.text_cost.retained_bytes <= inputs.corpus.limits.input_bytes);
    observer("emmy_indexed");
    control.check()?;
    public_result(
        PreparedSnapshot::prepare_inner(
            inputs,
            control,
            analysis,
            ingested.contribution_order,
            &mut observer,
        ),
        control,
    )
}
impl PreparedSnapshot {
    pub fn inputs(&self) -> &SourceInputs {
        &self.inputs
    }
    pub fn operation_control(&self) -> &OperationControl {
        &self.control
    }
    /// Low-level adapter access. Raw resolvers are not an authoritative managed-value fallback.
    pub fn analysis(&self) -> &EmmyLuaAnalysis {
        &self.analysis
    }
    pub fn files(&self) -> &BTreeMap<String, FileId> {
        &self.files
    }
    pub fn config(&self) -> &Emmyrc {
        &self.config
    }
    pub fn contribution_order(&self) -> Arc<[FileId]> {
        Arc::clone(&self.contribution_order)
    }
    pub fn workspace_facts(&self) -> Arc<lycheedev_emmy_adapter::session::WorkspaceFactsCache> {
        Arc::clone(&self.workspace_facts)
    }
    pub fn freeze(
        &self,
        graph: &Graph,
        selection: NativeViewSelection,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Arc<FrozenContext>> {
        require_same_control(&self.control, control)?;
        graph.check()?;
        public_result(self.freeze_inner(graph, selection), &self.control)
    }
    pub fn source_site(&self, site: BindingSite) -> lycheedev_core::Result<Site> {
        self.control.check()?;
        let file = self
            .files
            .iter()
            .find(|(_, id)| **id == site.file_id)
            .map(|(name, _)| name.as_str())
            .ok_or_else(|| input_error("binding site file is outside prepared inputs"))?;
        Ok(Site::at(
            file,
            self.inputs.original_range(file, site.syntax.get_range())?,
        ))
    }
}

struct ControlledWriter<'a> {
    control: &'a OperationControl,
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for ControlledWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.control.check().map_err(std::io::Error::other)?;
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|total| total > self.limit)
        {
            return Err(std::io::Error::other(
                "native serialized output byte budget exceeded",
            ));
        }
        for chunk in bytes.chunks(16384) {
            self.control.check().map_err(std::io::Error::other)?;
            self.bytes.extend_from_slice(chunk);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.control.check().map_err(std::io::Error::other)
    }
}
pub fn serialize_json<T: Serialize>(
    value: &T,
    control: &OperationControl,
    limit: usize,
) -> lycheedev_core::Result<Vec<u8>> {
    control.check()?;
    if limit == 0 || limit > 32 << 20 {
        return Err(input_error("serialization limit exceeds admitted ceiling"));
    }
    let mut writer = ControlledWriter {
        control,
        bytes: Vec::new(),
        limit,
    };
    let result = serde_json::to_writer(&mut writer, value);
    control.check()?;
    result.map_err(|error| ToolkitError::new("source.native_output", error.to_string(), 5))?;
    Ok(writer.bytes)
}

fn require_same_control(
    saved: &OperationControl,
    provided: &OperationControl,
) -> lycheedev_core::Result<()> {
    saved.check()?;
    provided.check()?;
    if saved.deadline() != provided.deadline()
        || !saved.same_scope(provided)
    {
        return Err(input_error(
            "single-operation native snapshots require the original admission deadline and cancellation token",
        ));
    }
    Ok(())
}

// Hash-only identity consumers must not retain a serialized copy of the XML
// tree. Keep the same writes, byte ceiling and error mapping as serialize_json.
struct ControlledHashWriter<'a> {
    control: &'a OperationControl,
    hasher: Sha256,
    bytes: usize,
    limit: usize,
}
impl Write for ControlledHashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.control.check().map_err(std::io::Error::other)?;
        let total = self.bytes.checked_add(bytes.len()).filter(|total| *total <= self.limit)
            .ok_or_else(|| std::io::Error::other("native serialized output byte budget exceeded"))?;
        for chunk in bytes.chunks(16384) {
            self.control.check().map_err(std::io::Error::other)?;
            self.hasher.update(chunk);
        }
        self.bytes = total;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.control.check().map_err(std::io::Error::other)
    }
}
fn serialize_json_hash<T: Serialize>(value: &T, control: &OperationControl, limit: usize) -> lycheedev_core::Result<(String, usize)> {
    control.check()?;
    if limit == 0 || limit > 32 << 20 {
        return Err(input_error("serialization limit exceeds admitted ceiling"));
    }
    let mut writer = ControlledHashWriter { control, hasher: Sha256::new(), bytes: 0, limit };
    let result = serde_json::to_writer(&mut writer, value);
    control.check()?;
    result.map_err(|error| ToolkitError::new("source.native_output", error.to_string(), 5))?;
    let digest = format!("{:x}", writer.hasher.finalize());
    control.check()?;
    Ok((digest, writer.bytes))
}
