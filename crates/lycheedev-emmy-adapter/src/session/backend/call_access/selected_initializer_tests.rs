//! Public ingestion -> actual owned query/43-checker regressions for initializer
//! provenance. Expected types and diagnostics are independent source contracts.
use crate::session::{
    BindingContext, BindingCoverage, BindingDecision, BindingSite, BoundValue, Diagnostic,
    DiagnosticRun, FunctionRef, OwnedBindingSession, OwnedQuery, OwnedQueryFailure, SelectedValue,
};
use crate::{IngestedBatch, ingest_public_batch};
use emmylua_code_analysis::{CheckConfig, DiagnosticCode, EmmyLuaAnalysis, Emmyrc, LuaType};
use emmylua_parser::{LuaAstNode, LuaExpr};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
struct Context(HashMap<BindingSite, BindingDecision<BoundValue>>);
impl BindingContext for Context {
    fn identity(&self) -> &str {
        "selected-initializer-callable"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.0
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
}
struct Fixture {
    analysis: EmmyLuaAnalysis,
    batch: IngestedBatch,
}
impl Fixture {
    fn new(sources: &[(&str, &str)]) -> Self {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir()
            .unwrap()
            .join("selected-initializer-input");
        analysis.add_main_workspace(root.clone());
        let batch = ingest_public_batch(
            &mut analysis,
            sources
                .iter()
                .map(|(name, text)| (root.join(name), Some(Arc::from(*text))))
                .collect(),
            32 << 20,
            || Ok::<_, ()>(()),
        )
        .unwrap();
        Self { analysis, batch }
    }
    fn expr(&self, file: usize, text: &str) -> LuaExpr {
        self.analysis
            .semantic_model(self.batch.file_ids[file])
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaExpr::cast)
            .find(|expr| expr.syntax().text().to_string() == text)
            .unwrap_or_else(|| panic!("missing expression {text}"))
    }
    fn site(&self, file: usize, text: &str) -> BindingSite {
        BindingSite {
            file_id: self.batch.file_ids[file],
            syntax: self.expr(file, text).get_syntax_id(),
        }
    }
    fn function(&self, file: usize, text: &str) -> BindingDecision<BoundValue> {
        let site = self.site(file, text);
        BindingDecision::Bound(BoundValue {
            identity: "actual-function".into(),
            proof: "initializer-proof".into(),
            value: SelectedValue::Function(FunctionRef {
                file_id: site.file_id,
                closure: site.syntax,
                declaration: None,
            }),
        })
    }
    fn run<R>(
        &self,
        file: usize,
        context: Context,
        f: impl FnOnce(&mut OwnedQuery<'_, '_, ()>) -> R,
    ) -> (R, Vec<BindingCoverage>) {
        let session =
            OwnedBindingSession::new_checked(Arc::new(context), || Ok::<_, ()>(())).unwrap();
        let mut check = || Ok::<_, ()>(());
        let mut query = OwnedQuery::new_with_contribution_order(
            &self.analysis,
            &session,
            self.batch.file_ids[file],
            &mut check,
            Arc::clone(&self.batch.contribution_order),
        );
        let result = f(&mut query);
        drop(query);
        (
            result,
            session.coverage_checked(|| Ok::<_, ()>(())).unwrap(),
        )
    }
    fn diagnose(
        &self,
        context: Context,
        codes: &[DiagnosticCode],
    ) -> (DiagnosticRun, Vec<BindingCoverage>) {
        self.diagnose_file(0, context, codes)
    }
    fn diagnose_file(
        &self,
        file: usize,
        context: Context,
        codes: &[DiagnosticCode],
    ) -> (DiagnosticRun, Vec<BindingCoverage>) {
        self.run(file, context, |query| {
            let mut config = Emmyrc::default();
            config.diagnostics.enables.extend(codes.iter().copied());
            let mut run = DiagnosticRun::default();
            query
                .diagnose_into(Arc::new(CheckConfig::new(&config)), &mut run)
                .unwrap();
            assert!(run.complete);
            run
        })
    }
    fn native_diagnostics(&self, file: usize, code: DiagnosticCode) -> Vec<lsp_types::Diagnostic> {
        let mut config = Emmyrc::default();
        config.diagnostics.enables.push(code);
        config.diagnostics.disable.extend(
            DiagnosticCode::all()
                .iter()
                .copied()
                .filter(|candidate| *candidate != code),
        );
        self.analysis
            .diagnose_file_with_config(
                self.batch.file_ids[file],
                Arc::new(CheckConfig::new(&config)),
            )
            .unwrap()
    }
    fn lsp_range(&self, file: usize, range: rowan::TextRange) -> lsp_types::Range {
        let file = self.batch.file_ids[file];
        let index = self.analysis.db.line_index(file).unwrap();
        let text = self.analysis.db.get_file_text(file).unwrap();
        let position = |offset| {
            let (line, character) = index.get_line_col(offset, text).unwrap();
            lsp_types::Position::new(line as u32, character as u32)
        };
        lsp_types::Range::new(position(range.start()), position(range.end()))
    }
}
fn assert_native_diagnostic(owned: &Diagnostic, native: &lsp_types::Diagnostic) {
    assert_eq!(
        native.code,
        Some(lsp_types::NumberOrString::String(
            owned.code.get_name().into()
        ))
    );
    assert_eq!(native.message, owned.message);
    assert_eq!(native.severity, Some(owned.severity));
    assert_eq!(native.tags, owned.tags);
    assert_eq!(native.data, owned.data);
    assert_eq!(native.source.as_deref(), Some("EmmyLua"));
    assert_eq!(native.related_information, None);
    assert_eq!(native.code_description, None);
}
fn diagnostics(run: &DiagnosticRun, code: DiagnosticCode) -> Vec<&Diagnostic> {
    run.diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
        .collect()
}
fn context(site: BindingSite, value: BindingDecision<BoundValue>) -> Context {
    Context(HashMap::from([(site, value)]))
}
const OLD: &str = "function(old) return old end";
const NEW: &str = "function(value) return value end";
const FUNCTIONS: &str = "---@param old number\n---@return number\nlocal f = function(old) return old end\n---@param value string\n---@return string\nlocal replacement = function(value) return value end\nlocal result = f(1)\n";

// The fixed VM rejects an incompatible fixed argument, but an unknown argument
// may still use the declared return. Compare both query and diagnostic results
// with the unmodified engine on a direct call to the selected signature. This
// keeps initializer provenance separate from the overload matching contract.
fn selected_call_matches_direct_native(
    fixture: &Fixture,
    native: &Fixture,
    file: usize,
    argument: &str,
    expected: LuaType,
    mismatches: usize,
) {
    let origin = fixture.site(0, OLD);
    let call = format!("f({argument})");
    let native_model = native.analysis.semantic_model(native.batch.file_ids[file]);
    let native_value = native_model.type_of_expr(native.site(file, &call).syntax);
    assert_eq!(native_value, expected, "direct native call: {call}");
    if argument == "unresolved_argument" {
        assert_eq!(
            native_model.type_of_expr(native.site(file, argument).syntax),
            LuaType::Unknown
        );
    }
    let (_, rows) = fixture.run(
        file,
        context(origin, fixture.function(file, NEW)),
        |query| {
            let callee = fixture.expr(file, "f");
            let candidates = query.callable_candidates_for_expr(&callee).unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].function_parameters().unwrap(),
                [Some(crate::session::OwnedValue::plain(LuaType::String))]
            );
            assert_eq!(
                candidates[0].function_return().unwrap().structural_type(),
                &LuaType::String
            );
            let value = query
                .type_of_expr(fixture.site(file, &call).syntax)
                .unwrap();
            assert_eq!(value.structural_type(), &expected, "selected call: {call}");
            assert_eq!(
                value,
                crate::session::OwnedValue::plain(native_value.clone())
            );
            assert_eq!(
                query
                    .type_of_expr(fixture.site(file, &call).syntax)
                    .unwrap(),
                value
            );
        },
    );
    let expected_rows = [BindingCoverage {
        site: origin,
        operation: "resolve",
        state: "bound",
        proof: "initializer-proof".into(),
        reason: None,
        parameter: None,
        parameter_index: None,
        argument: None,
    }];
    assert_eq!(rows, expected_rows);
    let (run, rows) = fixture.diagnose_file(
        file,
        context(origin, fixture.function(file, NEW)),
        &[DiagnosticCode::ParamTypeMismatch],
    );
    let owned_errors = diagnostics(&run, DiagnosticCode::ParamTypeMismatch);
    let native_errors = native.native_diagnostics(file, DiagnosticCode::ParamTypeMismatch);
    assert_eq!(
        owned_errors.len(),
        mismatches,
        "{call}: {:?}",
        run.diagnostics
    );
    assert_eq!(native_errors.len(), mismatches, "{call}: {native_errors:?}");
    for (owned, direct) in owned_errors.iter().zip(&native_errors) {
        assert_eq!(owned.range, fixture.expr(file, argument).get_range());
        assert_eq!(
            direct.range,
            native.lsp_range(file, native.expr(file, argument).get_range())
        );
        assert_native_diagnostic(owned, direct);
    }
    assert_eq!(rows, expected_rows);
}

#[test]
fn selected_initializer_drives_vm_candidates_and_keeps_exact_source_receipt() {
    for (argument, expected, mismatches) in [
        ("'ok'", LuaType::String, 0),
        ("unresolved_argument", LuaType::String, 0),
        ("1", LuaType::Unknown, 1),
    ] {
        let source = FUNCTIONS.replace("f(1)", &format!("f({argument})"));
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let direct_source = format!(
            "---@param value string\n---@return string\nlocal f = {NEW}\nlocal result = f({argument})\n"
        );
        let native = Fixture::new(&[("main.lua", &direct_source)]);
        selected_call_matches_direct_native(&fixture, &native, 0, argument, expected, mismatches);
    }
}

#[test]
fn selected_absent_or_unavailable_initializer_cannot_resurrect_old_function() {
    let fixture = Fixture::new(&[("main.lua", FUNCTIONS)]);
    for (decision, expected, state, reason, callable_errors) in [
        (
            BindingDecision::KnownAbsent {
                proof: "initializer-proof".into(),
            },
            LuaType::Nil,
            "known_absent",
            None,
            1,
        ),
        (
            BindingDecision::Unavailable {
                proof: "initializer-proof".into(),
                reason: "not-observed".into(),
            },
            LuaType::Unknown,
            "unavailable",
            Some("not-observed"),
            0,
        ),
    ] {
        let site = fixture.site(0, OLD);
        let (_, rows) = fixture.run(0, context(site, decision.clone()), |query| {
            let callee = fixture.expr(0, "f");
            assert_eq!(
                query
                    .type_of_expr(callee.get_syntax_id())
                    .unwrap()
                    .structural_type(),
                &expected
            );
            assert!(
                query
                    .callable_candidates_for_expr(&callee)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                query
                    .type_of_expr(fixture.site(0, "f(1)").syntax)
                    .unwrap()
                    .structural_type(),
                &LuaType::Unknown
            );
        });
        assert_eq!(
            rows,
            [BindingCoverage {
                site,
                operation: "resolve",
                state,
                proof: "initializer-proof".into(),
                reason: reason.map(str::to_owned),
                parameter: None,
                parameter_index: None,
                argument: None,
            }]
        );
        let (run, _) = fixture.diagnose(
            context(site, decision),
            &[
                DiagnosticCode::CallNonCallable,
                DiagnosticCode::ParamTypeMismatch,
            ],
        );
        let errors = diagnostics(&run, DiagnosticCode::CallNonCallable);
        assert_eq!(errors.len(), callable_errors);
        if let Some(error) = errors.first() {
            assert_eq!(error.range, fixture.expr(0, "f").get_range());
            assert_eq!(error.message, "Cannot call expression of type `nil`.");
        }
        assert!(diagnostics(&run, DiagnosticCode::ParamTypeMismatch).is_empty());
    }
}

#[test]
fn parameter_annotations_and_old_overloads_do_not_override_selected_signature() {
    for (argument, mismatch) in [("'ok'", 0), ("1", 1)] {
        let source = format!(
            "---@type fun(value: number): number\nlocal f = {OLD}\n---@param value string\n---@return string\nlocal replacement = {NEW}\nlocal result = f({argument})\n"
        );
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let (run, _) = fixture.diagnose(
            context(fixture.site(0, OLD), fixture.function(0, NEW)),
            &[DiagnosticCode::ParamTypeMismatch],
        );
        let errors = diagnostics(&run, DiagnosticCode::ParamTypeMismatch);
        assert_eq!(errors.len(), mismatch, "{argument}: {:?}", run.diagnostics);
        if let Some(error) = errors.first() {
            assert_eq!(error.range, fixture.expr(0, argument).get_range());
            assert!(error.message.contains("string"), "{}", error.message);
        }
    }
    let source = format!(
        "---@overload fun(): number\n---@param old number\n---@return number\nlocal f = {OLD}\n---@param first string\n---@param second string\n---@return string\nlocal replacement = function(first, second) return first end\nlocal result = f()\n"
    );
    let fixture = Fixture::new(&[("main.lua", &source)]);
    let (run, _) = fixture.diagnose(
        context(
            fixture.site(0, OLD),
            fixture.function(0, "function(first, second) return first end"),
        ),
        &[DiagnosticCode::MissingParameter],
    );
    let errors = diagnostics(&run, DiagnosticCode::MissingParameter);
    assert_eq!(errors.len(), 1, "{:?}", run.diagnostics);
    assert!(errors[0].message.contains('2'), "{}", errors[0].message);
}

#[test]
fn actual_selected_generic_constraint_is_checked_instead_of_old_declaration_docs() {
    for (argument, mismatch) in [("'ok'", 0), ("1", 1)] {
        let source = format!(
            "---@generic T: number\n---@param old T\n---@return T\nlocal f = {OLD}\n---@generic U: string\n---@param value U\n---@return U\nlocal replacement = {NEW}\nlocal result = f({argument})\n"
        );
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let (run, _) = fixture.diagnose(
            context(fixture.site(0, OLD), fixture.function(0, NEW)),
            &[DiagnosticCode::GenericConstraintMismatch],
        );
        let errors = diagnostics(&run, DiagnosticCode::GenericConstraintMismatch);
        assert_eq!(errors.len(), mismatch, "{argument}: {:?}", run.diagnostics);
        if let Some(error) = errors.first() {
            let LuaExpr::CallExpr(call) = fixture.expr(0, &format!("f({argument})")) else {
                unreachable!()
            };
            assert_eq!(error.range, call.get_args_list().unwrap().get_range());
            assert!(error.message.contains("string"), "{}", error.message);
        }
    }
}

#[test]
fn return_count_uses_actual_selected_function_arity() {
    let source = "---@return number, number\nlocal f = function() return 1, 2 end\n---@return string\nlocal replacement = function() return 'ok' end\n---@return string, string\nlocal function wrapper() return f() end\n";
    let fixture = Fixture::new(&[("main.lua", source)]);
    let (run, _) = fixture.diagnose(
        context(
            fixture.site(0, "function() return 1, 2 end"),
            fixture.function(0, "function() return 'ok' end"),
        ),
        &[DiagnosticCode::MissingReturnValue],
    );
    let errors = diagnostics(&run, DiagnosticCode::MissingReturnValue);
    assert_eq!(errors.len(), 1, "{:?}", run.diagnostics);
    assert_eq!(
        errors[0].message,
        "Annotations specify that at least 2 return value(s) are required, found 1 returned here instead."
    );
}

#[test]
fn async_and_assert_consumers_use_selected_initializer_values() {
    for (old_async, new_async, expected) in [(true, false, 0), (false, true, 1)] {
        let source = format!(
            "{}---@return number\nlocal f = function() return 1 end\n{}---@return string\nlocal replacement = function() return 'ok' end\nf()\n",
            if old_async { "---@async\n" } else { "" },
            if new_async { "---@async\n" } else { "" }
        );
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let (run, _) = fixture.diagnose(
            context(
                fixture.site(0, "function() return 1 end"),
                fixture.function(0, "function() return 'ok' end"),
            ),
            &[DiagnosticCode::AwaitInSync],
        );
        assert_eq!(
            diagnostics(&run, DiagnosticCode::AwaitInSync).len(),
            expected,
            "{:?}",
            run.diagnostics
        );
    }
    let fixture = Fixture::new(&[(
        "main.lua",
        "---@type true\nlocal value = true\nassert(value)\n",
    )]);
    for (decision, expected) in [
        (
            BindingDecision::KnownAbsent {
                proof: "initializer-proof".into(),
            },
            Some("Impossible assert: this expression is always falsy; prefer `error()`"),
        ),
        (
            BindingDecision::Unavailable {
                proof: "initializer-proof".into(),
                reason: "not-observed".into(),
            },
            None,
        ),
    ] {
        let (run, _) = fixture.diagnose(
            context(fixture.site(0, "true"), decision),
            &[DiagnosticCode::UnnecessaryAssert],
        );
        let errors = diagnostics(&run, DiagnosticCode::UnnecessaryAssert);
        assert_eq!(errors.len(), usize::from(expected.is_some()));
        if let Some(message) = expected {
            assert_eq!(errors[0].message, message);
        }
    }
}

#[test]
fn foreign_initializer_uses_its_own_file_identity_and_original_control() {
    const DEFINITION: &str =
        "---@param old number\n---@return number\nf = function(old) return old end\n";
    const DIRECT_DEFINITION: &str =
        "---@param value string\n---@return string\nf = function(value) return value end\n";
    let consumer = |argument: &str| {
        format!(
            "---@param value string\n---@return string\nlocal replacement = {NEW}\nlocal result = f({argument})\n"
        )
    };
    for (argument, expected, mismatches) in [
        ("'ok'", LuaType::String, 0),
        ("unresolved_argument", LuaType::String, 0),
        ("1", LuaType::Unknown, 1),
    ] {
        let source = consumer(argument);
        let fixture = Fixture::new(&[("definition.lua", DEFINITION), ("consumer.lua", &source)]);
        let native = Fixture::new(&[
            ("definition.lua", DIRECT_DEFINITION),
            ("consumer.lua", &source),
        ]);
        selected_call_matches_direct_native(&fixture, &native, 1, argument, expected, mismatches);
    }
    let source = consumer("'ok'");
    let fixture = Fixture::new(&[("definition.lua", DEFINITION), ("consumer.lua", &source)]);
    let origin = fixture.site(0, OLD);
    let model = fixture.analysis.semantic_model(fixture.batch.file_ids[1]);
    let id = model
        .resolve_name(fixture.expr(1, "f").get_position())
        .unwrap();
    let session = OwnedBindingSession::new_checked(
        Arc::new(context(origin, fixture.function(1, NEW))),
        || Ok::<_, ()>(()),
    )
    .unwrap();
    let mut check = || Err("original-deadline");
    let mut query = OwnedQuery::new_with_contribution_order(
        &fixture.analysis,
        &session,
        fixture.batch.file_ids[1],
        &mut check,
        Arc::clone(&fixture.batch.contribution_order),
    );
    assert!(matches!(
        query.owner_initializer_managed(&id),
        Err(OwnedQueryFailure::Control("original-deadline"))
    ));
    assert!(
        session
            .coverage_checked(|| Ok::<_, ()>(()))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn assignment_overload_shortcut_cannot_accept_a_different_selected_result() {
    let source = "---@overload fun(): number\n---@return number\nlocal f = function() return 1 end\n---@return string\nlocal replacement = function() return 'ok' end\n---@type number\nlocal result = f()\n";
    let fixture = Fixture::new(&[("main.lua", source)]);
    let (run, _) = fixture.diagnose(
        context(
            fixture.site(0, "function() return 1 end"),
            fixture.function(0, "function() return 'ok' end"),
        ),
        &[DiagnosticCode::AssignTypeMismatch],
    );
    let errors = diagnostics(&run, DiagnosticCode::AssignTypeMismatch);
    assert_eq!(errors.len(), 1, "{:?}", run.diagnostics);
    assert_eq!(errors[0].range, fixture.expr(0, "f()").get_range());
    // Detailed assignment failures include the fixed checker's reason suffix.
    // Keep both clauses and compare the full diagnostic with a direct native
    // assignment; deleting the suffix would change production diagnostics.
    assert_eq!(
        errors[0].message,
        "expected `number` but found `string`. expected `number`, found `string`"
    );
    let direct_source = "---@return string\nlocal replacement = function() return 'ok' end\n---@type number\nlocal result = replacement()\n";
    let native = Fixture::new(&[("main.lua", direct_source)]);
    let native_errors = native.native_diagnostics(0, DiagnosticCode::AssignTypeMismatch);
    assert_eq!(native_errors.len(), 1);
    assert_eq!(
        native_errors[0].range,
        native.lsp_range(0, native.expr(0, "replacement()").get_range())
    );
    assert_native_diagnostic(errors[0], &native_errors[0]);
}

#[test]
fn cached_initializer_selection_still_checks_the_original_control() {
    use std::cell::Cell;
    let fixture = Fixture::new(&[("main.lua", FUNCTIONS)]);
    let origin = fixture.site(0, OLD);
    let session = OwnedBindingSession::new_checked(
        Arc::new(context(origin, fixture.function(0, NEW))),
        || Ok::<_, &str>(()),
    )
    .unwrap();
    let model = fixture.analysis.semantic_model(fixture.batch.file_ids[0]);
    let declaration = model
        .resolve_name(fixture.expr(0, "f").get_position())
        .unwrap();
    let stopped = Cell::new(false);
    let mut check = || {
        if stopped.get() {
            Err("original-deadline")
        } else {
            Ok(())
        }
    };
    let mut query = OwnedQuery::new_with_contribution_order(
        &fixture.analysis,
        &session,
        fixture.batch.file_ids[0],
        &mut check,
        Arc::clone(&fixture.batch.contribution_order),
    );
    query.type_of_decl(&declaration).unwrap().unwrap();
    assert_eq!(
        query.initializer_selection.borrow().get(&origin),
        Some(&true)
    );
    let before = session.coverage_checked(|| Ok::<_, ()>(())).unwrap();
    stopped.set(true);
    assert!(matches!(
        query.owner_initializer_managed(&declaration),
        Err(OwnedQueryFailure::Control("original-deadline"))
    ));
    assert_eq!(
        session.coverage_checked(|| Ok::<_, ()>(())).unwrap(),
        before
    );
    stopped.set(false);
    assert!(query.owner_initializer_managed(&declaration).unwrap());
    assert_eq!(
        session.coverage_checked(|| Ok::<_, ()>(())).unwrap(),
        before
    );
}
