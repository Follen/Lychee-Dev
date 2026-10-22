//! Real full-batch admission and owned-query driver for semantic regressions.
//! The receipt and its analysis stay together. No second ingestion, inferred
//! contribution order, patched context API, or diagnostic fallback is used.
#![allow(dead_code)]
use emmylua_code_analysis::{
    CheckConfig, EmmyLuaAnalysis, FileFacts, FileId, LuaFunctionType, LuaMemberKey, LuaType,
    SemanticId,
};
use emmylua_parser::{LuaExpr, LuaIndexExpr, LuaSyntaxId, LuaSyntaxTree};
use lycheedev_emmy_adapter::session::{
    BindingContext, BindingCoverage, BindingDecision, BoundValue, DiagnosticRun,
    OwnedBindingSession, OwnedMemberInfo, OwnedQuery, OwnedQueryFailure, OwnedResolvedMember,
    OwnedValue, WorkspaceFactsCache,
};
use lycheedev_emmy_adapter::{ByteRange, IngestedBatch, IngestionFailure, ingest_public_batch};
use rowan::{TextRange, TextSize};
use std::{cell::RefCell, ops::Deref, path::PathBuf, sync::Arc};

pub struct OwnedQueryTest {
    analysis: EmmyLuaAnalysis,
    batch: IngestedBatch,
    workspace_facts: Arc<WorkspaceFactsCache>,
}
impl OwnedQueryTest {
    pub fn ingest(
        mut analysis: EmmyLuaAnalysis,
        files: Vec<(PathBuf, Option<Arc<str>>)>,
    ) -> Result<Self, IngestionFailure<()>> {
        let batch = ingest_public_batch(&mut analysis, files, 512 << 20, || Ok::<_, ()>(()))?;
        Ok(Self {
            analysis,
            batch,
            workspace_facts: Arc::new(WorkspaceFactsCache::new()),
        })
    }

    pub fn file_ids(&self) -> &[FileId] {
        &self.batch.file_ids
    }

    /// Test mutations retain the actual new admission receipt. An immutable
    /// model borrow prevents changing the database under an existing query.
    pub fn owned_ingest_shared<E>(
        &mut self,
        files: Vec<(PathBuf, Option<Arc<str>>)>,
        check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<FileId>, IngestionFailure<E>> {
        let batch = ingest_public_batch(&mut self.analysis, files, 512 << 20, check)?;
        let files = batch.file_ids.clone();
        self.batch = batch;
        self.workspace_facts = Arc::new(WorkspaceFactsCache::new());
        Ok(files)
    }

    pub fn owned_ingest_strings<E>(
        &mut self,
        files: Vec<(PathBuf, Option<String>)>,
        check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<FileId>, IngestionFailure<E>> {
        self.owned_ingest_shared(
            files
                .into_iter()
                .map(|(path, text)| (path, text.map(Arc::from)))
                .collect(),
            check,
        )
    }

    pub fn diagnose(
        &self,
        file: FileId,
        config: Arc<CheckConfig>,
        session: &OwnedBindingSession,
    ) -> Result<DiagnosticRun, OwnedQueryFailure<()>> {
        let mut check = || Ok::<_, ()>(());
        let mut query = OwnedQuery::new_with_contribution_order(
            &self.analysis,
            session,
            file,
            &mut check,
            Arc::clone(&self.batch.contribution_order),
        )
        .with_workspace_facts(Arc::clone(&self.workspace_facts));
        let mut run = DiagnosticRun::default();
        query.diagnose_into(config, &mut run)?;
        Ok(run)
    }
}

// Read-only public fact/AST access is convenient for choosing exact test sites.
// There is deliberately no DerefMut: mutation would invalidate batch ownership.
impl Deref for OwnedQueryTest {
    type Target = EmmyLuaAnalysis;
    fn deref(&self) -> &Self::Target {
        &self.analysis
    }
}

pub fn session(context: Arc<dyn BindingContext>) -> Arc<OwnedBindingSession> {
    OwnedBindingSession::new_checked(context, || Ok::<_, ()>(())).unwrap()
}

/// Read every original ledger field in producer order through the checked API.
pub trait SessionCoverageTest {
    fn coverage(&self) -> Vec<BindingCoverage>;
}
impl SessionCoverageTest for OwnedBindingSession {
    fn coverage(&self) -> Vec<BindingCoverage> {
        self.coverage_checked(|| Ok::<_, ()>(())).unwrap()
    }
}

pub fn byte_range(range: TextRange) -> ByteRange {
    ByteRange {
        start: range.start().into(),
        end: range.end().into(),
    }
}

/// One persistent owned query per test model. Only terminal test observations
/// expose LuaType; every member/relation consumer receives the complete carrier.
/// Interrupted or unimplemented queries fail here instead of becoming Unknown.
pub struct OwnedModel<'analysis, 'control> {
    fixture: &'analysis OwnedQueryTest,
    file: FileId,
    session: &'analysis OwnedBindingSession,
    query: RefCell<OwnedQuery<'analysis, 'control, ()>>,
}
impl<'analysis, 'control> OwnedModel<'analysis, 'control> {
    pub fn new(
        fixture: &'analysis OwnedQueryTest,
        file: FileId,
        session: &'analysis OwnedBindingSession,
        check: &'control mut dyn FnMut() -> Result<(), ()>,
    ) -> Self {
        Self {
            fixture,
            file,
            session,
            query: RefCell::new(
                OwnedQuery::new_with_contribution_order(
                    &fixture.analysis,
                    session,
                    file,
                    check,
                    Arc::clone(&fixture.batch.contribution_order),
                )
                .with_workspace_facts(Arc::clone(&fixture.workspace_facts)),
            ),
        }
    }

    pub fn model_for<'next>(
        &self,
        file: FileId,
        check: &'next mut dyn FnMut() -> Result<(), ()>,
    ) -> OwnedModel<'analysis, 'next> {
        OwnedModel::new(self.fixture, file, self.session, check)
    }

    pub fn binding_context_identity(&self) -> Option<&str> {
        Some(self.session.identity())
    }

    pub fn binding_decision(&self, syntax: LuaSyntaxId) -> BindingDecision<BoundValue> {
        self.query.borrow_mut().binding_decision(syntax).unwrap()
    }

    pub fn syntax_tree(&self) -> Option<&'analysis LuaSyntaxTree> {
        self.fixture
            .analysis
            .semantic_model(self.file)
            .syntax_tree()
    }

    pub fn file_facts(&self) -> Option<&'analysis FileFacts> {
        self.fixture.analysis.semantic_model(self.file).file_facts()
    }

    pub fn resolve_name(&self, offset: TextSize) -> Option<SemanticId> {
        self.fixture
            .analysis
            .semantic_model(self.file)
            .resolve_name(offset)
    }

    pub fn value_of_expr(&self, syntax: LuaSyntaxId) -> OwnedValue {
        self.query.borrow_mut().type_of_expr(syntax).unwrap()
    }

    pub fn type_of_expr(&self, syntax: LuaSyntaxId) -> LuaType {
        self.value_of_expr(syntax).structural_type().clone()
    }

    pub fn type_of_expr_at(&self, syntax: LuaSyntaxId, offset: TextSize) -> LuaType {
        self.query
            .borrow_mut()
            .type_of_expr_at(syntax, offset.into())
            .unwrap()
            .structural_type()
            .clone()
    }

    pub fn type_of_decl(&self, declaration: &SemanticId) -> Option<LuaType> {
        self.query
            .borrow_mut()
            .type_of_decl(declaration)
            .unwrap()
            .map(|value| value.structural_type().clone())
    }

    pub fn return_type(&self, closure: LuaSyntaxId) -> Option<LuaType> {
        self.query
            .borrow_mut()
            .return_type(closure)
            .unwrap()
            .map(|value| value.structural_type().clone())
    }

    pub fn type_of_signature(&self, closure: LuaSyntaxId) -> Option<Arc<LuaFunctionType>> {
        self.query
            .borrow_mut()
            .type_of_signature(closure)
            .unwrap()
            .map(|value| {
                let LuaType::DocFunction(function) = value.structural_type() else {
                    panic!("signature query returned a non-function carrier")
                };
                function.clone()
            })
    }

    pub fn infer_expr_list_types(
        &self,
        expressions: &[LuaExpr],
        variable_count: Option<usize>,
    ) -> Vec<(LuaType, TextRange)> {
        self.query
            .borrow_mut()
            .infer_expr_list_types(expressions, variable_count)
            .unwrap()
            .into_iter()
            .map(|(value, range)| (value.structural_type().clone(), range))
            .collect()
    }

    pub fn member_infos(&self, value: &OwnedValue) -> Vec<OwnedMemberInfo> {
        self.query.borrow_mut().member_infos(value).unwrap()
    }

    pub fn member_infos_with_key(
        &self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> Vec<OwnedMemberInfo> {
        self.query
            .borrow_mut()
            .member_infos_with_key(value, key)
            .unwrap()
    }

    pub fn member_infos_with_key_all(
        &self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> Vec<OwnedMemberInfo> {
        self.query
            .borrow_mut()
            .member_infos_with_key_all(value, key)
            .unwrap()
    }

    pub fn member_info(&self, value: &OwnedValue, key: &LuaMemberKey) -> Option<OwnedMemberInfo> {
        self.query.borrow_mut().member_info(value, key).unwrap()
    }

    pub fn member_type(&self, value: &OwnedValue, key: &LuaMemberKey) -> Option<LuaType> {
        self.query
            .borrow_mut()
            .member_type(value, key)
            .unwrap()
            .map(|value| value.structural_type().clone())
    }

    pub fn type_check(&self, source: &OwnedValue, target: &OwnedValue) -> bool {
        self.query.borrow_mut().type_check(source, target).unwrap()
    }

    pub fn resolve_member(&self, index: &LuaIndexExpr) -> Option<ResolvedMember> {
        self.query
            .borrow_mut()
            .resolve_member(index)
            .unwrap()
            .map(ResolvedMember)
    }
}

pub struct ResolvedMember(OwnedResolvedMember);
impl ResolvedMember {
    pub fn member_type(&self, model: &OwnedModel<'_, '_>) -> Option<LuaType> {
        model
            .query
            .borrow_mut()
            .resolved_member_type(&self.0)
            .unwrap()
            .map(|value| value.structural_type().clone())
    }
}
impl Deref for ResolvedMember {
    type Target = OwnedResolvedMember;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Caller-owned controls and sessions outlive the model without self-references.
// Each invocation has its own hygienic bindings and retains real query caches.
macro_rules! model {
    ($name:ident, $fixture:expr, $file:expr, $session:expr) => {
        let owned_session = $session;
        let mut owned_check = || Ok::<_, ()>(());
        let $name = $crate::owned_query::OwnedModel::new(
            &$fixture,
            $file,
            &owned_session,
            &mut owned_check,
        );
    };
}
pub(crate) use model;

macro_rules! foreign_model {
    ($name:ident, $model:expr, $file:expr) => {
        let mut owned_check = || Ok::<_, ()>(());
        let $name = $model.model_for($file, &mut owned_check);
    };
}
pub(crate) use foreign_model;
