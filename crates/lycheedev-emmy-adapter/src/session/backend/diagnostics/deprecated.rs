//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::DiagnosticCode;
use emmylua_parser::{LuaAst, LuaAstNode};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_deprecated(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let names = self.workspace_deprecated_names()?;
        if let Some(facts) = self.model.file_facts() {
            for use_ in &facts.name_uses {
                self.checkpoint()?;
                let range = use_.syntax.get_range();
                let local = facts.name_use_at_offset(range.start()).and_then(|use_| {
                    facts.find_visible_decl_before_offset(&use_.name, range.start())
                });
                if let Some(decl) = local {
                    if decl.deprecated && range != decl.name_range {
                        report_deprecated(context, range, &decl.name);
                    } else if !decl.deprecated
                        && self
                            .model
                            .resolve_type_def(&decl.name)
                            .is_some_and(|def| def.deprecated)
                    {
                        report_deprecated(context, range, &decl.name);
                    }
                    continue;
                }
                if names.global(use_.name.as_str()) {
                    report_deprecated(context, range, &use_.name);
                }
            }
        }
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for node in tree.get_red_root().descendants().filter_map(LuaAst::cast) {
            self.checkpoint()?;
            match node {
                LuaAst::LuaIndexExpr(index) => {
                    let Some(name) = index.get_index_key().map(|key| key.get_path_part()) else {
                        continue;
                    };
                    if !names.member(name.as_str()) {
                        continue;
                    }
                    let Some(resolved) = self.resolve_owned_member(&index)? else {
                        continue;
                    };
                    let range = index
                        .get_index_key()
                        .and_then(|key| key.get_range())
                        .unwrap_or_else(|| index.get_range());
                    let Some(id) = resolved.member_id else {
                        continue;
                    };
                    let file = resolved.file_id.unwrap_or(self.model.file_id());
                    let Some(member) = self
                        .model
                        .file_facts_of(file)
                        .and_then(|facts| facts.member_by_id(&id))
                    else {
                        continue;
                    };
                    if member.deprecated && id.member_key_range() != Some(range) {
                        report_deprecated(context, range, &resolved.name);
                    }
                }
                LuaAst::LuaDocNameType(name_type) => {
                    let Some(name) = name_type.get_name_text() else {
                        continue;
                    };
                    let Some(facts) = self.model.file_facts() else {
                        continue;
                    };
                    if let Some(def) = facts
                        .type_def_by_name(name.as_str())
                        .or_else(|| facts.type_def_by_full_name(name.as_str()))
                        && def.deprecated
                    {
                        report_deprecated(context, name_type.get_range(), &def.name);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
fn report_deprecated(context: &mut CheckContext<'_>, range: rowan::TextRange, name: &str) {
    context.add_diagnostic(
        DiagnosticCode::Deprecated,
        range,
        t!("`%{name}` is deprecated", name = name),
    );
}
