//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/code_style/preferred_local_alias.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
use super::CheckContext;
use crate::session::SelectedProjectionBackend;
use crate::session::backend::{OwnedQuery, QueryResult};
use emmylua_code_analysis::{DiagnosticCode, LuaType, SemanticId, SemanticModel};
use emmylua_parser::{
    LuaAssignStat, LuaAst, LuaAstNode, LuaExpr, LuaIndexExpr, LuaLocalStat, LuaSyntaxKind,
    LuaVarExpr, PathTrait,
};
use rowan::TextRange;
use std::collections::{HashMap, HashSet};

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_preferred_local_alias(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let mut aliases = LocalAliasSet::new();
        let chunk = tree.get_chunk_node();
        for walk in chunk.walk_descendants::<LuaAst>() {
            self.checkpoint()?;
            match walk {
                rowan::WalkEvent::Enter(node) => {
                    if is_scope(&node) {
                        aliases.push();
                    }
                    match node {
                        LuaAst::LuaLocalStat(stat) => self.alias_collect(&mut aliases, &stat)?,
                        LuaAst::LuaAssignStat(stat) => {
                            self.alias_invalidate(&mut aliases, &stat)?
                        }
                        LuaAst::LuaIndexExpr(index) => {
                            self.alias_preference(context, &aliases, &index)?
                        }
                        _ => {}
                    }
                }
                rowan::WalkEvent::Leave(node) => {
                    if is_scope(&node) {
                        aliases.pop();
                    }
                }
            }
        }
        Ok(())
    }
    fn alias_collect(
        &mut self,
        aliases: &mut LocalAliasSet,
        stat: &LuaLocalStat,
    ) -> QueryResult<(), E> {
        let names = stat.get_local_name_list().collect::<Vec<_>>();
        let values = stat.get_value_exprs().collect::<Vec<_>>();
        for i in 0..names.len().min(values.len()) {
            let name = &names[i];
            let value = &values[i];
            let LuaExpr::IndexExpr(index) = value else {
                continue;
            };
            if !is_only_dot_index_expr(value).unwrap_or(false) {
                continue;
            }
            let Some(LuaExpr::NameExpr(_)) = index.get_prefix_expr() else {
                continue;
            };
            let Some(path) = index.get_access_path() else {
                continue;
            };
            let Some(ref_var) = find_ref_var_decl_id(&self.model, value) else {
                continue;
            };
            let Some(resolved) = self.resolve_owned_member(index)? else {
                continue;
            };
            let ref_field = match resolved.member_id {
                Some(id) => Some(id),
                None => self.alias_runtime_member(index, &resolved.name)?,
            };
            let Some(token) = name.get_name_token() else {
                continue;
            };
            aliases.insert(path, token.get_name_text().to_string(), ref_field, ref_var);
            aliases.add_disable_check(value.get_range());
        }
        Ok(())
    }
    fn alias_runtime_member(
        &mut self,
        index: &LuaIndexExpr,
        name: &str,
    ) -> QueryResult<Option<SemanticId>, E> {
        let Some(prefix) = index.get_prefix_expr() else {
            return Ok(None);
        };
        let ty = self.type_of_expr(prefix.get_syntax_id())?;
        let LuaType::TableConst(table) = ty.structural_type() else {
            return Ok(None);
        };
        Ok(self.model.file_facts_of(table.file_id).and_then(|facts| {
            facts
                .members
                .iter()
                .find(|member| member.value_syntax.is_some() && member.key.name() == Some(name))
                .map(|member| member.id.clone())
        }))
    }
    fn alias_invalidate(
        &mut self,
        aliases: &mut LocalAliasSet,
        stat: &LuaAssignStat,
    ) -> QueryResult<(), E> {
        let (vars, _) = stat.get_var_and_expr_list();
        for var in vars {
            let LuaVarExpr::IndexExpr(index) = var else {
                continue;
            };
            let Some(resolved) = self.resolve_owned_member(&index)? else {
                continue;
            };
            if let Some(id) = resolved.member_id {
                aliases.remove_by_field(&id);
            }
        }
        Ok(())
    }
    fn alias_preference(
        &mut self,
        context: &mut CheckContext<'_>,
        aliases: &LocalAliasSet,
        index: &LuaIndexExpr,
    ) -> QueryResult<(), E> {
        if aliases.is_disable_check(&index.get_range()) {
            return Ok(());
        }
        let expr = LuaExpr::IndexExpr(index.clone());
        if !is_only_dot_index_expr(&expr).unwrap_or(false) {
            return Ok(());
        }
        if !matches!(index.get_prefix_expr(), Some(LuaExpr::NameExpr(_))) {
            return Ok(());
        }
        let Some(path) = index.get_access_path() else {
            return Ok(());
        };
        let Some(info) = aliases.get(&path) else {
            return Ok(());
        };
        let Some(var) = first_name_expr(index) else {
            return Ok(());
        };
        if !self.alias_root_references(&var, &info.ref_var)? {
            return Ok(());
        }
        if let Some(ref_field) = &info.ref_field {
            let Some(resolved) = self.resolve_owned_member(index)? else {
                return Ok(());
            };
            let check_field = match resolved.member_id {
                Some(id) => Some(id),
                None => self.alias_runtime_member(index, &resolved.name)?,
            };
            if check_field.as_ref() != Some(ref_field) {
                return Ok(());
            }
        }
        context.add_diagnostic(
            DiagnosticCode::PreferredLocalAlias,
            index.get_range(),
            t!(
                "Prefer use local alias variable `%{name}`",
                name = info.preferred_name
            ),
        );
        Ok(())
    }
    fn alias_root_references(
        &mut self,
        expr: &LuaExpr,
        expected: &SemanticId,
    ) -> QueryResult<bool, E> {
        // This is semantic_info's exact reachable producer order for a root
        // NameExpr inside IndexExpr. The assignment-RHS shortcut is unreachable:
        // the name's direct parent is the index, never LuaAssignStat.
        let Some(token) = expr.syntax().first_token() else {
            return Ok(false);
        };
        let offset = token.text_range().start();
        if let Some(id) = self
            .model
            .file_facts()
            .and_then(|facts| facts.decl_at_offset(offset))
            .map(|decl| decl.id.clone())
        {
            let _ = self.type_of_decl(&id)?;
            return Ok(&id == expected);
        }
        if let Some(id) = self
            .model
            .file_facts()
            .and_then(|facts| facts.member_at_offset(offset))
            .map(|member| member.id.clone())
        {
            let _ = self.type_of_member(&id)?;
            return Ok(&id == expected);
        }
        let LuaExpr::NameExpr(name) = expr else {
            return Ok(false);
        };
        let id = self.model.resolve_name(name.get_position());
        let _ = self.type_of_expr(expr.get_syntax_id())?;
        Ok(id.as_ref() == Some(expected))
    }
}

fn is_scope(node: &LuaAst) -> bool {
    matches!(
        node.syntax().kind().into(),
        LuaSyntaxKind::Chunk | LuaSyntaxKind::Block | LuaSyntaxKind::ClosureExpr
    )
}

fn find_ref_var_decl_id(semantic_model: &SemanticModel<'_>, expr: &LuaExpr) -> Option<SemanticId> {
    let mut prefix = expr.clone();
    while let LuaExpr::IndexExpr(index_expr) = prefix {
        match index_expr.get_prefix_expr() {
            Some(LuaExpr::NameExpr(name_expr)) => {
                return semantic_model.resolve_name(name_expr.get_position());
            }
            Some(LuaExpr::IndexExpr(prefix_index_expr)) => {
                prefix = LuaExpr::IndexExpr(prefix_index_expr);
            }
            _ => return None,
        }
    }
    None
}

fn is_only_dot_index_expr(expr: &LuaExpr) -> Option<bool> {
    let mut index_expr = match expr {
        LuaExpr::IndexExpr(index_expr) => index_expr.clone(),
        _ => return Some(false),
    };
    loop {
        let index_token = index_expr.get_index_token()?;
        if !index_token.is_dot() {
            return Some(false);
        }
        match index_expr.get_prefix_expr() {
            Some(LuaExpr::NameExpr(_)) => return Some(true),
            Some(LuaExpr::IndexExpr(prefix_index_expr)) => {
                index_expr = prefix_index_expr;
            }
            _ => return Some(false),
        }
    }
}

fn first_name_expr(index_expr: &LuaIndexExpr) -> Option<LuaExpr> {
    let mut index_expr = index_expr.clone();
    loop {
        match index_expr.get_prefix_expr() {
            Some(LuaExpr::NameExpr(name_expr)) => return Some(LuaExpr::NameExpr(name_expr)),
            Some(LuaExpr::IndexExpr(prefix_index_expr)) => {
                index_expr = prefix_index_expr;
            }
            _ => return None,
        }
    }
}

#[derive(Debug)]
struct LocalAliasSet {
    local_alias_stack: Vec<HashMap<String, LocalAliasInfo>>,
    disable_check: HashSet<TextRange>,
}

#[derive(Debug)]
struct LocalAliasInfo {
    ref_var: SemanticId,
    ref_field: Option<SemanticId>,
    preferred_name: String,
}

impl LocalAliasSet {
    fn new() -> Self {
        LocalAliasSet {
            local_alias_stack: vec![HashMap::new()],
            disable_check: HashSet::new(),
        }
    }

    fn push(&mut self) {
        self.local_alias_stack.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.local_alias_stack.pop();
    }

    fn insert(
        &mut self,
        access_path: String,
        preferred_name: String,
        ref_field: Option<SemanticId>,
        ref_var: SemanticId,
    ) {
        if let Some(map) = self.local_alias_stack.last_mut() {
            map.insert(
                access_path,
                LocalAliasInfo {
                    ref_var,
                    ref_field,
                    preferred_name,
                },
            );
        }
    }

    fn get(&self, access_path: &str) -> Option<&LocalAliasInfo> {
        for map in self.local_alias_stack.iter().rev() {
            if let Some(item) = map.get(access_path) {
                return Some(item);
            }
        }
        None
    }

    fn add_disable_check(&mut self, range: TextRange) {
        self.disable_check.insert(range);
    }

    fn remove_by_field(&mut self, field: &SemanticId) {
        for map in &mut self.local_alias_stack {
            map.retain(|_, info| info.ref_field.as_ref() != Some(field));
        }
    }

    fn is_disable_check(&self, range: &TextRange) -> bool {
        self.disable_check.contains(range)
    }
}
