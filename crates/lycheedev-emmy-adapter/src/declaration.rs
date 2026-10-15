use crate::{
    ByteRange, CallableInput, CoverageRow, DeclarationFailure, DeclarationLookup, ParseIssue,
    ParseIssues, TypeProjection,
};
use emmylua_code_analysis::{
    EmmyLuaAnalysis, Emmyrc, EmmyrcLuaVersion, FileId, LuaFunctionType, LuaType,
    binding_context::{
        BindingContext, BindingDecision, BindingSession, BindingSite, BoundValue, SelectedValue,
    },
    humanize_semantic_type_detailed,
};
use emmylua_parser::{LuaAstNode, LuaExpr};
use std::{path::PathBuf, sync::Arc};

/// One private engine model for an already-authenticated generated definition
/// document. Its engine file IDs and syntax IDs never cross the owned API.
pub struct DeclarationModel {
    analysis: EmmyLuaAnalysis,
    file_id: FileId,
    bytes: usize,
}

impl DeclarationModel {
    pub fn new<E>(
        definitions: Arc<str>,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Self, DeclarationFailure<E>> {
        check().map_err(DeclarationFailure::Interrupted)?;
        if definitions.len() > 8 << 20 {
            return Err(DeclarationFailure::InvalidInput(
                "API definitions exceed the admitted environment byte limit".into(),
            ));
        }
        let mut analysis = EmmyLuaAnalysis::new();
        let mut config = Emmyrc::default();
        config.runtime.version = EmmyrcLuaVersion::Lua51;
        analysis.update_config(Arc::new(config));
        let root = PathBuf::from("C:/lycheedev/authenticated-environment-types");
        analysis.add_main_workspace(root.clone());
        let ids = analysis
            .update_files_by_path_shared_checked(
                vec![(root.join("definitions.lua"), Some(definitions.clone()))],
                &mut check,
            )
            .map_err(DeclarationFailure::Interrupted)?;
        check().map_err(DeclarationFailure::Interrupted)?;
        let [file_id] = ids.as_slice() else {
            return Err(DeclarationFailure::InvalidInput(
                "API semantic input missing or duplicated".into(),
            ));
        };
        if analysis.db.get_file_text(*file_id) != Some(definitions.as_ref()) {
            return Err(DeclarationFailure::InvalidInput(
                "API semantic input differs from verified definitions".into(),
            ));
        }
        let errors = analysis
            .semantic_model(*file_id)
            .parse_errors()
            .unwrap_or_default();
        if let Some(first) = errors.first() {
            return Err(DeclarationFailure::Parse(ParseIssues {
                count: errors.len(),
                first: ParseIssue {
                    kind: format!("{:?}", first.kind),
                    range: ByteRange {
                        start: first.range.start().into(),
                        end: first.range.end().into(),
                    },
                },
            }));
        }
        Ok(Self {
            analysis,
            file_id: *file_id,
            bytes: definitions.len(),
        })
    }

    /// Query only the selected declaration. The supplied check must retain the
    /// caller's original deadline/cancellation lineage; `step` retains the
    /// caller's existing traversal budget and error type.
    #[allow(clippy::too_many_arguments)]
    pub fn type_at<E: 'static>(
        &self,
        target: ByteRange,
        query_identity: &str,
        callable: CallableInput<'_>,
        check: Arc<dyn Fn() -> Result<(), E> + Send + Sync>,
        error_code: fn(&E) -> String,
        step: impl FnMut() -> Result<(), E>,
    ) -> Result<Option<TypeProjection>, DeclarationFailure<E>> {
        self.type_at_with_declarations(
            target,
            query_identity,
            callable,
            check,
            error_code,
            step,
            |_| Ok(DeclarationLookup::Missing),
        )
    }

    /// The resolver must select from the same admitted catalog as `callable`.
    /// It is invoked at most once per name in this query, never retained or used
    /// across sessions. Projection work consumes the original check/step budget.
    #[allow(clippy::too_many_arguments)]
    pub fn type_at_with_declarations<E: 'static>(
        &self,
        target: ByteRange,
        query_identity: &str,
        callable: CallableInput<'_>,
        check: Arc<dyn Fn() -> Result<(), E> + Send + Sync>,
        error_code: fn(&E) -> String,
        mut step: impl FnMut() -> Result<(), E>,
        mut resolve: impl FnMut(&str) -> Result<DeclarationLookup, E>,
    ) -> Result<Option<TypeProjection>, DeclarationFailure<E>> {
        check().map_err(DeclarationFailure::Interrupted)?;
        if target.start >= target.end
            || target.end as usize > self.bytes
            || query_identity.is_empty()
        {
            return Err(DeclarationFailure::InvalidInput(
                "API Type coordinate or query identity is invalid".into(),
            ));
        }
        let model = self.analysis.semantic_model(self.file_id);
        let root = model
            .syntax_tree()
            .ok_or_else(|| DeclarationFailure::InvalidInput("API semantic tree missing".into()))?
            .get_red_root();
        let mut selected = None;
        for node in root.descendants() {
            check().map_err(DeclarationFailure::Interrupted)?;
            step().map_err(DeclarationFailure::Interrupted)?;
            if let Some(expr) = LuaExpr::cast(node) {
                let range = expr.get_range();
                let start: u32 = range.start().into();
                let end: u32 = range.end().into();
                if start <= target.start
                    && end >= target.end
                    && selected
                        .as_ref()
                        .is_none_or(|(length, _)| end - start < *length)
                {
                    selected = Some((end - start, expr.get_syntax_id()));
                }
            }
        }
        check().map_err(DeclarationFailure::Interrupted)?;
        let Some((_, syntax)) = selected else {
            return Ok(None);
        };
        let site = BindingSite {
            file_id: self.file_id,
            syntax,
        };
        let projection =
            crate::projection::project(&callable, check.as_ref(), &mut step, &mut resolve)?;
        let session = BindingSession::new(Arc::new(DeclarationContext {
            identity: query_identity.into(),
            site,
            signature: projection.signature,
            check: check.clone(),
            error_code,
        }));
        let model = self
            .analysis
            .semantic_model_with_context(self.file_id, session.clone());
        step().map_err(DeclarationFailure::Interrupted)?;
        let inferred = model.type_of_expr(syntax);
        check().map_err(DeclarationFailure::Interrupted)?;
        let coverage = session.coverage();
        let known = matches!(&inferred, LuaType::DocFunction(_))
            && coverage
                .iter()
                .any(|row| row.site == site && row.state == "bound");
        let rendered = humanize_semantic_type_detailed(&model, &inferred);
        check().map_err(DeclarationFailure::Interrupted)?;
        Ok(Some(TypeProjection {
            declarations: projection.declarations,
            known,
            rendered,
            coverage: coverage
                .into_iter()
                .map(|row| CoverageRow {
                    operation: row.operation.into(),
                    state: row.state.into(),
                    proof: row.proof,
                    reason: row.reason,
                })
                .collect(),
        }))
    }
}

struct DeclarationContext<E> {
    identity: String,
    site: BindingSite,
    signature: Result<Arc<LuaFunctionType>, &'static str>,
    check: Arc<dyn Fn() -> Result<(), E> + Send + Sync>,
    error_code: fn(&E) -> String,
}

impl<E> BindingContext for DeclarationContext<E> {
    fn identity(&self) -> &str {
        &self.identity
    }

    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        if let Err(error) = (self.check)() {
            return BindingDecision::Unavailable {
                proof: self.identity.clone(),
                reason: (self.error_code)(&error),
            };
        }
        if site != self.site {
            return BindingDecision::Unavailable {
                proof: self.identity.clone(),
                reason: "outside_authenticated_api_type_target".into(),
            };
        }
        match &self.signature {
            Ok(signature) => BindingDecision::Bound(BoundValue {
                identity: self.identity.clone(),
                proof: self.identity.clone(),
                value: SelectedValue::CallableSignature(signature.clone()),
            }),
            Err(reason) => BindingDecision::Unavailable {
                proof: self.identity.clone(),
                reason: (*reason).into(),
            },
        }
    }
}
