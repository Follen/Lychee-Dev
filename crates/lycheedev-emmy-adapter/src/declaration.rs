use crate::{
    ByteRange, CallableInput, CoverageRow, DeclarationFailure, DeclarationLookup, ParseIssue,
    ParseIssues, TypeProjection,
};
use emmylua_code_analysis::{
    BatchChange, EmmyLuaAnalysis, Emmyrc, EmmyrcLuaVersion, FileId, LuaType, file_path_to_uri,
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
        // The synthetic workspace exists only in Emmy's in-memory VFS. Its
        // file URI still requires an absolute path on the current host.
        let root = PathBuf::from(if cfg!(windows) {
            "C:/lycheedev/authenticated-environment-types"
        } else {
            "/lycheedev/authenticated-environment-types"
        });
        analysis.add_main_workspace(root.clone());
        // Public upstream ingestion accepts owned String input. Copy only this
        // separately bounded declaration document; Source workspace ingestion
        // has a different sharing contract and must not be routed through here.
        let text = copy_definitions(&definitions, &mut check)?;
        let uri = file_path_to_uri(&root.join("definitions.lua")).ok_or_else(|| {
            DeclarationFailure::InvalidInput("API semantic input path is invalid".into())
        })?;
        check().map_err(DeclarationFailure::Interrupted)?;
        // Use the public batch entry even for one document. It preserves the
        // previous full-cache-rebuild semantics, unlike the incremental entry.
        // Parsing and indexing are one synchronous upstream call. This callback
        // cannot interrupt that call; the existing owned worker hard deadline
        // remains required. On a stop observed afterwards, drop this whole model.
        let (_, ids) = analysis.db.apply_batch_with_ids(BatchChange {
            files: vec![(uri, Some(text))],
        });
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
        let Some(_) = selected else {
            return Ok(None);
        };
        let projection =
            crate::projection::project(&callable, check.as_ref(), &mut step, &mut resolve)?;
        step().map_err(DeclarationFailure::Interrupted)?;
        // Keep the former selected-value checkpoint and its unavailable reason.
        // A persistent interruption is returned with the original payload below;
        // a transient failed check remains unavailable, never raw-source inference.
        let (inferred, state, reason) = match check() {
            Err(error) => (LuaType::Unknown, "unavailable", Some(error_code(&error))),
            Ok(()) => match projection.signature {
                Ok(signature) => (LuaType::DocFunction(signature), "bound", None),
                Err(reason) => (LuaType::Unknown, "unavailable", Some(reason.into())),
            },
        };
        check().map_err(DeclarationFailure::Interrupted)?;
        let known = matches!(&inferred, LuaType::DocFunction(_));
        let rendered = humanize_semantic_type_detailed(&model, &inferred);
        check().map_err(DeclarationFailure::Interrupted)?;
        Ok(Some(TypeProjection {
            declarations: projection.declarations,
            known,
            rendered,
            // This is owned declaration-resolution evidence. No engine checker
            // or source-expression inference ran to render this signature.
            coverage: vec![CoverageRow {
                operation: "resolve".into(),
                state: state.into(),
                proof: query_identity.into(),
                reason,
            }],
        }))
    }
}

/// Bound cooperative work while adapting the immutable admitted bytes to the
/// public upstream String API. Chunks end on UTF-8 boundaries; input is neither
/// normalized nor executed. No callback or partly built model escapes failure.
fn copy_definitions<E>(
    definitions: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<String, DeclarationFailure<E>> {
    check().map_err(DeclarationFailure::Interrupted)?;
    let mut text = String::with_capacity(definitions.len());
    let mut start = 0;
    while start < definitions.len() {
        check().map_err(DeclarationFailure::Interrupted)?;
        let mut end = (start + 16_384).min(definitions.len());
        while !definitions.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&definitions[start..end]);
        start = end;
    }
    check().map_err(DeclarationFailure::Interrupted)?;
    Ok(text)
}
