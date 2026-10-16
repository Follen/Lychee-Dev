//! Raw shell producers used at the fixed engine's deliberately non-contextual
//! boundaries. Named identities remain symbolic until the original projection
//! point; selected inference is never substituted for this separate producer.
mod doc;
mod domain;
mod expression;
use self::domain::*;
use super::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    Decl, DeclKind, DocGenericParam, FileId, LuaType, ModuleExport, SemanticId, TypeDefKind,
    TypeScope,
};
use emmylua_parser::{
    LuaAstNode, LuaCallExpr, LuaClosureExpr, LuaDocType, LuaExpr, LuaForRangeStat, LuaIndexExpr,
    LuaLiteralToken, LuaReturnStat, LuaSyntaxId, NumberResult,
};
use smol_str::SmolStr;
use std::collections::HashSet;

#[derive(Default)]
struct ShellState {
    expressions: HashSet<(FileId, LuaSyntaxId)>,
    iterators: HashSet<(FileId, SemanticId)>,
}

impl<E> OwnedQuery<'_, '_, E> {
    /// Exact public-facade decl_type_lua producer: no flow, alias expansion,
    /// selected overlay, or signature-generic context is added here.
    pub(super) fn raw_declaration_type(
        &mut self,
        file: FileId,
        declaration: &SemanticId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        if self.model.file_facts_of(file).is_none() {
            return Ok(None);
        }
        let shell = self.shell_decl(file, declaration, &mut ShellState::default())?;
        Ok(Some(OwnedValue::plain(self.project_owned_shell(
            file,
            &shell,
            &[],
        )?)))
    }
    pub(super) fn raw_signature_return(
        &mut self,
        file: FileId,
        closure: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let generic_names = facts
            .signature_by_closure(closure)
            .and_then(|sig| sig.docs.as_ref())
            .map(|docs| {
                docs.generic_params
                    .iter()
                    .map(|g| g.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let shell = self.shell_signature_return(file, closure, &mut ShellState::default())?;
        Ok(Some(OwnedValue::plain(self.project_owned_shell(
            file,
            &shell,
            &generic_names,
        )?)))
    }
    fn shell_signature_return(
        &mut self,
        file: FileId,
        closure: LuaSyntaxId,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        let mut shell = TypeShell::unknown();
        for slot in self.shell_signature_returns(file, closure, state)? {
            self.checkpoint()?;
            shell.merge(&slot);
        }
        Ok(shell)
    }
    fn shell_signature_returns(
        &mut self,
        file: FileId,
        closure_syntax: LuaSyntaxId,
        state: &mut ShellState,
    ) -> QueryResult<Vec<TypeShell>, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(Vec::new());
        };
        let Some(sig) = facts.signature_by_closure(closure_syntax) else {
            return Ok(Vec::new());
        };
        if let Some(docs) = &sig.docs
            && !docs.returns.is_empty()
        {
            let mut slots = Vec::new();
            for syntax in &docs.returns {
                slots.push(self.shell_doc(file, *syntax, &docs.generic_params)?);
            }
            if slots.iter().any(|s| !s.is_unknown()) {
                return Ok(slots);
            }
        }
        if let Some(docs) = &sig.docs
            && !docs.return_overload_rows.is_empty()
        {
            let mut rows = Vec::new();
            let mut index = 0;
            for len in &docs.return_overload_rows {
                self.checkpoint()?;
                let end = (index + len).min(docs.return_overloads.len());
                rows.push(
                    docs.return_overloads[index..end]
                        .iter()
                        .map(|(_, s)| *s)
                        .collect::<Vec<_>>(),
                );
                index = end;
            }
            let max = rows.iter().map(Vec::len).max().unwrap_or(0);
            let mut slots = Vec::new();
            for slot in 0..max {
                let mut shell = TypeShell::unknown();
                for row in &rows {
                    self.checkpoint()?;
                    if let Some(syntax) = row.get(slot) {
                        shell.merge(&self.shell_doc(file, *syntax, &docs.generic_params)?);
                    } else {
                        shell.merge(&TypeShell::from_primitive(PrimitiveType::Nil));
                    }
                }
                slots.push(shell);
            }
            if slots.iter().any(|s| !s.is_unknown()) {
                return Ok(slots);
            }
        }
        let closure = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| closure_syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaClosureExpr::cast);
        let Some(closure) = closure else {
            return Ok(Vec::new());
        };
        let multi_path = closure.descendants::<LuaReturnStat>().count() > 1;
        let mut slots: Vec<TypeShell> = Vec::new();
        for row in closure.descendants::<LuaReturnStat>() {
            for (index, expr) in row.get_expr_list().enumerate() {
                self.checkpoint()?;
                let special = match &expr {
                    LuaExpr::NameExpr(name) if name.get_name_text().as_deref() == Some("self") => {
                        shell_method_self(facts, closure_syntax)
                    }
                    LuaExpr::LiteralExpr(literal) if multi_path => match literal.get_literal() {
                        Some(LuaLiteralToken::Number(n))
                            if matches!(
                                n.get_number_value(),
                                NumberResult::Int(_) | NumberResult::Uint(_)
                            ) =>
                        {
                            Some(TypeShell::from_primitive(PrimitiveType::Integer))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let shell = match special {
                    Some(s) => s,
                    None => self.shell_expression(file, expr.get_syntax_id(), state)?,
                };
                if let Some(slot) = slots.get_mut(index) {
                    slot.merge(&shell);
                } else {
                    slots.push(shell);
                }
            }
        }
        if (slots.is_empty() || slots.iter().all(TypeShell::is_unknown))
            && let Some(expected) = self.shell_expected_returns(file, closure_syntax, state)?
        {
            return Ok(expected);
        }
        Ok(slots)
    }
    fn shell_expected_returns(
        &mut self,
        file: FileId,
        closure: LuaSyntaxId,
        state: &mut ShellState,
    ) -> QueryResult<Option<Vec<TypeShell>>, E> {
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let Some(member) = facts
            .members
            .iter()
            .find(|m| m.value_syntax == Some(closure))
        else {
            return Ok(None);
        };
        for field in facts.members.iter().filter(|f| {
            f.key == member.key
                && f.owner != member.owner
                && matches!(f.owner, SemanticId::TypeDef(_))
        }) {
            let shell = self.shell_member(file, &field.id, state)?;
            for candidate in shell.candidates {
                self.checkpoint()?;
                if let TypeCandidate::Function(fun) = candidate {
                    return Ok(Some(if fun.returns_multi.len() > 1 {
                        fun.returns_multi.clone()
                    } else {
                        vec![fun.returns.clone()]
                    }));
                }
            }
        }
        Ok(None)
    }
    fn shell_decl(
        &mut self,
        file: FileId,
        id: &SemanticId,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(TypeShell::unknown());
        };
        let Some(decl) = facts.decl_by_id(id) else {
            return Ok(TypeShell::unknown());
        };
        if let Some(syntax) = decl.doc_type_syntax {
            let shell = self.shell_doc(file, syntax, &[])?;
            if !shell.is_unknown() {
                return Ok(shell);
            }
        }
        if let Some(module) = &decl.module_path
            && let Some(file) = self.model.module_file_of(module)
        {
            return Ok(TypeShell::from_module_ref(file));
        }
        if matches!(decl.kind, DeclKind::Param)
            && let Some((sig, _)) = facts.signature_and_param_index_of_decl(decl)
            && let Some(docs) = &sig.docs
            && let Some((_, syntax)) = docs.param_types.iter().find(|(name, _)| name == &decl.name)
        {
            let mut shell = self.shell_doc(file, *syntax, &docs.generic_params)?;
            if !shell.is_unknown() {
                if docs.nullable_params.iter().any(|n| n == &decl.name) {
                    shell.merge(&TypeShell::from_primitive(PrimitiveType::Nil));
                }
                return Ok(shell);
            }
        }
        if matches!(decl.kind, DeclKind::Local { is_iter: true, .. })
            && let Some(shell) = self.shell_iter_slot(file, decl, state)?
            && !shell.is_unknown()
        {
            return Ok(shell);
        }
        if let Some(syntax) = decl.value_expr_syntax {
            let shell = self.shell_expression(file, syntax, state)?;
            if !shell.is_unknown() {
                if matches!(decl.kind, DeclKind::Global) {
                    let names: HashSet<&str> = facts
                        .signatures
                        .iter()
                        .filter_map(|s| s.docs.as_ref())
                        .flat_map(|d| d.generic_params.iter().map(|g| g.name.as_str()))
                        .collect();
                    if shell.candidates.iter().any(
                        |c| matches!(c,TypeCandidate::Generic(n) if names.contains(n.as_str())),
                    ) {
                        return Ok(TypeShell::unknown());
                    }
                }
                return Ok(shell);
            }
        }
        Ok(TypeShell::unknown())
    }
    fn shell_member(
        &mut self,
        file: FileId,
        id: &SemanticId,
        state: &mut ShellState,
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(TypeShell::unknown());
        };
        let Some(member) = facts.member_by_id(id) else {
            return Ok(TypeShell::unknown());
        };
        if let Some(module) = &member.module_path
            && let Some(file) = self.model.module_file_of(module)
        {
            return Ok(TypeShell::from_module_ref(file));
        }
        if let Some(syntax) = member.value_syntax {
            let shell = if matches!(member.owner, SemanticId::TypeDef(_)) {
                let generics = facts
                    .type_def_by_id(&member.owner)
                    .map(|d| d.generic_params.as_slice())
                    .unwrap_or(&[]);
                self.shell_doc(file, syntax, generics)?
            } else {
                self.shell_expression(file, syntax, state)?
            };
            if !shell.is_unknown() {
                return Ok(shell);
            }
        }
        Ok(if member.is_method {
            TypeShell::from_primitive(PrimitiveType::Function)
        } else {
            TypeShell::unknown()
        })
    }
    fn shell_module(&mut self, file: FileId, state: &mut ShellState) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(TypeShell::unknown());
        };
        match &facts.module_export {
            ModuleExport::Decl { decl, name } => {
                let mut shell = self.shell_decl(file, decl, state)?;
                shell.merge(&TypeShell::from_name(name));
                Ok(shell)
            }
            ModuleExport::Global { name } => {
                if let Some(decl) = self.model.global_decl(name) {
                    if let SemanticId::Decl(key) = &decl {
                        let shell = self.shell_decl(key.file_id, &decl, state)?;
                        if !shell.is_unknown() {
                            return Ok(shell);
                        }
                    }
                }
                Ok(TypeShell::from_name(name))
            }
            ModuleExport::Expr { value_syntax } => {
                // module_export_type calls expr_type directly, without the
                // expr_type_of in-progress boundary.
                let expr = self
                    .model
                    .syntax_tree_of(file)
                    .and_then(|tree| value_syntax.to_node_from_root(&tree.get_red_root()))
                    .and_then(LuaExpr::cast);
                match expr {
                    Some(expr) => self.shell_expr(file, expr, state),
                    None => Ok(TypeShell::unknown()),
                }
            }
            ModuleExport::None => Ok(TypeShell::unknown()),
        }
    }
    fn shell_member_via_owner(
        &mut self,
        owner: &SemanticId,
        name: &str,
        methods_only: bool,
        state: &mut ShellState,
    ) -> QueryResult<Option<TypeShell>, E> {
        let order = self.owner_contribution_order()?;
        for owner in self.owned_resolve_owner_ids(owner, &order)? {
            for member in self.model.members_of_owner(&owner).iter() {
                self.checkpoint()?;
                if member.name != name {
                    continue;
                }
                let Some(facts) = self.model.file_facts_of(member.file_id) else {
                    continue;
                };
                if methods_only && !facts.member_by_id(&member.id).is_some_and(|m| m.is_method) {
                    continue;
                }
                let shell = self.shell_member(member.file_id, &member.id, state)?;
                if !shell.is_unknown() {
                    return Ok(Some(shell));
                }
            }
        }
        Ok(None)
    }
    fn shell_type_member(
        &mut self,
        file: FileId,
        id: &SemanticId,
        name: &str,
        visited: &mut Vec<SemanticId>,
    ) -> QueryResult<Option<TypeShell>, E> {
        self.checkpoint()?;
        if visited.contains(id) {
            return Ok(None);
        }
        visited.push(id.clone());
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        if let Some(member) = facts.field_members_of_type(id, name)
            && let Some(syntax) = member.value_syntax
        {
            let generics = facts
                .type_def_by_id(id)
                .map(|d| d.generic_params.as_slice())
                .unwrap_or(&[]);
            let shell = self.shell_doc(file, syntax, generics)?;
            if !shell.is_unknown() {
                return Ok(Some(shell));
            }
        }
        if let Some(def) = facts.type_def_by_id(id) {
            for name_super in &def.super_names {
                if let Some(def) = facts.type_def_by_full_name(name_super) {
                    if let Some(shell) = self.shell_type_member(file, &def.id, name, visited)? {
                        return Ok(Some(shell));
                    }
                }
            }
        }
        Ok(None)
    }
    fn shell_iter_slot(
        &mut self,
        file: FileId,
        decl: &Decl,
        state: &mut ShellState,
    ) -> QueryResult<Option<TypeShell>, E> {
        let key = (file, decl.id.clone());
        if !state.iterators.insert(key.clone()) {
            return Ok(None);
        }
        let result = self.shell_iter_slot_body(file, decl, state);
        state.iterators.remove(&key);
        result
    }
    fn shell_iter_slot_body(
        &mut self,
        file: FileId,
        decl: &Decl,
        state: &mut ShellState,
    ) -> QueryResult<Option<TypeShell>, E> {
        self.checkpoint()?;
        let stat = decl
            .owner_syntax
            .and_then(|syntax| {
                self.model
                    .syntax_tree_of(file)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            })
            .and_then(LuaForRangeStat::cast);
        let Some(stat) = stat else { return Ok(None) };
        let Some(index) = stat
            .get_var_name_list()
            .position(|v| v.get_name_text() == decl.name.as_str())
        else {
            return Ok(None);
        };
        let Some(LuaExpr::CallExpr(call)) = stat.get_expr_list().next() else {
            return Ok(None);
        };
        let name = match call.get_prefix_expr() {
            Some(LuaExpr::NameExpr(name)) => match name.get_name_text().as_deref() {
                Some("pairs" | "next") => "__pairs",
                Some("ipairs") => "__ipairs",
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let Some(arg) = call.get_args_list().and_then(|list| list.get_args().next()) else {
            return Ok(None);
        };
        let shell = self.shell_expr(file, arg, state)?;
        for candidate in &shell.candidates {
            self.checkpoint()?;
            let (type_name, args) = match candidate {
                TypeCandidate::Named(n) => (n.as_str(), Vec::new()),
                TypeCandidate::GenericInstance(i) => (i.name.as_str(), i.args.clone()),
                _ => continue,
            };
            let Some(def) = self.model.resolve_type_def_in(file, type_name) else {
                return Ok(None);
            };
            let mut members = self.model.members_of_owner(&def.id).to_vec();
            let order = self.owner_contribution_order()?;
            for owner in self.owned_resolve_owner_ids(&def.id, &order)? {
                members.extend(self.model.members_of_owner(&owner).iter().cloned());
            }
            for reference in members.iter().filter(|m| m.name == name) {
                self.checkpoint()?;
                let Some(facts) = self.model.file_facts_of(reference.file_id) else {
                    continue;
                };
                let Some(member) = facts.member_by_id(&reference.id) else {
                    continue;
                };
                let Some(closure) = member.value_syntax else {
                    continue;
                };
                let Some(docs) = facts
                    .signature_by_closure(closure)
                    .and_then(|s| s.docs.as_ref())
                else {
                    continue;
                };
                let Some(syntax) = docs.returns.first() else {
                    continue;
                };
                let node = self
                    .model
                    .syntax_tree_of(reference.file_id)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    .and_then(LuaDocType::cast);
                let Some(LuaDocType::Func(fun)) = node else {
                    continue;
                };
                let Some(returns) = fun.get_return_type_list() else {
                    continue;
                };
                let mut slots = Vec::new();
                for ret in returns.get_return_type_list() {
                    if let (_, Some(ty)) = ret.get_name_and_type() {
                        slots.push(self.shell_doc_node(
                            reference.file_id,
                            &ty,
                            &def.generic_params,
                        )?);
                    }
                }
                if let Some(slot) = slots.get(index) {
                    let shell = substitute_shell(slot, &def.generic_params, &args);
                    if !shell.is_unknown() {
                        return Ok(Some(shell));
                    }
                }
            }
        }
        Ok(None)
    }
}
fn shell_method_self(
    facts: &emmylua_code_analysis::FileFacts,
    closure: LuaSyntaxId,
) -> Option<TypeShell> {
    let member = facts
        .member_by_value_syntax(closure)
        .filter(|m| m.is_method)?;
    let def = match &member.owner {
        SemanticId::TypeDef(_) => facts.type_def_by_id(&member.owner),
        SemanticId::Decl(_) => facts
            .decl_by_id(&member.owner)
            .and_then(|d| d.owner_syntax)
            .and_then(|s| facts.type_def_by_owner_syntax(s)),
        _ => None,
    };
    if let Some(def) = def {
        return Some(TypeShell::from_name(&def.full_name));
    }
    let SemanticId::Decl(_) = &member.owner else {
        return None;
    };
    let syntax = facts.decl_by_id(&member.owner)?.value_expr_syntax?;
    Some(TypeShell::from_table(TableId::from_range(
        facts.file_id,
        syntax.get_range(),
    )))
}
