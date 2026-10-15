//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_code_analysis::{SemanticId, TypeScope};
use emmylua_parser::{LuaAst, LuaAstNode, LuaAstToken, LuaExpr, VisibilityKind};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_access_invisible(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for node in tree.get_red_root().descendants().filter_map(LuaAst::cast) {
            self.checkpoint()?;
            match node {
                LuaAst::LuaNameExpr(name) => {
                    let Some(first) = name.syntax().first_token() else {
                        continue;
                    };
                    let offset = first.text_range().start();
                    let decl = self
                        .model
                        .file_facts()
                        .and_then(|facts| {
                            facts
                                .decl_at_offset(offset)
                                .map(|d| d.id.clone())
                                .or_else(|| facts.member_at_offset(offset).map(|m| m.id.clone()))
                        })
                        .or_else(|| self.model.resolve_name(offset));
                    let Some(decl) = decl else {
                        continue;
                    };
                    let Some(token) = name.get_name_token() else {
                        continue;
                    };
                    if !visible(&self.model, &decl) {
                        context.add_diagnostic(
                            DiagnosticCode::AccessInvisible,
                            token.get_range(),
                            t!("The property is not accessible from this scope."),
                        );
                    }
                }
                LuaAst::LuaIndexExpr(index) => {
                    let Some(resolved) = self.resolve_owned_member(&index)? else {
                        continue;
                    };
                    let Some(id) = resolved.member_id else {
                        continue;
                    };
                    let Some(token) = index.get_index_name_token() else {
                        continue;
                    };
                    let mut visibility_id = resolved
                        .file_id
                        .and_then(|file| self.model.file_facts_of(file))
                        .and_then(|facts| {
                            facts
                                .members
                                .iter()
                                .find(|m| {
                                    m.key.name() == Some(resolved.name.as_str())
                                        && m.visibility != VisibilityKind::Public
                                })
                                .map(|m| m.id.clone())
                        });
                    if visibility_id.is_none() {
                        if let Some(prefix) = index.get_prefix_expr() {
                            visibility_id =
                                self.visibility_module_private_member(&prefix, &resolved.name)?;
                        }
                    }
                    let visibility_id = visibility_id.unwrap_or(id);
                    if !visible(&self.model, &visibility_id) {
                        context.add_diagnostic(
                            DiagnosticCode::AccessInvisible,
                            token.text_range(),
                            t!("The property is not accessible from this scope."),
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn visibility_module_private_member(
        &mut self,
        prefix: &LuaExpr,
        name: &str,
    ) -> QueryResult<Option<SemanticId>, E> {
        let LuaExpr::NameExpr(expr) = prefix else {
            return Ok(None);
        };
        let Some(decl) = self.model.resolve_name(expr.get_position()) else {
            return Ok(None);
        };
        let Some(syntax) = self
            .model
            .file_facts()
            .and_then(|f| f.decl_by_id(&decl))
            .and_then(|d| d.value_expr_syntax)
        else {
            return Ok(None);
        };
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(None);
        };
        let Some(call) = syntax
            .to_node_from_root(&tree.get_red_root())
            .and_then(emmylua_parser::LuaCallExpr::cast)
        else {
            return Ok(None);
        };
        let Some(arg) = call.get_args_list().and_then(|a| a.get_args().next()) else {
            return Ok(None);
        };
        let ty = self.type_of_expr(arg.get_syntax_id())?;
        let module = match ty.structural_type() {
            LuaType::StringConst(s) | LuaType::DocStringConst(s) => s.to_string(),
            _ => return Ok(None),
        };
        let Some(file) = self.model.module_file_of(&module) else {
            return Ok(None);
        };
        Ok(self.model.file_facts_of(file).and_then(|f| {
            f.members
                .iter()
                .find(|m| m.key.name() == Some(name) && m.visibility != VisibilityKind::Public)
                .map(|m| m.id.clone())
        }))
    }
}
fn visible(model: &emmylua_code_analysis::SemanticModel<'_>, decl: &SemanticId) -> bool {
    let file = match decl {
        SemanticId::Decl(k) => k.file_id,
        SemanticId::Member(k) => k.file_id,
        SemanticId::TypeDef(k) => match k.scope {
            TypeScope::File(file) => file,
            _ => return true,
        },
        _ => return true,
    };
    if let SemanticId::Member(_) = decl
        && let Some(member) = model.file_facts_of(file).and_then(|f| f.member_by_id(decl))
    {
        return match member.visibility {
            VisibilityKind::Public | VisibilityKind::Internal => true,
            VisibilityKind::Package | VisibilityKind::Private | VisibilityKind::Protected => {
                file == model.file_id()
            }
        };
    }
    if matches!(decl, SemanticId::TypeDef(_)) {
        return file == model.file_id();
    }
    true
}
