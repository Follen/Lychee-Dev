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
}
impl std::ops::Deref for Graph {
    type Target = GraphFacts;
    fn deref(&self) -> &GraphFacts {
        &self.facts
    }
}
impl Graph {
    pub fn inputs(&self) -> &SourceInputs {
        &self.inputs
    }
    pub fn check(&self) -> lycheedev_core::Result<()> {
        self.control.check()
    }
}

#[derive(Clone, Copy)]
pub enum EvaluatorProfile {
    Native,
    LegacyV7,
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
            ingested.text_cost,
            ingested.contribution_order,
            &mut observer,
        ),
        control,
    )
}
impl PreparedSnapshot {
    /// Additional engine-owned source text, excluding parser/index allocations.
    /// The original source corpus stays shared by graph/request views.
    pub fn engine_text_cost(&self) -> lycheedev_emmy_adapter::IngestionTextCost {
        self.engine_text_cost
    }
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
    pub fn check(&self) -> lycheedev_core::Result<()> {
        self.control.check()
    }
    pub fn build(
        &self,
        name: &str,
        profile: EvaluatorProfile,
        scope: Option<&str>,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Graph> {
        require_same_control(&self.control, control)?;
        if name.len() > 256 {
            return Err(input_error("graph name byte budget exceeded"));
        }
        let ordered: Vec<_> = self.inputs.ordered.iter().map(String::as_str).collect();
        let result = public_result(
            self.graph_inner(
                name,
                &self.inputs.phase,
                &ordered,
                scope,
                matches!(profile, EvaluatorProfile::Native),
            ),
            &self.control,
        );
        control.check()?;
        result
    }
    /// A changed phase/order makes a new immutable admission, graph and provider identity.
    /// It reuses the same admitted syntax and the original operation deadline.
    pub fn build_execution(
        &self,
        name: &str,
        phase: &str,
        ordered: &[&str],
        profile: EvaluatorProfile,
        scope: Option<&str>,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Graph> {
        require_same_control(&self.control, control)?;
        if name.len() > 256 {
            return Err(input_error("graph name byte budget exceeded"));
        }
        let result = public_result(
            self.graph_inner(
                name,
                phase,
                ordered,
                scope,
                matches!(profile, EvaluatorProfile::Native),
            ),
            &self.control,
        );
        control.check()?;
        result
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
    pub fn query<T>(
        &self,
        graph: &Graph,
        selection: NativeViewSelection,
        control: &OperationControl,
        query: impl FnOnce(
            &EmmyLuaAnalysis,
            Arc<BindingSession>,
            &OperationControl,
        ) -> lycheedev_core::Result<T>,
    ) -> lycheedev_core::Result<T> {
        require_same_control(&self.control, control)?;
        graph.check()?;
        let provider = self.freeze(graph, selection, control)?;
        let session = BindingSession::new_checked(provider, || self.control.check())?;
        let result = query(&self.analysis, session, &self.control);
        self.control.check()?;
        control.check()?;
        result
    }
    pub fn serialize<T: Serialize>(
        &self,
        value: &T,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Vec<u8>> {
        require_same_control(&self.control, control)?;
        let result = serialize_json(value, &self.control, self.inputs.corpus.limits.output_bytes);
        control.check()?;
        result
    }
    /// Syntax verification remains pending until a separate trusted checker executes.
    pub fn syntax_requirement(&self) -> Value {
        json!({"state":"pending","luac":self.inputs.corpus.luac,"inputIdentity":self.inputs.identity_sha256(),
            "files":self.inputs.corpus.files.iter().map(|file|&file.identity).collect::<Vec<_>>()})
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
        || !Arc::ptr_eq(&saved.cancel_handle(), &provided.cancel_handle())
    {
        return Err(input_error(
            "single-operation native snapshots require the original admission deadline and cancellation token",
        ));
    }
    Ok(())
}
