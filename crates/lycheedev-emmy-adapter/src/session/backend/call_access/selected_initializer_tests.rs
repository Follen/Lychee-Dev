//! Public ingestion -> actual owned query/43-checker regressions for initializer
//! provenance. Expected types and diagnostics are independent source contracts.
use crate::session::{
    BindingContext, BindingCoverage, BindingDecision, BindingSite, BoundValue, CallPermit,
    Diagnostic, DiagnosticRun, FunctionRef, OwnedBindingSession, OwnedQuery, OwnedQueryFailure,
    SelectedValue,
};
use crate::{IngestedBatch, ingest_public_batch};
use emmylua_code_analysis::{CheckConfig, DiagnosticCode, EmmyLuaAnalysis, Emmyrc, LuaType};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaReturnStat};
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
struct Context {
    values: HashMap<BindingSite, BindingDecision<BoundValue>>,
    calls: HashMap<BindingSite, CallPermit>,
}
impl BindingContext for Context {
    fn identity(&self) -> &str {
        "selected-initializer-callable"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.values
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        self.calls
            .get(&site)
            .cloned()
            .map(BindingDecision::Bound)
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
    Context {
        values: HashMap::from([(site, value)]),
        ..Default::default()
    }
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
    let mut diagnostic_rows = expected_rows.to_vec();
    let closures = if fixture.batch.file_ids[file] == origin.file_id {
        vec![OLD, NEW]
    } else {
        vec![NEW]
    };
    for text in closures {
        let closure = fixture.expr(file, text);
        let returned = closure
            .syntax()
            .descendants()
            .find_map(LuaReturnStat::cast)
            .unwrap();
        for (syntax, operation) in [
            (returned.get_syntax_id(), "return_count_check"),
            (closure.get_syntax_id(), "return_flow_check"),
        ] {
            diagnostic_rows.push(BindingCoverage {
                site: BindingSite {
                    file_id: fixture.batch.file_ids[file],
                    syntax,
                },
                operation,
                state: "native_checked",
                proof: String::new(),
                reason: None,
                parameter: None,
                parameter_index: None,
                argument: None,
            });
        }
    }
    assert_eq!(rows, diagnostic_rows);
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
fn selected_count_and_callable_receipts_use_shape_not_value_types() {
    for (source, closure, call, ready, redundant) in [
        (
            "local selected=function(...) end\nselected(1,2,3)\n",
            "function(...) end",
            "selected(1,2,3)",
            true,
            false,
        ),
        (
            "---@return UnknownValue\nlocal selected=function(value) return value end\nselected(1)\n",
            "function(value) return value end",
            "selected(1)",
            true,
            false,
        ),
        (
            "local selected=function(value) end\nselected(External())\n",
            "function(value) end",
            "selected(External())",
            false,
            false,
        ),
        (
            "local selected=function() end\nselected(1,External())\n",
            "function() end",
            "selected(1,External())",
            false,
            true,
        ),
    ] {
        let fixture = Fixture::new(&[("main.lua", source)]);
        let callee = fixture.site(0, "selected");
        let call = fixture.site(0, call);
        let mut context = context(callee, fixture.function(0, closure));
        context.calls.insert(
            call,
            CallPermit {
                callee,
                proof: "count-permit".into(),
                capability_profile: "typed-call-v1".into(),
            },
        );
        let (run, rows) = fixture.diagnose(
            context,
            &[
                DiagnosticCode::MissingParameter,
                DiagnosticCode::RedundantParameter,
                DiagnosticCode::CallNonCallable,
            ],
        );
        for code in [
            DiagnosticCode::MissingParameter,
            DiagnosticCode::RedundantParameter,
            DiagnosticCode::CallNonCallable,
        ] {
            assert_eq!(
                !diagnostics(&run, code).is_empty(),
                redundant && code == DiagnosticCode::RedundantParameter,
                "{source}: {:?}",
                run.diagnostics
            );
        }
        let counts = rows
            .iter()
            .filter(|row| row.site == call && row.operation == "argument_count_check")
            .collect::<Vec<_>>();
        assert_eq!(counts.len(), 1, "{source}: {rows:?}");
        assert_eq!(
            counts[0].state,
            if ready {
                "native_checked"
            } else {
                "unavailable"
            }
        );
        assert_eq!(counts[0].proof, "count-permit");
        assert!(
            rows.iter().any(|row| row.site == call
                && row.operation == "callable_check"
                && row.state == "native_checked"),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn environment_required_unknown_slot_keeps_exact_arity() {
    use crate::session::{
        EnvironmentCallable, EnvironmentOrigin, EnvironmentRecord, EnvironmentSlot,
        EnvironmentTypeCatalog,
    };
    let callable = Arc::new(EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "api.json".into(),
            material_sha256: "a".repeat(64),
            pointer: "/functions/0".into(),
            identity: "fixed-api".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "Api".into(),
            parameters: vec![EnvironmentSlot {
                index: 1,
                name: "unit".into(),
                type_name: "UnprojectedUnitToken".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog {
            identity: "fixed-api-types".into(),
            ..Default::default()
        }),
    });
    for (source, call, diagnostic) in [
        ("Api()\n", "Api()", Some(DiagnosticCode::MissingParameter)),
        ("Api(1)\n", "Api(1)", None),
        (
            "Api(1,2)\n",
            "Api(1,2)",
            Some(DiagnosticCode::RedundantParameter),
        ),
    ] {
        let fixture = Fixture::new(&[("main.lua", source)]);
        let callee = fixture.site(0, "Api");
        let call = fixture.site(0, call);
        let mut context = context(
            callee,
            BindingDecision::Bound(BoundValue {
                identity: "api-selected".into(),
                proof: "api-proof".into(),
                value: SelectedValue::EnvironmentCallable(Arc::clone(&callable)),
            }),
        );
        context.calls.insert(
            call,
            CallPermit {
                callee,
                proof: "api-permit".into(),
                capability_profile: "authenticated_environment_signature_v1".into(),
            },
        );
        let (run, rows) = fixture.diagnose(
            context,
            &[
                DiagnosticCode::MissingParameter,
                DiagnosticCode::RedundantParameter,
            ],
        );
        for code in [
            DiagnosticCode::MissingParameter,
            DiagnosticCode::RedundantParameter,
        ] {
            assert_eq!(
                !diagnostics(&run, code).is_empty(),
                diagnostic == Some(code),
                "{source}: {:?}",
                run.diagnostics
            );
        }
        assert!(
            rows.iter().any(|row| row.site == call
                && row.operation == "argument_count_check"
                && row.state == "native_checked"),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn callback_count_receipt_and_excess_parameter_keep_the_selected_call_owner() {
    let source = "---@param callback fun()\nlocal selected=function(callback) end\nselected(function(extra) end)\n";
    let fixture = Fixture::new(&[("main.lua", source)]);
    let callee = fixture.site(0, "selected");
    let call = fixture.site(0, "selected(function(extra) end)");
    let mut context = context(callee, fixture.function(0, "function(callback) end"));
    context.calls.insert(
        call,
        CallPermit {
            callee,
            proof: "callback-permit".into(),
            capability_profile: "typed-call-v1".into(),
        },
    );
    let (run, rows) = fixture.diagnose(context, &[DiagnosticCode::RedundantParameter]);
    let errors = diagnostics(&run, DiagnosticCode::RedundantParameter);
    assert_eq!(errors.len(), 1, "{:?}", run.diagnostics);
    assert_eq!(
        usize::from(errors[0].range.start()),
        source.find("extra").unwrap()
    );
    assert_eq!(
        usize::from(errors[0].range.end()),
        source.find("extra").unwrap() + 5
    );
    let owner = &errors[0].data.as_ref().unwrap()["bindingCheckOwner"];
    assert_eq!(owner["start"], u32::from(call.syntax.get_range().start()));
    assert_eq!(owner["end"], u32::from(call.syntax.get_range().end()));
    assert!(
        rows.iter().any(|row| row.site == call
            && row.operation == "callback_count_check"
            && row.state == "native_checked"
            && row.proof == "callback-permit"),
        "{rows:?}"
    );
}

#[test]
fn return_receipts_follow_actual_counts_and_flow_without_claiming_unknown_tails() {
    for (source, operation, ready, diagnostic) in [
        (
            "---@return number\nlocal function subject() return 1 end\n",
            "return_count_check",
            true,
            None,
        ),
        (
            "---@return number\nlocal function subject() return end\n",
            "return_count_check",
            true,
            Some(DiagnosticCode::MissingReturnValue),
        ),
        (
            "---@return number\nlocal function subject() end\n",
            "return_flow_check",
            true,
            Some(DiagnosticCode::MissingReturn),
        ),
        (
            "---@return number?\nlocal function subject() end\n",
            "return_flow_check",
            true,
            None,
        ),
        (
            "---@return nil\nlocal function subject() end\n",
            "return_flow_check",
            true,
            None,
        ),
        (
            "---@return number\nlocal function subject() return External() end\n",
            "return_count_check",
            false,
            None,
        ),
        (
            "---@return UndeclaredReturnType\nlocal function subject() end\n",
            "return_count_check",
            false,
            None,
        ),
        (
            "local function pair() return 1,2 end\n---@return number, number\nlocal function subject() return pair(),3 end\n",
            "return_count_check",
            true,
            None,
        ),
        (
            "local function empty() end\n---@return number\nlocal function subject() return empty() end\n",
            "return_count_check",
            true,
            Some(DiagnosticCode::MissingReturnValue),
        ),
    ] {
        let fixture = Fixture::new(&[("main.lua", source)]);
        let (run, rows) = fixture.diagnose(
            Context::default(),
            &[
                DiagnosticCode::MissingReturnValue,
                DiagnosticCode::RedundantReturnValue,
                DiagnosticCode::MissingReturn,
            ],
        );
        for code in [
            DiagnosticCode::MissingReturnValue,
            DiagnosticCode::RedundantReturnValue,
            DiagnosticCode::MissingReturn,
        ] {
            assert_eq!(
                !diagnostics(&run, code).is_empty(),
                diagnostic == Some(code),
                "{source}: {:?}",
                run.diagnostics
            );
        }
        let start = source.find("local function subject").unwrap();
        let receipts = rows
            .iter()
            .filter(|row| {
                row.operation == operation
                    && u32::from(row.site.syntax.get_range().start()) as usize >= start
            })
            .collect::<Vec<_>>();
        assert!(!receipts.is_empty(), "{source}: {rows:?}");
        assert!(
            receipts.iter().all(|row| row.state
                == if ready {
                    "native_checked"
                } else {
                    "unavailable"
                }),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn return_type_checks_align_scalar_tail_tuple_and_variadic_slots() {
    for (source, errors) in [
        (
            "---@return number, number\nlocal function subject() return 1,2 end\n",
            0,
        ),
        (
            "local function pair() return 1,2 end\n---@return number, number\nlocal function subject() return pair(),3 end\n",
            0,
        ),
        (
            "local function pair() return 1,2 end\n---@return number, number\nlocal function subject() return pair() end\n",
            0,
        ),
        (
            "local function pair() return 1,2 end\n---@return number\nlocal function subject() return (pair()) end\n",
            0,
        ),
        (
            "---@return number, string\nlocal function subject() return 1,2 end\n",
            1,
        ),
        (
            "local function pair() return 1,2 end\n---@return string, string\nlocal function subject() return pair() end\n",
            2,
        ),
        (
            "---@return number, string...\nlocal function subject() return 1,'a','b' end\n",
            0,
        ),
        (
            "---@return number, string...\nlocal function subject() return 1,'a',2 end\n",
            1,
        ),
        (
            "---@param value [number, string]\n---@return [number, string]\nlocal function subject(value) return value end\n",
            0,
        ),
        (
            "---@param value [number, number]\n---@return [number, string]\nlocal function subject(value) return value end\n",
            1,
        ),
        (
            "---@return number[]\nlocal function subject() return {1,2} end\n",
            0,
        ),
        (
            "---@return number, string\nlocal function subject() return 1,External() end\n",
            0,
        ),
        (
            "---@return string, string\nlocal function subject() return 1,External() end\n",
            1,
        ),
        (
            "---@generic T\n---@param value T\n---@return T\nlocal function subject(value) return value end\n",
            0,
        ),
        (
            "---@return never\nlocal function subject() return 1 end\n",
            1,
        ),
    ] {
        let fixture = Fixture::new(&[("main.lua", source)]);
        let (run, _) = fixture.diagnose(Context::default(), &[DiagnosticCode::ReturnTypeMismatch]);
        let found = diagnostics(&run, DiagnosticCode::ReturnTypeMismatch);
        assert_eq!(found.len(), errors, "{source}: {:?}", run.diagnostics);
        let expression_start = source.rfind("return ").unwrap() + "return ".len();
        for diagnostic in found {
            assert!(
                u32::from(diagnostic.range.start()) as usize >= expression_start,
                "{source}: {diagnostic:?}"
            );
        }
    }
}

#[test]
fn typed_open_return_tails_compare_elements_without_claiming_fixed_cardinality() {
    use emmylua_code_analysis::VariadicType;
    for (annotation, prefix, element, expected, errors) in [
        ("number...", false, LuaType::Number, "number, number", 0),
        ("number...", false, LuaType::Number, "number, string", 1),
        ("number...", false, LuaType::Number, "number...", 0),
        ("number...", false, LuaType::Number, "string...", 1),
        ("number...", false, LuaType::Number, "number, string...", 1),
        (
            "number, string...",
            true,
            LuaType::String,
            "number, string, string",
            0,
        ),
        (
            "number, string...",
            true,
            LuaType::String,
            "number, number, string",
            1,
        ),
        (
            "number, string...",
            true,
            LuaType::String,
            "number, string...",
            0,
        ),
        (
            "number, string...",
            true,
            LuaType::String,
            "number, number...",
            1,
        ),
        (
            "number, string...",
            true,
            LuaType::String,
            "number, number, string...",
            1,
        ),
        ("unknown...", false, LuaType::Unknown, "number, string", 0),
        ("any...", false, LuaType::Any, "string...", 0),
        (
            "number, unknown...",
            true,
            LuaType::Unknown,
            "number, string, string",
            0,
        ),
        ("number, any...", true, LuaType::Any, "string, string...", 1),
    ] {
        let source = format!(
            "---@return {annotation}\nlocal generated=function() end\n---@return {expected}\nlocal function subject() return generated() end\n"
        );
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let callee = fixture.site(0, "generated");
        let call = fixture.site(0, "generated()");
        let make_context = || {
            let mut context = context(callee, fixture.function(0, "function() end"));
            context.calls.insert(
                call,
                CallPermit {
                    callee,
                    proof: "open-return-permit".into(),
                    capability_profile: "typed-call-v1".into(),
                },
            );
            context
        };
        // Assert the actual input of the checker, rather than relying on a
        // fixture name to imply that an open return pack was produced.
        let (actual, _) = fixture.run(0, make_context(), |query| {
            query
                .type_of_expr_at(call.syntax, call.syntax.get_range().start().into())
                .unwrap()
        });
        let LuaType::Variadic(pack) = actual.structural_type() else {
            panic!("{source}: expected an actual variadic pack, got {actual:?}");
        };
        assert_eq!(pack.get_max_len(), None, "{source}: {actual:?}");
        let tail = if prefix {
            let VariadicType::Multi(types) = pack.as_ref() else {
                panic!("{source}: expected Multi(prefix, Base<T>), got {actual:?}");
            };
            assert_eq!(types.len(), 2, "{source}: {actual:?}");
            assert_eq!(types[0], LuaType::Number);
            let LuaType::Variadic(tail) = &types[1] else {
                panic!("{actual:?}");
            };
            assert_eq!(pack.get_min_len(), Some(1));
            tail
        } else {
            assert_eq!(pack.get_min_len(), None);
            pack
        };
        assert!(
            matches!(tail.as_ref(), VariadicType::Base(ty) if ty == &element),
            "{source}: {actual:?}"
        );
        let (run, rows) = fixture.diagnose(make_context(), &[DiagnosticCode::ReturnTypeMismatch]);
        let found = diagnostics(&run, DiagnosticCode::ReturnTypeMismatch);
        assert_eq!(found.len(), errors, "{source}: {:?}", run.diagnostics);
        for error in found {
            assert_eq!(error.range, call.syntax.get_range(), "{source}: {error:?}");
        }
        let return_rows = rows
            .iter()
            .filter(|row| row.site == call && row.operation == "return_check")
            .collect::<Vec<_>>();
        assert_eq!(return_rows.len(), 1, "{source}: {rows:?}");
        assert_eq!(return_rows[0].state, "unavailable", "{source}: {rows:?}");
        assert_eq!(return_rows[0].proof, "open-return-permit");
        let subject_start = source.find("local function subject").unwrap();
        let counts = rows
            .iter()
            .filter(|row| {
                row.operation == "return_count_check"
                    && u32::from(row.site.syntax.get_range().start()) as usize >= subject_start
            })
            .collect::<Vec<_>>();
        assert!(!counts.is_empty(), "{source}: {rows:?}");
        assert!(
            counts.iter().all(|row| row.state == "unavailable"),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn authenticated_environment_return_pack_is_checked_against_the_declared_slot() {
    use crate::session::{
        EnvironmentCallable, EnvironmentOrigin, EnvironmentRecord, EnvironmentSlot,
        EnvironmentTypeCatalog,
    };
    let source = "---@return string\nlocal function subject() return Api() end\n";
    let fixture = Fixture::new(&[("main.lua", source)]);
    let callee = fixture.site(0, "Api");
    let call = fixture.site(0, "Api()");
    let callable = Arc::new(EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "api.json".into(),
            material_sha256: "a".repeat(64),
            pointer: "/functions/0".into(),
            identity: "fixed-return-api".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "Api".into(),
            returns: vec![EnvironmentSlot {
                index: 1,
                name: "value".into(),
                type_name: "number".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog {
            identity: "fixed-return-types".into(),
            ..Default::default()
        }),
    });
    let mut context = context(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "api-return-selected".into(),
            proof: "api-return-proof".into(),
            value: SelectedValue::EnvironmentCallable(callable),
        }),
    );
    context.calls.insert(
        call,
        CallPermit {
            callee,
            proof: "api-return-permit".into(),
            capability_profile: "authenticated_environment_signature_v1".into(),
        },
    );
    let (run, rows) = fixture.diagnose(context, &[DiagnosticCode::ReturnTypeMismatch]);
    let errors = diagnostics(&run, DiagnosticCode::ReturnTypeMismatch);
    assert_eq!(errors.len(), 1, "{:?}", run.diagnostics);
    assert_eq!(errors[0].range, call.syntax.get_range());
    assert!(
        errors[0].message.contains("string") && errors[0].message.contains("number"),
        "{:?}",
        errors[0]
    );
    assert!(
        rows.iter().any(|row| row.site == call
            && row.operation == "return_check"
            && row.state == "native_checked"),
        "{rows:?}"
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

#[test]
fn return_count_keeps_required_open_prefix_and_vararg_scalar_positions() {
    for (source, expected, checked) in [
        (
            "---@return number, string...\nlocal function subject() end\n",
            Some(DiagnosticCode::MissingReturn),
            true,
        ),
        (
            "---@return number, string...\nlocal function subject() return end\n",
            Some(DiagnosticCode::MissingReturnValue),
            true,
        ),
        (
            "---@return number, string...\nlocal function subject() return 1 end\n",
            None,
            true,
        ),
        (
            "---@return number?, string...\nlocal function subject() end\n",
            None,
            true,
        ),
        (
            "---@param ... number\n---@return number, number\nlocal function subject(...) return (...) end\n",
            Some(DiagnosticCode::MissingReturnValue),
            true,
        ),
        (
            "---@param ... number\n---@return number\nlocal function subject(...) return (...) end\n",
            None,
            true,
        ),
        (
            "---@param ... number\n---@return number, number\nlocal function subject(...) return ..., 3 end\n",
            None,
            true,
        ),
        (
            "---@param ... number\n---@return number, number\nlocal function subject(...) return ... end\n",
            None,
            false,
        ),
    ] {
        let fixture = Fixture::new(&[("main.lua", source)]);
        let (run, rows) = fixture.diagnose(
            Context::default(),
            &[
                DiagnosticCode::MissingReturn,
                DiagnosticCode::MissingReturnValue,
                DiagnosticCode::RedundantReturnValue,
                DiagnosticCode::ReturnTypeMismatch,
            ],
        );
        for code in [
            DiagnosticCode::MissingReturn,
            DiagnosticCode::MissingReturnValue,
            DiagnosticCode::RedundantReturnValue,
            DiagnosticCode::ReturnTypeMismatch,
        ] {
            assert_eq!(
                !diagnostics(&run, code).is_empty(),
                expected == Some(code),
                "{source}: {:?}",
                run.diagnostics
            );
        }
        let start = source.find("local function subject").unwrap();
        let operation = if source.contains("return ...")
            || source.contains("return (")
            || source.contains("return end")
            || source.contains("return 1")
        {
            "return_count_check"
        } else {
            "return_flow_check"
        };
        let found = rows
            .iter()
            .filter(|row| {
                row.operation == operation
                    && u32::from(row.site.syntax.get_range().start()) as usize >= start
            })
            .collect::<Vec<_>>();
        assert!(!found.is_empty(), "{source}: {rows:?}");
        assert!(
            found.iter().all(|row| row.state
                == if checked {
                    "native_checked"
                } else {
                    "unavailable"
                }),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn return_pack_unions_keep_branch_arity_slots_and_typed_tails() {
    use crate::session::OwnedValue;
    use crate::session::backend::return_pack::{
        ReturnArity, return_arity, return_has_typed_open_tail,
    };
    use emmylua_code_analysis::{LuaTupleStatus, LuaTupleType, VariadicType};
    let base = |ty| OwnedValue::plain(LuaType::Variadic(Arc::new(VariadicType::Base(ty))));
    let fixture = Fixture::new(&[("main.lua", "local value=1")]);
    let number = OwnedValue::plain(LuaType::Number);
    let string = OwnedValue::plain(LuaType::String);
    let known_unknown = OwnedValue::pack(vec![number.clone(), OwnedValue::plain(LuaType::Unknown)]);
    let union = |values| OwnedValue::union_checked(values, || Ok::<_, ()>(())).unwrap();
    let same = union(vec![
        OwnedValue::pack(vec![number.clone(), string.clone()]),
        OwnedValue::pack(vec![number.clone(), number.clone()]),
    ]);
    let different = union(vec![
        number.clone(),
        OwnedValue::pack(vec![number.clone(), string.clone()]),
    ]);
    let open = union(vec![
        base(LuaType::String),
        OwnedValue::pack(vec![number.clone(), base(LuaType::Number)]),
    ]);
    assert_eq!(
        return_arity(known_unknown.structural_type()),
        Some(ReturnArity::fixed(2))
    );
    assert_eq!(
        return_arity(same.structural_type()),
        Some(ReturnArity::fixed(2))
    );
    assert_eq!(
        return_arity(different.structural_type()),
        Some(ReturnArity {
            min: 1,
            max: Some(2)
        })
    );
    assert_eq!(
        return_arity(open.structural_type()),
        Some(ReturnArity { min: 0, max: None })
    );
    assert!(return_has_typed_open_tail(open.structural_type()));
    assert_eq!(return_arity(&LuaType::Never), None);
    let tuple = LuaType::Tuple(Arc::new(LuaTupleType::new(
        vec![LuaType::Number, LuaType::String],
        LuaTupleStatus::DocResolve,
    )));
    assert_eq!(return_arity(&tuple), Some(ReturnArity::fixed(1)));
    assert_eq!(
        return_arity(base(tuple).structural_type()),
        Some(ReturnArity::fixed(2))
    );
    let ((known_second, absent, open_second), _) = fixture.run(0, Context::default(), |query| {
        (
            query.owned_return_slot(&known_unknown, 1).unwrap().unwrap(),
            query.owned_return_slot(&different, 1).unwrap().unwrap(),
            query.owned_return_slot(&open, 1).unwrap().unwrap(),
        )
    });
    assert_eq!(known_second.structural_type(), &LuaType::Unknown);
    assert_eq!(
        absent.structural_type(),
        &LuaType::from_vec(vec![LuaType::Nil, LuaType::String])
    );
    assert_eq!(
        open_second.structural_type(),
        &LuaType::from_vec(vec![LuaType::String, LuaType::Number])
    );
}

#[test]
fn never_return_does_not_claim_one_value_or_complete_count_comparison() {
    let source = "---@return never\nlocal generated=function() end\n---@return number, number\nlocal function subject() return generated() end\n";
    let fixture = Fixture::new(&[("main.lua", source)]);
    let callee = fixture.site(0, "generated");
    let call = fixture.site(0, "generated()");
    let mut selected = context(callee, fixture.function(0, "function() end"));
    selected.calls.insert(
        call,
        CallPermit {
            callee,
            proof: "never-permit".into(),
            capability_profile: "typed-call-v1".into(),
        },
    );
    let (run, rows) = fixture.diagnose(
        selected,
        &[
            DiagnosticCode::ReturnTypeMismatch,
            DiagnosticCode::MissingReturnValue,
        ],
    );
    let start = source.find("local function subject").unwrap();
    assert!(
        run.diagnostics
            .iter()
            .filter(|row| u32::from(row.range.start()) as usize >= start)
            .all(|row| !matches!(
                row.code,
                DiagnosticCode::ReturnTypeMismatch | DiagnosticCode::MissingReturnValue
            )),
        "{:?}",
        run.diagnostics
    );
    assert!(
        rows.iter().any(|row| row.operation == "return_count_check"
            && row.state == "unavailable"
            && u32::from(row.site.syntax.get_range().start()) as usize >= start),
        "{rows:?}"
    );
}

#[test]
fn inferred_return_signature_applies_vararg_list_adjustment_and_keeps_union_rows() {
    use crate::session::backend::return_pack::{ReturnArity, return_arity};
    for (body, expected) in [
        ("return ...", ReturnArity { min: 0, max: None }),
        ("return (...)", ReturnArity::fixed(1)),
        ("return ..., 3", ReturnArity::fixed(2)),
    ] {
        let source = format!("---@param ... number\nlocal generated=function(...) {body} end\n");
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let closure = fixture.site(0, &format!("function(...) {body} end"));
        let (actual, _) = fixture.run(0, Context::default(), |query| {
            query.return_type(closure.syntax).unwrap().unwrap()
        });
        assert_eq!(
            return_arity(actual.structural_type()),
            Some(expected),
            "{source}: {actual:?}"
        );
    }
    let source = "local generated=function(flag) if flag then return 1,'a' end return 2,'b' end\n";
    let fixture = Fixture::new(&[("main.lua", source)]);
    let closure = fixture.site(
        0,
        "function(flag) if flag then return 1,'a' end return 2,'b' end",
    );
    let (actual, _) = fixture.run(0, Context::default(), |query| {
        query.return_type(closure.syntax).unwrap().unwrap()
    });
    assert!(
        matches!(actual.structural_type(), LuaType::Union(_)),
        "{actual:?}"
    );
    assert_eq!(
        return_arity(actual.structural_type()),
        Some(ReturnArity::fixed(2))
    );
}

#[test]
fn return_pack_slot_keeps_selected_nominal_identity() {
    use crate::session::OwnedValue;
    let fixture = Fixture::new(&[("main.lua", "local value=1")]);
    fixture.run(0, Context::default(), |query| {
        let selected = OwnedValue::nominal(query.session, "fixed-catalog", "Duration");
        let pack = OwnedValue::pack(vec![OwnedValue::plain(LuaType::Number), selected.clone()]);
        let actual = query.owned_return_slot(&pack, 1).unwrap().unwrap();
        assert_eq!(actual, selected);
        assert_ne!(actual, OwnedValue::plain(actual.structural_type().clone()));
    });
}

#[test]
fn return_union_rows_reach_type_and_count_consumers_without_fixed_arity_claims() {
    use crate::session::backend::return_pack::{ReturnArity, return_arity};
    for (producer, expected, arity, mismatches, redundant, checked) in [
        (
            "local generated=function(flag) if flag then return 1,2 end return 3,4 end",
            "number",
            ReturnArity::fixed(2),
            0,
            true,
            true,
        ),
        (
            "local generated=function(flag) if flag then return 1 end return 3,4 end",
            "number",
            ReturnArity {
                min: 1,
                max: Some(2),
            },
            0,
            false,
            false,
        ),
        (
            "---@return string...\nlocal tail=function() end\nlocal generated=function(flag,...) if flag then return tail() end return 'prefix',tail() end",
            "number, number",
            ReturnArity { min: 0, max: None },
            2,
            false,
            false,
        ),
    ] {
        let source = format!(
            "{producer}\n---@return {expected}\nlocal function subject(flag,...) return generated(flag,...) end\n"
        );
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let call = fixture.site(0, "generated(flag,...)");
        let (actual, _) = fixture.run(0, Context::default(), |query| {
            query
                .type_of_expr_at(call.syntax, call.syntax.get_range().start().into())
                .unwrap()
        });
        assert!(
            matches!(actual.structural_type(), LuaType::Union(_)),
            "{source}: {actual:?}"
        );
        assert_eq!(
            return_arity(actual.structural_type()),
            Some(arity),
            "{source}: {actual:?}"
        );
        let (run, rows) = fixture.diagnose(
            Context::default(),
            &[
                DiagnosticCode::ReturnTypeMismatch,
                DiagnosticCode::RedundantReturnValue,
                DiagnosticCode::MissingReturnValue,
            ],
        );
        let start = source.find("local function subject").unwrap();
        let subject_diagnostics = |code| {
            run.diagnostics
                .iter()
                .filter(move |row| {
                    row.code == code && u32::from(row.range.start()) as usize >= start
                })
                .count()
        };
        assert_eq!(
            subject_diagnostics(DiagnosticCode::ReturnTypeMismatch),
            mismatches,
            "{source}: {:?}",
            run.diagnostics
        );
        assert_eq!(
            subject_diagnostics(DiagnosticCode::RedundantReturnValue),
            usize::from(redundant),
            "{source}: {:?}",
            run.diagnostics
        );
        assert_eq!(
            subject_diagnostics(DiagnosticCode::MissingReturnValue),
            0,
            "{source}: {:?}",
            run.diagnostics
        );
        let counts = rows
            .iter()
            .filter(|row| {
                row.operation == "return_count_check"
                    && u32::from(row.site.syntax.get_range().start()) as usize >= start
            })
            .collect::<Vec<_>>();
        assert!(!counts.is_empty(), "{source}: {rows:?}");
        assert!(
            counts.iter().all(|row| row.state
                == if checked {
                    "native_checked"
                } else {
                    "unavailable"
                }),
            "{source}: {rows:?}"
        );
    }
}

#[test]
fn known_return_prefix_reports_excess_without_inventing_unknown_tail_length() {
    for (expected, excess) in [("number", true), ("number, number, number", false)] {
        let source = format!(
            "---@return {expected}\nlocal function subject() return 1, 2, External() end\n"
        );
        let fixture = Fixture::new(&[("main.lua", &source)]);
        let (run, rows) = fixture.diagnose(
            Context::default(),
            &[
                DiagnosticCode::MissingReturnValue,
                DiagnosticCode::RedundantReturnValue,
            ],
        );
        assert!(
            diagnostics(&run, DiagnosticCode::MissingReturnValue).is_empty(),
            "{source}: {:?}",
            run.diagnostics
        );
        let found = diagnostics(&run, DiagnosticCode::RedundantReturnValue);
        assert_eq!(
            found.len(),
            usize::from(excess),
            "{source}: {:?}",
            run.diagnostics
        );
        if excess {
            assert_eq!(found[0].range, fixture.site(0, "2").syntax.get_range());
        }
        let start = source.find("local function subject").unwrap();
        let counts = rows
            .iter()
            .filter(|row| {
                row.operation == "return_count_check"
                    && u32::from(row.site.syntax.get_range().start()) as usize >= start
            })
            .collect::<Vec<_>>();
        assert!(!counts.is_empty(), "{rows:?}");
        assert!(
            counts.iter().all(|row| row.state == "unavailable"),
            "{rows:?}"
        );
    }
}
