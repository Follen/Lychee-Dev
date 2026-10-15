//! Resolved/runtime/declared member priorities on a flow-sensitive index read.
use super::*;
use emmylua_code_analysis::{LuaArrayLen, LuaSignatureId};
use emmylua_parser::{
    LuaClosureExpr, LuaForStat, LuaIndexExpr, LuaIndexKey, LuaUnaryExpr, UnaryOperator,
};
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn flow_index_expr(
        &mut self,
        index: &LuaIndexExpr,
        offset: u32,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let syntax = index.get_syntax_id();
        if let Some(resolved) = self.resolve_owned_member(index)?
            && let Some(member) = &resolved.member_id
        {
            let mut value = self.type_of_member_at(member, offset)?;
            if matches!(value.structural_type(), LuaType::Function)
                && let SemanticId::Member(key) = member
                && let Some(syntax) = self
                    .model
                    .file_facts_of(key.file_id)
                    .and_then(|facts| facts.member_by_id(member))
                    .and_then(|member| member.value_syntax)
                && let Some(closure) = self
                    .model
                    .syntax_tree_of(key.file_id)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    .and_then(LuaClosureExpr::cast)
            {
                value = OwnedValue::plain(LuaType::Signature(LuaSignatureId::from_closure(
                    key.file_id,
                    &closure,
                )));
            }
            let file = match member {
                SemanticId::Member(key) => key.file_id,
                _ => self.model.file_id(),
            };
            let runtime = self.model.file_facts_of(file).is_some_and(|facts| {
                facts
                    .members_of_owner(&resolved.owner)
                    .any(|candidate| &candidate.id == member)
            });
            if runtime
                && let Some(key) = self.flow_member_key(index)?
                && let Some(prefix @ LuaExpr::NameExpr(_)) = index.get_prefix_expr()
            {
                let prefix = self.type_of_expr_at(prefix.get_syntax_id(), offset)?;
                if matches!(
                    prefix.structural_type(),
                    LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_)
                ) && let Some(member) = self.member_type(&prefix, &key)?
                    && !member.structural_type().is_unknown()
                {
                    return self.apply_expr_casts(syntax, member);
                }
            }
            if runtime
                && let Some(prefix) = index.get_prefix_expr()
                && matches!(index.get_index_key(), Some(LuaIndexKey::Expr(_)))
                && let SemanticId::Member(key) = member
                && let Some(def) = self
                    .model
                    .file_facts_of(key.file_id)
                    .and_then(|facts| facts.member_by_id(member))
                && self.member_is_initializer(def)?
            {
                let prefix = self.type_of_expr_at(prefix.get_syntax_id(), offset)?;
                if let Some(syntax_value) = def.value_syntax {
                    let value = self.type_of_expr(syntax_value)?;
                    match value.structural_type() {
                        LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => {
                            return self
                                .apply_expr_casts(syntax, OwnedValue::plain(LuaType::Integer));
                        }
                        LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                            return self.apply_expr_casts(syntax, value);
                        }
                        _ => {}
                    }
                }
                if let Some(value) = self.member_type(&prefix, &def.key)? {
                    return self.apply_expr_casts(syntax, value);
                }
            }
            if let SemanticId::Decl(owner) = &resolved.owner
                && let Some(facts) = self.model.file_facts_of(owner.file_id)
                && !runtime
                && let Some(member) = facts
                    .members_of_owner(&resolved.owner)
                    .find(|member| member.key.name() == Some(resolved.name.as_str()))
                && !self.member_is_initializer(member)?
            {
                let runtime = self.type_of_member_at(&member.id, offset)?;
                if !runtime.structural_type().is_unknown() {
                    return self.apply_expr_casts(syntax, runtime);
                }
            }
            if let Some(key) = self.flow_member_key(index)?
                && let Some(prefix) = index.get_prefix_expr()
            {
                let prefix = self.type_of_expr_at(prefix.get_syntax_id(), offset)?;
                if let Some(member) = self.member_type(&prefix, &key)?
                    && !matches!(member.structural_type(), LuaType::Unknown | LuaType::Any)
                    && (self.flow_member_more_specific(&member, &value)?
                        || matches!(prefix.structural_type(), LuaType::Generic(_)))
                {
                    return self.apply_expr_casts(syntax, member);
                }
            }
            return Ok(value);
        }
        if let Some(value) = self.array_flow_index(index, offset)? {
            return self.apply_expr_casts(syntax, value);
        }
        let value = self.type_of_expr(syntax)?;
        self.apply_expr_casts(syntax, value)
    }
    fn flow_member_more_specific(
        &mut self,
        prefix: &OwnedValue,
        base: &OwnedValue,
    ) -> QueryResult<bool, E> {
        if guards::singleton(base.structural_type()) {
            return Ok(prefix == base);
        }
        if let Some(prefixes) = prefix.union_components() {
            for prefix in prefixes {
                self.checkpoint()?;
                if let Some(bases) = base.union_components() {
                    let mut matched = false;
                    for base in bases {
                        if self.type_check(&prefix, &base)? {
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        return Ok(false);
                    }
                } else if !self.type_check(&prefix, base)? {
                    return Ok(false);
                }
            }
            Ok(true)
        } else if let Some(bases) = base.union_components() {
            for base in bases {
                if self.type_check(prefix, &base)? {
                    return Ok(true);
                }
            }
            Ok(false)
        } else {
            self.type_check(prefix, base)
        }
    }
    fn array_flow_index(
        &mut self,
        index: &LuaIndexExpr,
        offset: u32,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let Some(LuaExpr::NameExpr(name)) = index.get_prefix_expr() else {
            return Ok(None);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let value = self.type_of_decl_at(&decl, offset)?;
        let components = algebra::components(value);
        let integer = match index.get_index_key() {
            Some(LuaIndexKey::Expr(expr)) => {
                if self.numeric_flow_index(&expr, &decl)? {
                    for component in &components {
                        self.checkpoint()?;
                        if let LuaType::Array(array) = component.structural_type() {
                            return Ok(Some(self.owned_child(component, 0, array.get_base())));
                        }
                    }
                }
                return Ok(None);
            }
            Some(LuaIndexKey::Integer(number)) => match number.get_number_value() {
                emmylua_parser::NumberResult::Int(value) => value,
                emmylua_parser::NumberResult::Uint(value) => value as i64,
                _ => return Ok(None),
            },
            Some(LuaIndexKey::Idx(index)) => index as i64,
            _ => return Ok(None),
        };
        for component in &components {
            self.checkpoint()?;
            if let LuaType::Array(array) = component.structural_type()
                && match array.get_len() {
                    LuaArrayLen::Max(max) | LuaArrayLen::Min(max) => integer > 0 && integer <= *max,
                    LuaArrayLen::None => false,
                }
            {
                return Ok(Some(self.owned_child(component, 0, array.get_base())));
            }
        }
        Ok(None)
    }
    fn numeric_flow_index(
        &mut self,
        expr: &LuaExpr,
        declaration: &SemanticId,
    ) -> QueryResult<bool, E> {
        let LuaExpr::NameExpr(name) = expr else {
            return Ok(false);
        };
        let Some(node) = self
            .model
            .syntax_tree()
            .and_then(|tree| name.get_syntax_id().to_node_from_root(&tree.get_red_root()))
        else {
            return Ok(false);
        };
        let mut stat = None;
        for ancestor in node.ancestors() {
            self.checkpoint()?;
            if let Some(found) = LuaForStat::cast(ancestor) {
                stat = Some(found);
                break;
            }
        }
        let Some(stat) = stat else { return Ok(false) };
        if stat.get_var_name().is_none_or(|token| {
            token.get_name_text() != name.get_name_text().as_deref().unwrap_or_default()
        }) {
            return Ok(false);
        }
        let mut exprs = Vec::new();
        for expr in stat.get_iter_expr() {
            self.checkpoint()?;
            exprs.push(expr);
        }
        if exprs.len() < 2 {
            return Ok(false);
        }
        for expr in exprs {
            for unary in expr.descendants::<LuaUnaryExpr>() {
                self.checkpoint()?;
                if unary
                    .get_op_token()
                    .is_some_and(|token| token.get_op() == UnaryOperator::OpLen)
                    && let Some(LuaExpr::NameExpr(name)) = unary.get_expr()
                    && self.model.resolve_name(name.get_position()).as_ref() == Some(declaration)
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
