//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::humanize_semantic_type as humanize_type;
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{LuaAstNode, LuaDocTagCast};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_cast_type_mismatch(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for tag in tree
            .get_red_root()
            .descendants()
            .filter_map(LuaDocTagCast::cast)
        {
            self.checkpoint()?;
            let Some(key) = tag.get_key_expr() else {
                continue;
            };
            let origin = self.type_of_expr(key.get_syntax_id())?;
            let check_origin = cast_origin_constraint(&self.model, origin.structural_type())
                .map(OwnedValue::plain)
                .unwrap_or_else(|| origin.clone());
            for op_type in tag.get_op_types() {
                if op_type.get_op().is_some() {
                    continue;
                }
                let Some(doc) = op_type.get_type() else {
                    continue;
                };
                let target = OwnedValue::plain(self.model.doc_type_lua(doc.get_syntax_id()));
                if self.type_check(&check_origin, &target)?
                    || cast_broad_compatible(
                        check_origin.structural_type(),
                        target.structural_type(),
                    )
                {
                    continue;
                }
                context.add_diagnostic(
                    DiagnosticCode::CastTypeMismatch,
                    op_type.get_range(),
                    t!(
                        "Cannot cast `%{original}` to `%{target}`. %{reason}",
                        original = humanize_type(&self.model, origin.structural_type()),
                        target = humanize_type(&self.model, target.structural_type()),
                        reason = ""
                    ),
                );
            }
        }
        Ok(())
    }
}
fn cast_origin_constraint(
    semantic_model: &emmylua_code_analysis::SemanticModel<'_>,
    ty: &LuaType,
) -> Option<LuaType> {
    match ty {
        LuaType::TplRef(tpl) => tpl.get_constraint().cloned(),
        LuaType::Ref(id) | LuaType::Def(id) => {
            // Resolved type names (Animal) are not generic parameters.
            if semantic_model.type_def_of(id).is_some() {
                return None;
            }
            let name = id.get_name();
            for (_signature, param) in semantic_model.file_facts()?.generic_param_constraints(name)
            {
                let Some(constraint) = param.constraint else {
                    continue;
                };
                let constraint_ty = semantic_model.doc_type_lua(constraint);
                if !matches!(constraint_ty, LuaType::Unknown) {
                    return Some(constraint_ty);
                }
            }
            None
        }
        _ => None,
    }
}

fn cast_broad_compatible(origin: &LuaType, target: &LuaType) -> bool {
    fn strip_union(ty: &LuaType) -> Option<LuaType> {
        match ty {
            LuaType::Union(union) => {
                let types = union.into_vec();
                types.iter().find(|ty| !matches!(ty, LuaType::Nil)).cloned()
            }
            other => Some(other.clone()),
        }
    }
    matches!(
        (strip_union(origin), strip_union(target)),
        (Some(LuaType::Table), Some(LuaType::Tuple(_)))
            | (Some(LuaType::Tuple(_)), Some(LuaType::Table))
    )
}
