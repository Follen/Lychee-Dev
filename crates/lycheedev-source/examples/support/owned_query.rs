// Probe-only adaptation of the public owned query/session. Every semantic value
// stays OwnedValue across query calls; structural views are only for display.
use emmylua_code_analysis::{
    CheckConfig as ProbeCheckConfig, FileId as ProbeFileId, LuaMemberKey as ProbeMemberKey,
    LuaType as ProbeLuaType, SemanticModel as ProbeSemanticModel,
};
use emmylua_parser::{
    LuaExpr as ProbeExpr, LuaSyntaxId as ProbeSyntaxId, LuaSyntaxTree as ProbeSyntaxTree,
};
use lycheedev_core::{ToolkitError as ProbeError, control::OperationControl as ProbeControl};
use lycheedev_emmy_adapter::session::{
    BindingContext as ProbeContext, BindingCoverage as ProbeCoverage,
    BindingDecision as ProbeDecision, BindingReference as ProbeReference,
    BoundValue as ProbeBoundValue, Diagnostic as ProbeDiagnostic,
    DiagnosticRun as ProbeDiagnosticRun, OwnedBindingSession as ProbeSession,
    OwnedMemberInfo as ProbeMemberInfo, OwnedQuery as ProbeOwnedQuery,
    OwnedQueryFailure as ProbeFailure, OwnedValue,
};

fn probe_failure(error: ProbeFailure<ProbeError>) -> ProbeError {
    match error {
        ProbeFailure::Control(error) => error,
        ProbeFailure::Continue(continuation) => ProbeError::new(
            "source.owned_query_incomplete",
            format!("owned query continuation: {continuation:?}"),
            3,
        ),
    }
}
fn probe_session(
    provider: std::sync::Arc<dyn ProbeContext>,
    control: &ProbeControl,
) -> std::result::Result<std::sync::Arc<ProbeSession>, String> {
    ProbeSession::new_checked(provider, || control.check()).map_err(|error| error.to_string())
}
fn probe_coverage(
    session: &ProbeSession,
    control: &ProbeControl,
) -> std::result::Result<Vec<ProbeCoverage>, String> {
    session
        .coverage_checked(|| control.check())
        .map_err(|error| error.to_string())
}
fn probe_references(
    session: &ProbeSession,
    identity: &str,
    control: &ProbeControl,
) -> std::result::Result<ProbeDecision<Vec<ProbeReference>>, String> {
    session
        .references_checked(identity, || control.check())
        .map_err(|error| error.to_string())
}
fn probe_query<'a, 'c>(
    prepared: &'a lycheedev_source::native::PreparedSnapshot,
    session: &'a ProbeSession,
    file: ProbeFileId,
    check: &'c mut dyn FnMut() -> std::result::Result<(), ProbeError>,
) -> ProbeModel<'a, 'c> {
    ProbeModel {
        query: ProbeOwnedQuery::new_with_contribution_order(
            prepared.analysis(),
            session,
            file,
            check,
            prepared.contribution_order(),
        )
        .with_workspace_facts(prepared.workspace_facts()),
        raw: prepared.analysis().semantic_model(file),
        session,
    }
}
struct ProbeModel<'a, 'c> {
    query: ProbeOwnedQuery<'a, 'c, ProbeError>,
    raw: ProbeSemanticModel<'a>,
    session: &'a ProbeSession,
}
impl<'a, 'c> ProbeModel<'a, 'c> {
    fn raw_model(&self) -> &ProbeSemanticModel<'a> {
        &self.raw
    }
    fn syntax_tree(&self) -> Option<&'a ProbeSyntaxTree> {
        self.raw.syntax_tree()
    }
    fn binding_context_identity(&self) -> &str {
        self.session.identity()
    }
    fn type_of_expr(&mut self, syntax: ProbeSyntaxId) -> std::result::Result<OwnedValue, String> {
        self.query
            .type_of_expr(syntax)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn callable_candidates_for_expr(
        &mut self,
        expr: &ProbeExpr,
    ) -> std::result::Result<Vec<OwnedValue>, String> {
        self.query
            .callable_candidates_for_expr(expr)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn type_of_signature_in_file(
        &mut self,
        file: ProbeFileId,
        syntax: ProbeSyntaxId,
    ) -> std::result::Result<Option<OwnedValue>, String> {
        self.query
            .type_of_signature_in_file(file, syntax)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn binding_decision(
        &mut self,
        syntax: ProbeSyntaxId,
    ) -> std::result::Result<ProbeDecision<ProbeBoundValue>, String> {
        self.query
            .binding_decision(syntax)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn member_infos(
        &mut self,
        ty: &OwnedValue,
    ) -> std::result::Result<Vec<ProbeMemberInfo>, String> {
        self.query
            .member_infos(ty)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn member_info(
        &mut self,
        ty: &OwnedValue,
        key: &ProbeMemberKey,
    ) -> std::result::Result<Option<ProbeMemberInfo>, String> {
        self.query
            .member_info(ty, key)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn member_infos_with_key(
        &mut self,
        ty: &OwnedValue,
        key: &ProbeMemberKey,
    ) -> std::result::Result<Vec<ProbeMemberInfo>, String> {
        self.query
            .member_infos_with_key(ty, key)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn member_infos_with_key_all(
        &mut self,
        ty: &OwnedValue,
        key: &ProbeMemberKey,
    ) -> std::result::Result<Vec<ProbeMemberInfo>, String> {
        self.query
            .member_infos_with_key_all(ty, key)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn member_type(
        &mut self,
        ty: &OwnedValue,
        key: &ProbeMemberKey,
    ) -> std::result::Result<Option<OwnedValue>, String> {
        self.query
            .member_type(ty, key)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
    fn type_check(
        &mut self,
        actual: &OwnedValue,
        expected: &OwnedValue,
    ) -> std::result::Result<bool, String> {
        self.query
            .type_check(actual, expected)
            .map_err(probe_failure)
            .map_err(|error| error.to_string())
    }
}
fn probe_diagnostics(
    prepared: &lycheedev_source::native::PreparedSnapshot,
    session: &ProbeSession,
    file: ProbeFileId,
) -> std::result::Result<Vec<ProbeDiagnostic>, String> {
    // The old diagnose entry created a fresh model; keep that independent cache
    // while retaining exactly the same session ledger and absolute deadline.
    let mut check = || prepared.operation_control().check();
    let mut model = probe_query(prepared, session, file, &mut check);
    let mut run = ProbeDiagnosticRun::default();
    model
        .query
        .diagnose_into(
            std::sync::Arc::new(ProbeCheckConfig::new(prepared.config())),
            &mut run,
        )
        .map_err(probe_failure)
        .map_err(|error| error.to_string())?;
    if !run.complete {
        return Err("owned diagnostic registry did not complete".into());
    }
    Ok(run.diagnostics)
}
fn probe_function_meta(value: &OwnedValue) -> &emmylua_code_analysis::LuaFunctionType {
    match value.structural_type() {
        ProbeLuaType::DocFunction(function) => function,
        _ => unreachable!("owned callable candidate must be a function"),
    }
}
