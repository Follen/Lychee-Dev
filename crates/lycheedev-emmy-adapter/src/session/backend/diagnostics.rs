//! Owned checker scheduling and diagnostic filtering.
//!
//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! src/check/checker/mod.rs, Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
//! The engine's public diagnose entry is deliberately not called: it would run
//! a second registry, including file-level force-enabled selected consumers.

macro_rules! t {
    ($message:expr $(, $key:ident = $value:expr)* $(,)?) => {{
        let keys: &[&str] = &[$(stringify!($key)),*];
        let values: &[String] = &[$(format!("{}", $value)),*];
        $crate::session::backend::diagnostics::translate($message, keys, values)
    }};
}

mod access_invisible;
mod analyze_error;
mod assign_type_mismatch;
mod attribute_check;
mod await_in_sync;
mod builtin;
mod call_non_callable;
mod cast_type_mismatch;
mod check_export;
mod check_field;
mod check_return_count;
mod circle_doc_class;
mod deprecated;
mod discard_returns;
mod duplicate_field;
mod duplicate_index;
mod duplicate_require;
mod duplicate_type;
mod enum_value_mismatch;
mod generic_constraint_mismatch;
mod global_non_module;
mod incomplete_signature_doc;
mod invert_if;
mod local_const_reassign;
mod need_check_nil;
mod non_literal_expressions_in_assert;
mod param_count;
mod param_type_check;
mod preferred_local_alias;
mod readonly;
mod redefined_local;
mod require_module_visibility;
mod return_type_mismatch;
mod syntax_error;
mod type_access_modifier;
mod unbalanced_assignments;
mod undefined_doc_param;
mod undefined_field;
mod undefined_global;
mod unknown_doc_tag;
mod unnecessary_assert;
mod unnecessary_if;
mod unresolved_require;
mod unused;

use super::{OwnedQuery, QueryResult};
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::{CheckConfig, DiagnosticCode, SemanticModel};
use lsp_types::{DiagnosticSeverity, DiagnosticTag};
use rowan::TextRange;
use std::sync::Arc;

fn translate(template: &str, keys: &[&str], values: &[String]) -> String {
    // Fixed upstream uses unminified keys. Its generated public lookup applies
    // the same locale/fallback chain as its t! macro. No duplicated locale data.
    let locale = rust_i18n::locale();
    let translated = emmylua_code_analysis::_rust_i18n_try_translate(&locale, template)
        .unwrap_or_else(|| std::borrow::Cow::Borrowed(template));
    rust_i18n::replace_patterns(&translated, keys, values)
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub range: TextRange,
    pub code: DiagnosticCode,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub tags: Option<Vec<DiagnosticTag>>,
    pub data: Option<serde_json::Value>,
}

/// Execution facts, separate from selected semantic coverage. A stopped checker
/// is never reported as checked; configuration and definition skips are explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckerExecution {
    DefinitionFileSkipped,
    ConfigSkipped(&'static str),
    Started(&'static str),
    Completed(&'static str),
    Stopped(&'static str),
}

#[derive(Debug, Default)]
pub struct DiagnosticRun {
    pub diagnostics: Vec<Diagnostic>,
    pub execution: Vec<CheckerExecution>,
    pub complete: bool,
}

trait Checker {
    const CODES: &'static [DiagnosticCode];
    fn check(context: &mut CheckContext<'_>, model: &SemanticModel<'_>);
}

pub(super) struct CheckContext<'a> {
    pub semantic_model: &'a SemanticModel<'a>,
    pub config: Arc<CheckConfig>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> CheckContext<'a> {
    fn new(semantic_model: &'a SemanticModel<'a>, config: Arc<CheckConfig>) -> Self {
        Self {
            semantic_model,
            config,
            diagnostics: Vec::new(),
        }
    }

    pub fn add_diagnostic<T: AsRef<str>>(
        &mut self,
        code: DiagnosticCode,
        range: TextRange,
        message: T,
    ) {
        self.add_diagnostic_with_data(code, range, message.as_ref().into(), None);
    }

    pub fn add_diagnostic_with_data(
        &mut self,
        code: DiagnosticCode,
        range: TextRange,
        message: String,
        data: Option<serde_json::Value>,
    ) {
        if let Some(facts) = self.semantic_model.file_facts() {
            let force_enable = facts.file_diagnostic_enabled.contains(&code);
            if !force_enable {
                if !self.config.is_code_enabled(&code) {
                    return;
                }
                if facts.file_diagnostic_disabled.contains(&code) {
                    return;
                }
            }
            if facts.is_range_diagnostic_disabled(&code, &range) {
                return;
            }
        } else if !self.config.is_code_enabled(&code) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            range,
            code,
            message,
            severity: self.config.severity_of(&code),
            tags: match code {
                DiagnosticCode::Unused | DiagnosticCode::UnreachableCode => {
                    Some(vec![DiagnosticTag::UNNECESSARY])
                }
                DiagnosticCode::Deprecated => Some(vec![DiagnosticTag::DEPRECATED]),
                _ => None,
            },
            data,
        });
    }

    pub fn is_global_disabled(&self, name: &str) -> bool {
        self.config.is_global_disabled(name)
    }

    fn enabled(&self, codes: &[DiagnosticCode]) -> bool {
        let file_enabled = self
            .semantic_model
            .file_facts()
            .map(|facts| &facts.file_diagnostic_enabled);
        codes.iter().any(|code| {
            self.config.is_code_enabled(code)
                || file_enabled.is_some_and(|enabled| enabled.contains(code))
        })
    }
}

impl<E> OwnedQuery<'_, '_, E> {
    fn run_pure_checker<T: Checker>(
        &mut self,
        context: &mut CheckContext<'_>,
        run: &mut DiagnosticRun,
    ) -> QueryResult<(), E> {
        let name = std::any::type_name::<T>();
        self.run_owned_checker(context, run, name, T::CODES, |_, context| {
            let model = context.semantic_model;
            T::check(context, model);
            Ok(())
        })
    }

    fn run_owned_checker(
        &mut self,
        context: &mut CheckContext<'_>,
        run: &mut DiagnosticRun,
        name: &'static str,
        codes: &[DiagnosticCode],
        check: impl FnOnce(&mut Self, &mut CheckContext<'_>) -> QueryResult<(), E>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let start = std::time::Instant::now();
        if context.enabled(codes) {
            run.execution.push(CheckerExecution::Started(name));
            let result = check(self, context).and_then(|()| self.checkpoint());
            match result {
                Ok(()) => run.execution.push(CheckerExecution::Completed(name)),
                Err(error) => {
                    run.execution.push(CheckerExecution::Stopped(name));
                    return Err(error);
                }
            }
        } else {
            run.execution.push(CheckerExecution::ConfigSkipped(name));
        }
        if let Some(profile) = context.config.profile() {
            profile.record(name, start.elapsed());
        }
        self.checkpoint()
    }
}

impl<E> OwnedQuery<'_, '_, E> {
    /// Run exactly the fixed 43-checker registry, in producer order, on this
    /// query's sole session and caches. On interruption `run.complete` stays
    /// false and execution facts remain available; partial diagnostics are not
    /// promoted to a completed result. Reusing an output resets its old facts.
    pub fn diagnose_into(
        &mut self,
        config: Arc<CheckConfig>,
        run: &mut DiagnosticRun,
    ) -> QueryResult<(), E> {
        *run = DiagnosticRun::default();
        self.checkpoint()?;
        if self.model.file_facts().is_some_and(|facts| facts.is_meta)
            || self
                .model
                .workspace_id_of(self.model.file_id())
                .is_none_or(|workspace| !workspace.is_main())
        {
            run.execution.push(CheckerExecution::DefinitionFileSkipped);
            self.checkpoint()?;
            run.complete = true;
            return Ok(());
        }
        // This is only an immutable public fact/view handle, not another DB or
        // another inference session. All contextual queries below use `self`.
        let model = self.analysis.semantic_model(self.model.file_id());
        let context = &mut CheckContext::new(&model, config);
        self.run_pure_checker::<syntax_error::SyntaxErrorChecker>(context, run)?;
        self.run_pure_checker::<analyze_error::AnalyzeErrorChecker>(context, run)?;
        self.run_pure_checker::<unused::UnusedChecker>(context, run)?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::deprecated::DeprecatedChecker",
            &[DiagnosticCode::Deprecated],
            |query, context| query.check_deprecated(context),
        )?;
        self.run_pure_checker::<undefined_global::UndefinedGlobal>(context, run)?;
        self.run_pure_checker::<undefined_doc_param::UndefinedDocParamChecker>(context, run)?;
        self.run_pure_checker::<unbalanced_assignments::UnbalancedAssignmentsChecker>(
            context, run,
        )?;
        self.run_pure_checker::<redefined_local::RedefinedLocalChecker>(context, run)?;
        self.run_pure_checker::<duplicate_require::DuplicateRequireChecker>(context, run)?;
        self.run_pure_checker::<duplicate_type::DuplicateTypeChecker>(context, run)?;
        self.run_pure_checker::<type_access_modifier::InconsistentTypeAccessModifierChecker>(
            context, run,
        )?;
        self.run_pure_checker::<circle_doc_class::CircleDocClassChecker>(context, run)?;
        self.run_pure_checker::<unknown_doc_tag::UnknownDocTagChecker>(context, run)?;
        self.run_pure_checker::<local_const_reassign::LocalConstReassignChecker>(context, run)?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::param_type_check::ParamTypeChecker",
            &[DiagnosticCode::ParamTypeMismatch],
            |query, context| query.check_param_type(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::return_type_mismatch::ReturnTypeMismatchChecker",
            &[DiagnosticCode::ReturnTypeMismatch],
            |query, context| query.check_return_type_mismatch(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::assign_type_mismatch::AssignTypeMismatchChecker",
            &[DiagnosticCode::AssignTypeMismatch],
            |query, context| query.check_assign_type_mismatch(context),
        )?;
        self.run_pure_checker::<invert_if::InvertIfChecker>(context, run)?;
        self.run_pure_checker::<non_literal_expressions_in_assert::NonLiteralExpressionsInAssertChecker>(context, run)?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::preferred_local_alias::PreferredLocalAliasChecker",
            &[DiagnosticCode::PreferredLocalAlias],
            |query, context| query.check_preferred_local_alias(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::generic_constraint_mismatch::GenericConstraintMismatchChecker",
            &[DiagnosticCode::GenericConstraintMismatch],
            |query, context| query.check_generic_constraint_mismatch(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::param_count::ParamCountChecker",
            &[
                DiagnosticCode::MissingParameter,
                DiagnosticCode::RedundantParameter,
            ],
            |query, context| query.check_param_count(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::check_return_count::CheckReturnCountChecker",
            &[
                DiagnosticCode::RedundantReturnValue,
                DiagnosticCode::MissingReturnValue,
                DiagnosticCode::MissingReturn,
            ],
            |query, context| query.check_return_count(context),
        )?;
        self.run_pure_checker::<incomplete_signature_doc::IncompleteSignatureDocChecker>(
            context, run,
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::unnecessary_assert::UnnecessaryAssertChecker",
            &[DiagnosticCode::UnnecessaryAssert],
            |query, context| query.check_unnecessary_assert(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::unnecessary_if::UnnecessaryIfChecker",
            &[DiagnosticCode::UnnecessaryIf],
            |query, context| query.check_unnecessary_if(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::await_in_sync::AwaitInSyncChecker",
            &[DiagnosticCode::AwaitInSync],
            |query, context| query.check_await_in_sync(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::access_invisible::AccessInvisibleChecker",
            &[DiagnosticCode::AccessInvisible],
            |query, context| query.check_access_invisible(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::cast_type_mismatch::CastTypeMismatchChecker",
            &[DiagnosticCode::CastTypeMismatch],
            |query, context| query.check_cast_type_mismatch(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::call_non_callable::CallNonCallableChecker",
            &[DiagnosticCode::CallNonCallable],
            |query, context| query.check_call_non_callable(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::enum_value_mismatch::EnumValueMismatchChecker",
            &[DiagnosticCode::EnumValueMismatch],
            |query, context| query.check_enum_value_mismatch(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::attribute_check::AttributeCheckChecker",
            &[
                DiagnosticCode::AttributeParamTypeMismatch,
                DiagnosticCode::AttributeMissingParameter,
                DiagnosticCode::AttributeRedundantParameter,
            ],
            |query, context| query.check_attribute_check(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::need_check_nil::NeedCheckNilChecker",
            &[DiagnosticCode::NeedCheckNil],
            |query, context| query.check_need_check_nil(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::unresolved_require::UnresolvedRequireChecker",
            &[DiagnosticCode::UnresolvedRequire],
            |query, context| query.check_unresolved_require(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::require_module_visibility::RequireModuleVisibilityChecker",
            &[DiagnosticCode::RequireModuleNotVisible],
            |query, context| query.check_require_module_visibility(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::undefined_field::UndefinedFieldChecker",
            &[DiagnosticCode::UndefinedField],
            |query, context| query.check_undefined_field(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::check_export::CheckExportChecker",
            &[DiagnosticCode::InjectField, DiagnosticCode::UndefinedField],
            |query, context| query.check_export(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::duplicate_field::DuplicateFieldChecker",
            &[
                DiagnosticCode::DuplicateDocField,
                DiagnosticCode::DuplicateSetField,
            ],
            |query, context| query.check_duplicate_field(context),
        )?;
        self.run_pure_checker::<duplicate_index::DuplicateIndexChecker>(context, run)?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::readonly::ReadOnlyChecker",
            &[DiagnosticCode::ReadOnly],
            |query, context| query.check_readonly(context),
        )?;
        self.run_owned_checker(
            context,
            run,
            "lycheedev_emmy_adapter::check_field::CheckFieldChecker",
            &[DiagnosticCode::MissingFields, DiagnosticCode::InjectField],
            |query, context| query.check_field(context),
        )?;
        self.run_pure_checker::<discard_returns::DiscardReturnsChecker>(context, run)?;
        self.run_pure_checker::<global_non_module::GlobalInNonModuleChecker>(context, run)?;
        self.checkpoint()?;
        run.diagnostics = std::mem::take(&mut context.diagnostics);
        run.complete = true;
        Ok(())
    }
}
