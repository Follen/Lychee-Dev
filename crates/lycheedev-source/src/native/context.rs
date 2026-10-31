#[cfg(test)]
#[path = "expression_identity_tests.rs"]
mod expression_identity_tests;
#[path = "expression_index.rs"]
mod expression_index;
#[path = "managed_sites.rs"]
mod managed_sites;
#[path = "original_lines.rs"]
mod original_lines;
#[cfg(test)]
#[path = "retained_site_identity_tests.rs"]
mod retained_site_identity_tests;

use emmylua_code_analysis::{FileId, SemanticId};
use emmylua_parser::LuaSyntaxId;
use lycheedev_emmy_adapter::session::{
    BindingContext, BindingDecision, BindingReference, BindingSite, BoundValue, CallObservation,
    CallPermit, DerivedNumber, DerivedNumberRule, FieldPresence, FunctionRef, ObjectField,
    ObjectRef, ObjectShape, ObjectViewKey as NativeObjectViewKey,
    OwnedBindingSession as BindingSession, SelectedValue, ShapeEnumeration, SourceSpan,
};
use lycheedev_emmy_adapter::session::{
    EnvironmentCallable, EnvironmentOrigin, EnvironmentRecord, EnvironmentReturn,
    EnvironmentTypeCatalog, ObservedCallee,
};
use lycheedev_emmy_adapter::session::{ExternalSourceSpan, FieldWriteOrigin, ObjectOrigin};
use std::collections::{HashMap, HashSet};

pub const NATIVE_RULES: &str = "wow-native-binding-context-v14-argument-comparison-coverage";
pub const NATIVE_PROFILE: &str = "typed-call-v1";

// A source template and a concrete invocation are distinct query contracts.
// The default source view never chooses an arbitrary runtime occurrence.
#[derive(Clone, Copy)]
pub enum NativeViewSelection {
    SourceTemplates,
    Invocation(usize),
    Entry(usize),
    Load(usize),
    Occurrence(usize),
}

// The provider contains only immutable graph decisions and syntax identities.
// Construction may collect source identities; queries cannot reach analysis.
pub struct FrozenContext {
    control: OperationControl,
    identity: String,
    decisions: HashMap<BindingSite, BindingDecision<BoundValue>>,
    calls: HashMap<BindingSite, BindingDecision<CallPermit>>,
    call_observations: HashMap<BindingSite, BindingDecision<CallObservation>>,
    managed_sites: Arc<managed_sites::ManagedSites>,
    refs: HashMap<String, Vec<BindingReference>>,
    shapes: HashMap<NativeObjectViewKey, ObjectShape>,
}
impl BindingContext for FrozenContext {
    fn identity(&self) -> &str {
        &self.identity
    }
    fn object_shape(&self, key: &NativeObjectViewKey) -> BindingDecision<ObjectShape> {
        if let Err(error) = self.control.check() {
            return BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), "control")),
                reason: error.code,
            };
        }
        self.shapes
            .get(key)
            .cloned()
            .map(BindingDecision::Bound)
            .unwrap_or_else(|| BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), format!("{key:?}"))),
                reason: "object_observation_not_in_context".into(),
            })
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        if let Err(error) = self.control.check() {
            return BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), "control")),
                reason: error.code,
            };
        }
        self.decisions.get(&site).cloned().unwrap_or_else(|| {
            if self.managed_sites.contains_read(&site) {
                BindingDecision::Unavailable {
                    proof: digest(&(self.identity.as_str(), format!("{site:?}"))),
                    reason: "managed_use_missing_from_bounded_graph".into(),
                }
            } else {
                BindingDecision::NotManaged
            }
        })
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        if let Err(error) = self.control.check() {
            return BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), "control")),
                reason: error.code,
            };
        }
        self.calls.get(&site).cloned().unwrap_or_else(|| {
            if self.managed_sites.contains_call(&site) {
                BindingDecision::Unavailable {
                    proof: digest(&(self.identity.as_str(), format!("{site:?}"))),
                    reason: "managed_call_missing_from_bounded_graph".into(),
                }
            } else {
                BindingDecision::NotManaged
            }
        })
    }
    fn call_observation(&self, site: BindingSite) -> BindingDecision<CallObservation> {
        if let Err(error) = self.control.check() {
            return BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), "control")),
                reason: error.code,
            };
        }
        self.call_observations
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn references(&self, identity: &str) -> BindingDecision<Vec<BindingReference>> {
        if let Err(error) = self.control.check() {
            return BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), "control")),
                reason: error.code,
            };
        }
        if let Some(refs) = self.refs.get(identity) {
            BindingDecision::Bound(refs.clone())
        } else {
            BindingDecision::Unavailable {
                proof: digest(&(self.identity.as_str(), identity)),
                reason: "value_identity_not_in_context".into(),
            }
        }
    }
}
pub struct PreparedSnapshot {
    syntax: SourceSyntaxIndex,
    inputs: Arc<SourceInputs>,
    control: OperationControl,
    owner: Arc<()>,
    analysis: EmmyLuaAnalysis,
    engine_text_cost: lycheedev_emmy_adapter::IngestionTextCost,
    contribution_order: Arc<[FileId]>,
    workspace_facts: Arc<lycheedev_emmy_adapter::session::WorkspaceFactsCache>,
    config: Emmyrc,
    files: BTreeMap<String, FileId>,
    // Exact original-byte lookups only: one packed sorted range index per file.
    // File IDs and B-tree nodes are not repeated for every expression.
    expressions: BTreeMap<FileId, expression_index::ExpressionIndex<LuaSyntaxId>>,
    // Paths are owned by SourceInputs. These exact original-byte keys retain
    // only the admitted file ordinal and range, never another Site::id string.
    closures: BTreeMap<SyntaxSite, LuaSyntaxId>,
    entry_catalog: Vec<EntryCandidate>,
    original_lines: BTreeMap<FileId, original_lines::OriginalLines>,
    parameters: BTreeMap<SyntaxSite, SemanticId>,
    managed_sites: Arc<managed_sites::ManagedSites>,
}
impl PreparedSnapshot {
    fn prepare_inner(
        inputs: Arc<SourceInputs>,
        control: &OperationControl,
        analysis: EmmyLuaAnalysis,
        engine_text_cost: lycheedev_emmy_adapter::IngestionTextCost,
        contribution_order: Arc<[FileId]>,
        observer: &mut impl FnMut(&'static str),
    ) -> Result<Self> {
        control.check().map_err(|e| e.to_string())?;
        let config = inputs.corpus.config.clone();
        let mut input = Self {
            syntax: SourceSyntaxIndex::default(),
            inputs,
            control: control.clone(),
            owner: Arc::new(()),
            analysis,
            engine_text_cost,
            contribution_order,
            workspace_facts: Arc::new(
                lycheedev_emmy_adapter::session::WorkspaceFactsCache::default(),
            ),
            config,
            files: BTreeMap::new(),
            expressions: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry_catalog: vec![],
            original_lines: BTreeMap::new(),
            parameters: BTreeMap::new(),
            managed_sites: Arc::new(managed_sites::ManagedSites::default()),
        };
        let mut syntax_counts = SourceSyntaxCounts::for_files(
            input.inputs.corpus.files.len(),
            input.inputs.corpus.limits.files,
        )?;
        let mut line_index_remaining = 64usize * 1024 * 1024;
        let mut nodes = 0;
        let mut ids = HashSet::new();
        let mut uris = HashSet::new();
        let mut managed_sites = managed_sites::ManagedSites::default();
        for (file_index, source) in input.inputs.corpus.files.iter().enumerate() {
            control.check().map_err(|e| e.to_string())?;
            let name = source.path.as_str();
            let uri = file_path_to_uri(&input.inputs.corpus.root.join(name)).ok_or("source URI")?;
            let file_id = input
                .analysis
                .get_file_id(&uri)
                .ok_or("source file identity")?;
            let actual_uri = input
                .analysis
                .get_uri(file_id)
                .ok_or("source reverse URI identity")?;
            if !ids.insert(file_id) || !uris.insert(actual_uri.to_string()) || actual_uri != uri {
                return Err("admitted paths alias the same Emmy FileId or URI".into());
            }
            if input.analysis.db.get_file_text(file_id) != Some(source.analysis.as_ref()) {
                return Err("Emmy analyzed bytes differ from admitted analysis bytes".into());
            }
            input.files.insert(name.into(), file_id);
            if let Some(lines) = original_lines::OriginalLines::build(
                &source.original,
                line_index_remaining,
                || control.check().map_err(|error| error.to_string()),
            )? {
                line_index_remaining = line_index_remaining
                    .checked_sub(lines.retained_bytes())
                    .ok_or("original line index accounting overflow")?;
                input.original_lines.insert(file_id, lines);
            }
            let model = input.analysis.semantic_model(file_id);
            if !model.parse_errors().unwrap_or_default().is_empty() {
                return Err(format!("native source parse error: {name}"));
            }
            let root = model
                .syntax_tree()
                .ok_or("source syntax tree")?
                .get_red_root();
            let mut expressions = Vec::new();
            let mut managed_file = managed_sites::ManagedFileBuilder::default();
            for node in root.descendants() {
                control.check().map_err(|e| e.to_string())?;
                nodes += 1;
                if nodes > input.inputs.corpus.limits.ast_nodes {
                    return Err("native identity AST budget exceeded".into());
                }
                syntax_counts[file_index].observe(&node);
                if let Some(expr) = LuaExpr::cast(node.clone()) {
                    let syntax = expr.get_syntax_id();
                    let range = input
                        .inputs
                        .original_range(name, expr.get_range())
                        .expect("admitted parser range");
                    expressions.push((range.start().into(), range.end().into(), syntax));
                    managed_file.observe(syntax);
                }
                if let Some(closure) = LuaClosureExpr::cast(node) {
                    input.closures.insert(
                        SyntaxSite::at(
                            file_index,
                            input
                                .inputs
                                .original_range(name, closure.get_range())
                                .expect("admitted parser range"),
                        ),
                        closure.get_syntax_id(),
                    );
                    input.entry_catalog.push(
                        input
                            .entry_candidate(name, closure.get_syntax_id())
                            .map_err(|error| error.to_string())?,
                    );
                }
            }
            input.expressions.insert(
                file_id,
                expression_index::ExpressionIndex::build(expressions, || {
                    control.check().map_err(|error| error.to_string())
                })?,
            );
            // The original expression-compaction control check precedes this
            // per-file sort; the next original declaration check follows it.
            // No whole-workspace hash table or second membership copy remains.
            managed_sites.insert(file_id, managed_file.finish());
            for declaration in &model.file_facts().ok_or("source facts")?.decls {
                control.check().map_err(|e| e.to_string())?;
                if matches!(declaration.kind, emmylua_code_analysis::DeclKind::Param) {
                    input.parameters.insert(
                        SyntaxSite::at(
                            file_index,
                            input
                                .inputs
                                .original_range(name, declaration.name_range)
                                .expect("admitted parser range"),
                        ),
                        declaration.id.clone(),
                    );
                }
            }
        }
        // Identity validation is complete. These uniqueness sets are not query
        // state and must not overlap the following syntax-index allocations.
        drop(ids);
        drop(uris);
        input.managed_sites = Arc::new(managed_sites);
        observer("source_identity_indexed");
        control.check().map_err(|e| e.to_string())?;
        input.syntax = input.build_syntax_index(&syntax_counts)?;
        drop(syntax_counts);
        observer("source_syntax_indexed");
        control.check().map_err(|e| e.to_string())?;
        Ok(input)
    }
    pub fn binding_site(&self, site: &Site) -> Result<BindingSite> {
        self.control.check().map_err(|e| e.to_string())?;
        let file_id = *self.files.get(&site.file).ok_or("site file")?;
        let syntax = *self
            .expressions
            .get(&file_id)
            .and_then(|expressions| expressions.get(site.start, site.end))
            .ok_or_else(|| format!("expression identity unavailable: {}", site.id()))?;
        Ok(BindingSite { file_id, syntax })
    }
    fn expression_binding_site(&self, site: &Site) -> Option<BindingSite> {
        let file_id = *self.files.get(&site.file)?;
        let syntax = *self.expressions.get(&file_id)?.get(site.start, site.end)?;
        Some(BindingSite { file_id, syntax })
    }
    fn closure_syntax(&self, site: &Site) -> Option<LuaSyntaxId> {
        self.closures
            .get(&SyntaxSite::from_site(&self.inputs, site)?)
            .copied()
    }
    fn parameter_identity(&self, site: &Site) -> Option<&SemanticId> {
        self.parameters
            .get(&SyntaxSite::from_site(&self.inputs, site)?)
    }
    fn graph_inner(
        &self,
        name: &str,
        phase: &str,
        ordered: &[&str],
        scope: Option<&str>,
    ) -> Result<Graph> {
        self.control.check().map_err(|e| e.to_string())?;
        let owned = self
            .inputs
            .execution(phase, ordered, &self.control)
            .map_err(|e| e.to_string())?;
        self.graph_owned(name, scope, owned)
    }
    fn graph_owned(
        &self,
        name: &str,
        scope: Option<&str>,
        owned: Arc<SourceInputs>,
    ) -> Result<Graph> {
        let ordered = &owned.ordered;
        let phase = &owned.phase;
        let mut inputs = json!({"admissionSHA256":owned.identity_sha256(),"adapter":fixed_graph_limits(),
            "orderedScriptLoadOccurrences":ordered.iter().enumerate().map(|(index,file)|json!({"index":index,"file":file})).collect::<Vec<_>>(),
            "executionPhase":phase,"evaluator":"native-invocations"});
        if owned.load_plan.is_some() {
            inputs["adapter"]["explicitExecution"] = json!({"rulesVersion":ENTRY_RULES,"views":["entry"],"loadPlan":"known_occurrences_or_unknown_file_set","unknownOrder":"knowledge_boundary_no_execution","entryArguments":"unspecified_unknown_or_exact_list_nil_padding"});
        }
        if !owned.host.data.bindings.is_empty() {
            inputs["adapter"]["hostRuntime"] =
                host_runtime_profile_for(owned.entry.is_some(), owned.corpus.limits.research);
        }
        if let Some(scope) = scope {
            if scope.len() > 256 {
                return Err("validation scope byte budget exceeded".into());
            }
            inputs["validationScope"] = json!(scope);
        }
        let inputs = GraphSnapshotEvidence {
            admission: owned.identity.clone(),
            common: owned.graph_common.clone(),
            execution: evidence_fields(inputs),
        };
        build_graph(self, name, inputs, owned)
    }
    fn freeze_inner(
        &self,
        graph: &Graph,
        selection: NativeViewSelection,
    ) -> Result<Arc<FrozenContext>> {
        self.control.check().map_err(|e| e.to_string())?;
        graph.control.check().map_err(|e| e.to_string())?;
        if !Arc::ptr_eq(&self.owner, &graph.owner) {
            return Err("graph belongs to another prepared snapshot".into());
        }
        let facts = graph
            .native_invocations
            .as_ref()
            .ok_or("native invocation facts missing")?;
        if !facts.control.complete {
            return Err(
                "prefix graph requires boundary review; complete native provider unavailable"
                    .into(),
            );
        }
        if let NativeViewSelection::Occurrence(id) = selection
            && id >= facts.control.occurrences.len()
        {
            return Err("control occurrence missing".into());
        }
        let selected_frame = match selection {
            NativeViewSelection::Invocation(id) => Some(
                facts
                    .frames
                    .get(id)
                    .filter(|frame| frame.call.is_some())
                    .ok_or("invocation selector is not a concrete frame")?,
            ),
            NativeViewSelection::Entry(id) => Some(
                facts
                    .frames
                    .get(id)
                    .filter(|frame| {
                        matches!(
                            frame.origin,
                            Some(NativeInvocationOrigin::ExplicitEntry { .. })
                        )
                    })
                    .ok_or("entry selector is not an explicit entry frame")?,
            ),
            _ => None,
        };
        let accepts = |context: &str| -> bool {
            if let NativeViewSelection::Occurrence(id) = selection {
                return facts.control.context_occurrences.get(context) == Some(&id);
            }
            match facts.query_views.get(context) {
                Some(NativeQueryView::Invocation { frame } | NativeQueryView::Entry { frame }) => {
                    selected_frame.is_some_and(|selected| selected.id == *frame)
                }
                Some(NativeQueryView::Static { function }) => {
                    selected_frame.is_none_or(|selected| selected.function != *function)
                }
                Some(NativeQueryView::Load { occurrence }) => match selection {
                    NativeViewSelection::Load(selected) => selected == *occurrence,
                    _ => true,
                },
                Some(NativeQueryView::Xml { .. }) => true,
                None => false,
            }
        };
        let identity = match selection {
            NativeViewSelection::SourceTemplates => graph.snapshot_key.clone(),
            NativeViewSelection::Invocation(frame) => {
                digest(&(graph.snapshot_key.as_str(), "invocation", frame))
            }
            NativeViewSelection::Entry(frame) => {
                digest(&(graph.snapshot_key.as_str(), "entry", frame))
            }
            NativeViewSelection::Load(occurrence) => {
                digest(&(graph.snapshot_key.as_str(), "load", occurrence))
            }
            NativeViewSelection::Occurrence(occurrence) => digest(&(
                graph.snapshot_key.as_str(),
                "control-occurrence",
                occurrence,
            )),
        };
        let mut provider = FrozenContext {
            control: self.control.clone(),
            identity,
            decisions: HashMap::new(),
            calls: HashMap::new(),
            call_observations: HashMap::new(),
            managed_sites: self.managed_sites.clone(),
            refs: HashMap::new(),
            shapes: HashMap::new(),
        };
        for shape in &graph.object_views {
            self.control.check().map_err(|e| e.to_string())?;
            let key = Self::native_view_key(&shape.key);
            let mut fields = Vec::new();
            for field in &shape.fields {
                let proof = digest(&(shape.proof_id.as_str(), field));
                let value = self.native_value_decision(
                    graph,
                    &field.selected_value,
                    field.nested_object_view,
                    &proof,
                )?;
                let span = |site: &Site| -> Result<FieldWriteOrigin> {
                    Ok(match graph.inputs.origin_at(site) {
                        ValueOrigin::Xml { xml } => FieldWriteOrigin::Xml(ExternalSourceSpan {
                            path: xml.path,
                            content_sha256: xml.sha256,
                            range: owned_byte_range(TextRange::new(
                                site.start.into(),
                                site.end.into(),
                            )),
                        }),
                        _ => FieldWriteOrigin::Lua(SourceSpan {
                            file_id: *self.files.get(&site.file).ok_or("write source file")?,
                            range: owned_byte_range(
                                self.inputs
                                    .analysis_range(
                                        &site.file,
                                        TextRange::new(site.start.into(), site.end.into()),
                                    )
                                    .map_err(|e| e.to_string())?,
                            ),
                        }),
                    })
                };
                let destination_write = span(
                    &graph
                        .writes
                        .get(field.destination_write)
                        .ok_or("destination write")?
                        .site,
                )?;
                let source_write = field
                    .source_write
                    .map(|id| span(&graph.writes.get(id).ok_or("source write")?.site))
                    .transpose()?;
                let copy_origin = if let Some(id) = field.xml_copy_origin {
                    let copy = graph
                        .xml
                        .as_ref()
                        .and_then(|facts| facts.copies.get(id))
                        .ok_or("XML copy edge")?;
                    Some(span(&xml_site(&copy.origin))?)
                } else {
                    field
                        .copy_origin
                        .map(|id| -> Result<FieldWriteOrigin> {
                            let copy = graph.copies.get(id).ok_or("copy edge")?;
                            span(&graph.calls.get(copy.call).ok_or("copy call")?.site)
                        })
                        .transpose()?
                };
                fields.push(ObjectField {
                    key: field.key.clone(),
                    presence: match field.presence {
                        SlotPresence::Present => FieldPresence::Present,
                        SlotPresence::Absent => FieldPresence::Absent,
                        SlotPresence::Unavailable => FieldPresence::Unavailable,
                    },
                    value,
                    destination_write,
                    source_write,
                    copy_origin,
                });
            }
            let enumeration = match &shape.enumeration {
                ObjectEnumeration::Complete => ShapeEnumeration::Complete,
                ObjectEnumeration::Unavailable(reason) => {
                    ShapeEnumeration::Unavailable(reason.clone())
                }
            };
            provider.shapes.insert(
                key.clone(),
                ObjectShape {
                    key,
                    fields,
                    enumeration,
                    proof: shape.proof_id.clone(),
                },
            );
        }
        for fact in &graph.uses {
            self.control.check().map_err(|e| e.to_string())?;
            if !accepts(&fact.context) {
                continue;
            }
            let Some(site) = self.expression_binding_site(&fact.site) else {
                continue;
            };
            // Only Name/Index use sites are value replacement points. Literals
            // and calls retain native inference unless a profile intrinsic
            // explicitly projects the graph-selected fresh object.
            if !provider.managed_sites.contains_read(&site) {
                continue;
            }
            let decision = self.native_decision(graph, fact)?;
            if provider.decisions.contains_key(&site) {
                return Err(format!(
                    "ambiguous execution occurrence: {}",
                    fact.site.id()
                ));
            }
            if let BindingDecision::Bound(value) = &decision {
                provider
                    .refs
                    .entry(value.identity.clone())
                    .or_default()
                    .push(BindingReference {
                        site,
                        proof: fact.proof_id.clone(),
                        execution_context: fact.context.clone(),
                    });
            }
            provider.decisions.insert(site, decision);
        }
        for call in &graph.calls {
            self.control.check().map_err(|e| e.to_string())?;
            if !accepts(&call.context) {
                continue;
            }
            let site = self.binding_site(&call.site)?;
            let callee = &graph.uses[call.callee];
            let result = &graph.uses[call.result_fact.ok_or("call result fact")?];
            let function = match callee.decision {
                Decision::Bound(id) => match graph.values[id].kind {
                    ValueKind::Function(function) => Some(&graph.functions[function]),
                    _ => None,
                },
                _ => None,
            };
            let host_descriptor = match callee.decision {
                Decision::Bound(id) => match graph.values[id].kind {
                    ValueKind::HostCallable(binding) => self.inputs.host.data.bindings[binding]
                        .native_descriptor()
                        .cloned(),
                    _ => None,
                },
                _ => None,
            };
            let multiple_return_paths = facts
                .frames
                .iter()
                .find(|frame| frame.call == Some(call.id))
                .is_some_and(|frame| frame.return_paths.len() > 1);
            let return_slots = graph
                .host_runtime
                .as_ref()
                .and_then(|host| host.calls.iter().find(|slots| slots.call == call.id));
            let open_prefix = return_slots
                .and_then(|slots| slots.return_open_tail.as_ref())
                .map(|tail| tail.first_unknown_slot.saturating_sub(1));
            let uncertain_returns = return_slots.is_some_and(|slots| {
                slots
                    .return_arities
                    .as_ref()
                    .is_some_and(|arities| arities.len() > 1)
            });
            let return_observation_reason = if uncertain_returns {
                Some("host_sequence_requires_cardinality_projection")
            } else if multiple_return_paths {
                Some("conditional_return_path_candidates_require_path_projection")
            } else {
                None
            };
            let selected_value_check = if call.conditional_branch.is_none()
                && let Decision::Bound(id) = callee.decision
            {
                let known_shape = match &graph.values[id].kind {
                    ValueKind::Primitive(_) => true,
                    ValueKind::Object(_) => callee
                        .object_view
                        .and_then(|view| graph.object_views.get(view))
                        .is_some_and(|view| {
                            matches!(view.enumeration, ObjectEnumeration::Complete)
                        }),
                    _ => false,
                };
                known_shape
                    && matches!(
                        provider.decisions.get(&self.binding_site(&callee.site)?),
                        Some(BindingDecision::Bound(_))
                    )
            } else {
                false
            };
            let decision = if host_descriptor.is_some() && call.conditional_branch.is_none() {
                // A declaration permits signature checks independently of heap
                // effects and return cardinality. Neither grants future lookup.
                BindingDecision::Bound(CallPermit {
                    callee: self.binding_site(&callee.site)?,
                    proof: result.proof_id.clone(),
                    capability_profile: "authenticated_environment_signature_v1".into(),
                })
            } else if selected_value_check {
                // The selected value can be checked for callability without
                // inventing a function invocation or any return observation.
                BindingDecision::Bound(CallPermit {
                    callee: self.binding_site(&callee.site)?,
                    proof: callee.proof_id.clone(),
                    capability_profile: NATIVE_PROFILE.into(),
                })
            } else if call.conditional_branch.is_some() {
                BindingDecision::Unavailable {
                    proof: result.proof_id.clone(),
                    reason: "conditional_execution_unknown".into(),
                }
            } else if let Some(function) = function {
                if function.intrinsic.is_some() && matches!(result.decision, Decision::Bound(_)) {
                    provider
                        .decisions
                        .insert(site, self.native_decision(graph, result)?);
                    BindingDecision::Bound(CallPermit {
                        callee: self.binding_site(&callee.site)?,
                        proof: result.proof_id.clone(),
                        capability_profile: "fixture_intrinsic_value".into(),
                    })
                } else {
                    // The exact selected function identity admits static
                    // callable/parameter checks even in a deferred body. An
                    // execution frame and its returns are separate evidence.
                    BindingDecision::Bound(CallPermit {
                        callee: self.binding_site(&callee.site)?,
                        proof: result.proof_id.clone(),
                        capability_profile: NATIVE_PROFILE.into(),
                    })
                }
            } else {
                match &callee.decision {
                    Decision::KnownAbsent(_) => BindingDecision::KnownAbsent {
                        proof: callee.proof_id.clone(),
                    },
                    Decision::Unavailable(reason) => BindingDecision::Unavailable {
                        proof: callee.proof_id.clone(),
                        reason: reason.clone(),
                    },
                    _ => BindingDecision::Unavailable {
                        proof: callee.proof_id.clone(),
                        reason: "selected_value_not_function".into(),
                    },
                }
            };
            if matches!(&decision, BindingDecision::Bound(_))
                && let Some(frame) = facts
                    .frames
                    .iter()
                    .find(|frame| frame.call == Some(call.id))
                && let Some(closure_id) = frame.closure
            {
                let closure = facts
                    .closures
                    .get(closure_id)
                    .ok_or("call observation closure instance")?;
                let selected = self.native_decision(graph, callee)?;
                let BindingDecision::Bound(BoundValue {
                    identity: selected_callee_identity,
                    value: SelectedValue::Function(function),
                    ..
                }) = selected
                else {
                    return Err("call observation selected callee is not its function".into());
                };
                if closure.function != frame.function
                    || callee.decision != Decision::Bound(closure.value)
                    || self.function_id_for_value(graph, &callee.decision) != Some(frame.function)
                {
                    return Err("call observation definition identity mismatch".into());
                }
                if frame.return_facts.len() > INVOCATION_RETURN_LIMIT {
                    return Err("call observation return slot budget exceeded".into());
                }
                let mut returns = frame
                    .return_facts
                    .iter()
                    .map(|id| {
                        self.native_decision(
                            graph,
                            graph.uses.get(*id).ok_or("call return fact identity")?,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(prefix) = open_prefix {
                    returns.truncate(prefix);
                }
                let observation = CallObservation {
                    callee: ObservedCallee::Source(function),
                    selected_callee_identity,
                    closure_identity: format!("{}:closure:{}", graph.snapshot_key, closure.id),
                    invocation_identity: format!("{}:frame:{}", graph.snapshot_key, frame.id),
                    returns,
                    open_tail: open_prefix.is_some(),
                    proof: digest(&(
                        graph.snapshot_key.as_str(),
                        call.id,
                        frame.id,
                        &frame.return_facts,
                        &result.proof_id,
                    )),
                };
                if provider
                    .call_observations
                    .insert(
                        site,
                        match return_observation_reason {
                            Some(reason) => BindingDecision::Unavailable {
                                proof: result.proof_id.clone(),
                                reason: reason.into(),
                            },
                            None => BindingDecision::Bound(observation),
                        },
                    )
                    .is_some()
                {
                    return Err("ambiguous call return observation".into());
                }
            }
            if matches!(&decision, BindingDecision::Bound(_))
                && let Some(descriptor) = host_descriptor
            {
                let Decision::Bound(callee_value) = callee.decision else {
                    return Err("host callee identity missing".into());
                };
                let slots = graph
                    .host_runtime
                    .as_ref()
                    .and_then(|host| host.calls.iter().find(|slots| slots.call == call.id))
                    .ok_or("host call slot observation missing")?;
                let mut returns = slots
                    .returns
                    .iter()
                    .map(|slot| self.native_decision(graph, &graph.uses[slot.use_fact]))
                    .collect::<Result<Vec<_>>>()?;
                if let Some(prefix) = open_prefix {
                    returns.truncate(prefix);
                }
                let observation = CallObservation {
                    callee: ObservedCallee::Environment(descriptor),
                    selected_callee_identity: format!(
                        "{}:value:{callee_value}",
                        graph.snapshot_key
                    ),
                    closure_identity: String::new(),
                    invocation_identity: format!("{}:call:{}", graph.snapshot_key, call.id),
                    returns,
                    open_tail: open_prefix.is_some(),
                    proof: slots.proof_id.clone(),
                };
                if provider
                    .call_observations
                    .insert(
                        site,
                        match return_observation_reason {
                            Some(reason) => BindingDecision::Unavailable {
                                proof: slots.proof_id.clone(),
                                reason: reason.into(),
                            },
                            None => BindingDecision::Bound(observation),
                        },
                    )
                    .is_some()
                {
                    return Err("ambiguous host call observation".into());
                }
            }
            if function.is_some()
                && matches!(&decision, BindingDecision::Bound(permit)
                    if permit.capability_profile != "fixture_intrinsic_value")
                && !provider.call_observations.contains_key(&site)
            {
                // A selected source function can be checked in a deferred body
                // without executing it. Absence here is an observed-knowledge
                // boundary, not an unmanaged provider with a fixed signature.
                provider.call_observations.insert(
                    site,
                    BindingDecision::Unavailable {
                        proof: result.proof_id.clone(),
                        reason: "native_call_return_observation_missing".into(),
                    },
                );
            }
            if provider.calls.insert(site, decision).is_some() {
                return Err("ambiguous call execution occurrence".into());
            }
        }
        // The engine validates every object view against its binding session.
        // A selected invocation/load view has its own cache identity, so rebase
        // shape keys and nested references together without changing evidence.
        if provider.identity != graph.snapshot_key {
            let rebase = |decision: &mut BindingDecision<BoundValue>| {
                if let BindingDecision::Bound(BoundValue {
                    value: SelectedValue::Object(object),
                    ..
                }) = decision
                {
                    object.view.context = provider.identity.clone();
                    object.epoch = digest(&(
                        provider.identity.as_str(),
                        object.view.observation,
                        object.view.object_id,
                    ));
                }
            };
            for decision in provider.decisions.values_mut() {
                rebase(decision);
            }
            for observation in provider.call_observations.values_mut() {
                if let BindingDecision::Bound(observation) = observation {
                    for value in &mut observation.returns {
                        rebase(value);
                    }
                }
            }
            let mut shapes = HashMap::new();
            for (_, mut shape) in provider.shapes.drain() {
                shape.key.context = provider.identity.clone();
                for field in &mut shape.fields {
                    rebase(&mut field.value);
                }
                shapes.insert(shape.key.clone(), shape);
            }
            provider.shapes = shapes;
        }
        self.control.check().map_err(|e| e.to_string())?;
        Ok(Arc::new(provider))
    }
    fn function_id_for_value(&self, graph: &Graph, decision: &Decision) -> Option<usize> {
        match decision {
            Decision::Bound(value) => match graph.values.get(*value)?.kind {
                ValueKind::Function(function) => Some(function),
                _ => None,
            },
            _ => None,
        }
    }
    fn native_view_key(key: &ObjectViewKey) -> NativeObjectViewKey {
        NativeObjectViewKey {
            context: key.context.clone(),
            observation: key.observation,
            object_id: key.object_id,
        }
    }
    fn native_decision(
        &self,
        graph: &Graph,
        fact: &UseFact,
    ) -> Result<BindingDecision<BoundValue>> {
        self.native_value_decision(graph, &fact.decision, fact.object_view, &fact.proof_id)
    }
    fn native_derived_number(
        &self,
        graph: &Graph,
        derived: &NativeDerivedNumber,
    ) -> Result<DerivedNumber> {
        self.control.check().map_err(|error| error.to_string())?;
        let control = &graph
            .native_invocations
            .as_ref()
            .ok_or("native control evidence missing")?
            .control;
        if derived.rule != "lua51-numeric-for-iteration-value-v1"
            || !derived.number.is_finite()
            || derived.iteration >= LOOP_ITERATION_LIMIT
            || !(2..=3).contains(&derived.header_facts.len())
            || graph
                .values
                .get(derived.value)
                .is_none_or(|value| value.origin.source() != Some(&derived.origin))
            || control
                .numbers
                .get(&derived.value)
                .is_none_or(|number| number.to_bits() != derived.number.to_bits())
            || derived.number_bits != format!("{:016x}", derived.number.to_bits())
            || control
                .occurrences
                .get(derived.occurrence)
                .is_none_or(|occurrence| {
                    !occurrence.loops.iter().any(|item| {
                        item.site == derived.statement && item.iteration == derived.iteration
                    })
                })
        {
            return Err("derived numeric source or occurrence evidence mismatch".into());
        }
        let expected = digest(&(
            graph.snapshot_key.as_str(),
            derived.value,
            &derived.number_bits,
            &derived.origin,
            &derived.statement,
            &derived.header_facts,
            derived.iteration,
            derived.predecessor,
            derived.occurrence,
            &derived.rule,
        ));
        if derived.proof_id != expected {
            return Err("derived numeric rule proof mismatch".into());
        }
        if let Some(previous) = derived.predecessor {
            if control
                .derived_numbers
                .get(&previous)
                .is_none_or(|previous| {
                    previous.statement != derived.statement
                        || previous.origin != derived.origin
                        || previous.iteration + 1 != derived.iteration
                })
            {
                return Err("derived numeric predecessor evidence mismatch".into());
            }
        } else if derived.iteration != 0 {
            return Err("derived numeric predecessor missing".into());
        }
        let span = |site: &Site| -> Result<SourceSpan> {
            Ok(SourceSpan {
                file_id: *self
                    .files
                    .get(&site.file)
                    .ok_or("numeric source file missing")?,
                range: owned_byte_range(
                    self.inputs
                        .analysis_range(
                            &site.file,
                            TextRange::new(site.start.into(), site.end.into()),
                        )
                        .map_err(|error| error.to_string())?,
                ),
            })
        };
        let statement = span(&derived.statement)?;
        let statement = BindingSite {
            file_id: statement.file_id,
            syntax: LuaSyntaxId::new(
                emmylua_parser::LuaSyntaxKind::ForStat.into(),
                TextRange::new(statement.range.start.into(), statement.range.end.into()),
            ),
        };
        let header = derived
            .header_facts
            .iter()
            .map(|id| {
                self.binding_site(
                    &graph
                        .uses
                        .get(*id)
                        .ok_or("numeric header fact missing")?
                        .site,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(DerivedNumber {
            value: derived.number,
            origin: span(&derived.origin)?,
            occurrence_identity: format!(
                "{}:occurrence:{}",
                graph.snapshot_key, derived.occurrence
            ),
            rule: DerivedNumberRule::NumericForIteration {
                statement,
                header,
                iteration: derived.iteration,
                predecessor: derived
                    .predecessor
                    .map(|value| format!("{}:value:{value}", graph.snapshot_key)),
            },
            proof: derived.proof_id.clone(),
        })
    }
    fn native_value_decision(
        &self,
        graph: &Graph,
        decision: &Decision,
        object_view: Option<usize>,
        proof: &str,
    ) -> Result<BindingDecision<BoundValue>> {
        Ok(match decision {
            Decision::KnownAbsent(_) => BindingDecision::KnownAbsent {
                proof: proof.to_owned(),
            },
            Decision::Unavailable(reason) => BindingDecision::Unavailable {
                proof: proof.to_owned(),
                reason: reason.clone(),
            },
            Decision::NotManaged => BindingDecision::NotManaged,
            Decision::Bound(id) => {
                let value = &graph.values[*id];
                let environment_value = match &value.kind {
                    ValueKind::HostCallable(binding) => self.inputs.host.data.bindings[*binding]
                        .native_descriptor()
                        .map(|descriptor| SelectedValue::EnvironmentCallable(descriptor.clone())),
                    ValueKind::HostReturn {
                        binding,
                        call,
                        slot,
                    } => self.inputs.host.data.bindings[*binding]
                        .native_descriptor()
                        .map(|descriptor| {
                            SelectedValue::EnvironmentReturn(EnvironmentReturn {
                                callable: descriptor.clone(),
                                slot: *slot,
                                invocation_identity: format!("{}:call:{call}", graph.snapshot_key),
                            })
                        }),
                    _ => None,
                };
                if let Some(value) = environment_value {
                    return Ok(BindingDecision::Bound(BoundValue {
                        identity: format!("{}:value:{id}", graph.snapshot_key),
                        proof: proof.to_owned(),
                        value,
                    }));
                }
                if let ValueKind::Object(object) = &value.kind {
                    let view = graph
                        .object_views
                        .get(object_view.ok_or("object use lacks frozen observation")?)
                        .ok_or("object view missing")?;
                    if view.key.context != graph.snapshot_key || view.key.object_id != *object {
                        return Err("object use observation identity mismatch".into());
                    }
                    let origin = match &value.origin {
                        ValueOrigin::Source(origin) => {
                            ObjectOrigin::Lua(self.binding_site(origin)?)
                        }
                        ValueOrigin::Xml { xml } => ObjectOrigin::Xml(ExternalSourceSpan {
                            path: xml.path.clone(),
                            content_sha256: xml.sha256.clone(),
                            range: lycheedev_emmy_adapter::ByteRange {
                                start: xml.bytes.start as u32,
                                end: xml.bytes.end as u32,
                            },
                        }),
                        _ => {
                            return Ok(BindingDecision::Unavailable {
                                proof: proof.to_owned(),
                                reason: "object_origin_unavailable".into(),
                            });
                        }
                    };
                    return Ok(BindingDecision::Bound(BoundValue {
                        identity: format!("{}:value:{id}", graph.snapshot_key),
                        proof: proof.to_owned(),
                        value: SelectedValue::Object(ObjectRef {
                            identity: format!("{}:object:{object}", graph.snapshot_key),
                            epoch: digest(&view.key),
                            origin,
                            view: Self::native_view_key(&view.key),
                        }),
                    }));
                }
                let Some(origin) = value.origin.source() else {
                    return Ok(BindingDecision::Unavailable {
                        proof: proof.to_owned(),
                        reason: if matches!(value.origin, ValueOrigin::Entry { .. }) {
                            "entry_argument_has_no_native_source_descriptor"
                        } else {
                            "environment_value_has_no_native_source_descriptor"
                        }
                        .into(),
                    });
                };
                let selected = match &value.kind {
                    ValueKind::HostCallable(_) | ValueKind::HostReturn { .. } => {
                        return Ok(BindingDecision::Unavailable {
                            proof: proof.to_owned(),
                            reason: "host_value_type_unavailable".into(),
                        });
                    }
                    ValueKind::Function(function) => {
                        let function = &graph.functions[*function];
                        SelectedValue::Function(FunctionRef {
                            file_id: self.files[&function.closure.file],
                            closure: self
                                .closure_syntax(&function.closure)
                                .ok_or("closure identity")?,
                            declaration: None,
                        })
                    }
                    ValueKind::Object(_) => {
                        unreachable!("object values were projected with their own origin")
                    }
                    ValueKind::Primitive(kind)
                        if kind == "number"
                            && graph.native_invocations.as_ref().is_some_and(|facts| {
                                facts.control.derived_numbers.contains_key(id)
                            }) =>
                    {
                        let derived = graph
                            .native_invocations
                            .as_ref()
                            .and_then(|facts| facts.control.derived_numbers.get(id))
                            .ok_or("derived numeric evidence missing")?;
                        SelectedValue::DerivedNumber(self.native_derived_number(graph, derived)?)
                    }
                    ValueKind::Primitive(_) | ValueKind::CallResult(_) => {
                        SelectedValue::Expression(self.binding_site(origin)?)
                    }
                    ValueKind::NativeParameter(site) => SelectedValue::Parameter(
                        self.parameter_identity(site)
                            .ok_or("formal parameter identity")?
                            .clone(),
                    ),
                };
                BindingDecision::Bound(BoundValue {
                    identity: format!("{}:value:{id}", graph.snapshot_key),
                    proof: proof.to_owned(),
                    value: selected,
                })
            }
        })
    }
}

fn fixed_graph_limits() -> Value {
    json!({"rulesVersion":NATIVE_RULES,"capabilityProfile":NATIVE_PROFILE,"dependencyLockSHA256":hash(DEPENDENCY_LOCK),
                "objectObservationLimits":{"objects":OBJECT_LIMIT,"views":OBJECT_VIEW_LIMIT,"slots":OBJECT_SLOT_LIMIT,"fieldsPerObject":OBJECT_FIELD_LIMIT,"depth":OBJECT_DEPTH_LIMIT,"mutations":OBJECT_MUTATION_LIMIT,"sharedConstructionSteps":10000},
                "nativeObjectLimits":{"projectionDepth":64,"projectionFieldSteps":4096,"structureDepth":32,"structureSteps":4096},
                "invocationLimits":{"callDepth":INVOCATION_DEPTH_LIMIT,"frames":INVOCATION_FRAME_LIMIT,"closures":INVOCATION_CLOSURE_LIMIT,"capturesPerClosure":INVOCATION_CAPTURE_LIMIT,"captureEdges":INVOCATION_CAPTURE_EDGE_LIMIT,"parameters":INVOCATION_PARAMETER_LIMIT,"returnArity":INVOCATION_RETURN_LIMIT,"reachableNodes":INVOCATION_FOOTPRINT_LIMIT,"retainedEscapeRoots":INVOCATION_ESCAPE_ROOT_LIMIT,"escapeEvents":INVOCATION_FOOTPRINT_LIMIT,"callObservations":INVOCATION_FRAME_LIMIT,"callObservationReturnSlots":INVOCATION_RETURN_LIMIT,"uncertainEffects":INVOCATION_UNCERTAIN_EFFECT_LIMIT,"checkpoints":INVOCATION_CHECKPOINT_LIMIT,"restores":INVOCATION_CHECKPOINT_LIMIT,"checkpointSlots":INVOCATION_CHECKPOINT_SLOT_LIMIT,"expressionDepth":64,"blockDepth":64,"sharedConstructionSteps":10000},
                "controlLimits":{"iterationsPerLoop":LOOP_ITERATION_LIMIT,"totalIterations":LOOP_TOTAL_LIMIT,"activePaths":CONTROL_ACTIVE_LIMIT,"terminalPaths":CONTROL_TERMINAL_LIMIT,"nodes":CONTROL_NODE_LIMIT,"edges":CONTROL_EDGE_LIMIT},
                "controlProfile":"structured-native-occurrence-scope-activation-v1","derivedNumberProfile":"lua51-numeric-for-original-span-rule-v1","queryPolicy":query_policy(),"queryViewProfile":"source-template-load-invocation-or-exact-control-occurrence-v2","cellProfile":"lexical-frame-activation-cells-with-reference-captures-v2","unknownEffectsProfile":"complete-call-footprints-with-idempotent-object-versions-v3","callReturnProfile":"native-source-observation-slots-and-return-path-candidates-v2"})
}

fn owned_byte_range(range: TextRange) -> lycheedev_emmy_adapter::ByteRange {
    lycheedev_emmy_adapter::ByteRange {
        start: range.start().into(),
        end: range.end().into(),
    }
}

#[cfg(test)]
mod call_return_evidence_tests {
    use super::*;
    use lycheedev_emmy_adapter::session::{DiagnosticRun, OwnedQuery};

    #[test]
    fn deferred_selected_calls_keep_declared_types_without_claiming_observed_arity() {
        for declaration in ["", "---@return number\n"] {
            let text = format!(
                "---@param n number\n{declaration}local function f(n) mystery(); return 1 end\nlocal function one(value) end\nlocal function caller() return one(f(1)) end\nlocal function badcaller() return f('bad',2) end\n"
            );
            let control = OperationControl::default();
            let host = TrustedHostEnvironment::from_host_material(
                "deferred-call-return".into(),
                false,
                vec![],
                vec![],
                &control,
            )
            .unwrap();
            let config = crate::engine_config::addon_diagnostics();
            let inputs = SourceInputs::admit(SourceInputSpec {
                files: vec![SourceFileInput {
                    path: "main.lua".into(), content_sha256: hash(text.as_bytes()),
                    analysis_utf8: text.clone().into(), original_utf8: text.clone(), map: SourceMap::Identity,
                }],
                workspace_root: std::env::current_dir().unwrap().join("deferred-call-return"),
                source_identity: json!({"test":"deferred-call-return","declaration":declaration}),
                phase: "after_load".into(), ordered_occurrences: vec!["main.lua".into()],
                config: config.clone(), host,
                luac: LuacIdentity { version:"5.1.5".into(), platform:"windows-amd64".into(),
                    executable_sha256:"a".repeat(64), executable_bytes:1 },
                limits: SourceLimits::research(),
            }, &control).unwrap();
            let prepared = prepare(inputs, &control).unwrap();
            let graph = prepared
                .build("deferred-call-return", None, &control)
                .unwrap();
            let call = graph
                .calls
                .iter()
                .find(|call| call.site.text(prepared.inputs()).unwrap() == "f(1)")
                .unwrap();
            assert!(matches!(
                call.invocation_coverage,
                CallCoverage::Unavailable
            ));
            assert!(
                !graph
                    .native_invocations
                    .as_ref()
                    .unwrap()
                    .frames
                    .iter()
                    .any(|frame| frame.call == Some(call.id))
            );
            let provider = prepared
                .freeze(&graph, NativeViewSelection::SourceTemplates, &control)
                .unwrap();
            let site = prepared.binding_site(&call.site).unwrap();
            assert!(matches!(provider.call(site), BindingDecision::Bound(_)));
            assert!(
                matches!(provider.call_observation(site), BindingDecision::Unavailable { ref reason, .. }
                if reason == "native_call_return_observation_missing")
            );
            let session = BindingSession::new_checked(provider, || Ok::<_, ()>(())).unwrap();
            let mut check = || Ok::<_, ()>(());
            let mut query = OwnedQuery::new_with_contribution_order(
                &prepared.analysis,
                &session,
                site.file_id,
                &mut check,
                prepared.contribution_order.clone(),
            );
            let value = query.type_of_expr(site.syntax).unwrap();
            assert_eq!(
                value.structural_type(),
                if declaration.is_empty() {
                    &emmylua_code_analysis::LuaType::Unknown
                } else {
                    &emmylua_code_analysis::LuaType::Number
                }
            );
            let mut run = DiagnosticRun::default();
            query
                .diagnose_into(
                    Arc::new(emmylua_code_analysis::CheckConfig::new(&config)),
                    &mut run,
                )
                .unwrap();
            let outer = graph
                .calls
                .iter()
                .find(|call| call.site.text(prepared.inputs()).unwrap() == "one(f(1))")
                .unwrap();
            let outer = prepared.binding_site(&outer.site).unwrap();
            let bad_call = graph
                .calls
                .iter()
                .find(|call| call.site.text(prepared.inputs()).unwrap() == "f('bad',2)")
                .unwrap();
            let bad_site = prepared.binding_site(&bad_call.site).unwrap();
            let rows = session.coverage_checked(|| Ok::<_, ()>(())).unwrap();
            assert!(rows.iter().any(|row| row.site == site
                && row.operation == "argument_count_check"
                && row.state == "native_checked"));
            assert!(rows.iter().any(|row| row.site == outer
                && row.operation == "argument_count_check"
                && row.state == "unavailable"));
            assert!(!rows.iter().any(|row| row.site == outer
                && row.operation == "argument_count_check"
                && row.state == "native_checked"));
            for operation in ["callable_check", "argument_count_check", "argument_check"] {
                assert!(rows.iter().any(|row| row.site == bad_site
                    && row.operation == operation
                    && row.state == "native_checked"));
            }
            for code in [
                emmylua_code_analysis::DiagnosticCode::ParamTypeMismatch,
                emmylua_code_analysis::DiagnosticCode::RedundantParameter,
            ] {
                assert!(run.diagnostics.iter().any(|row| row.code == code));
            }
        }
    }
}
