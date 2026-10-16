//! Member discriminants and dynamic literal keys use owned values throughout.
use super::*;
use emmylua_code_analysis::{LuaMemberKey, TypeDefKind};
use emmylua_parser::{LuaIndexExpr, LuaIndexKey, LuaLiteralExpr, LuaLiteralToken};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn narrow_flow_intersect(
        &mut self,
        left: OwnedValue,
        right: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        let common = self.flow_intersect(left.clone(), right.clone())?;
        if !common.structural_type().is_unknown() {
            return Ok(common);
        }
        Ok(match (left.structural_type(), right.structural_type()) {
            (LuaType::Integer, LuaType::Number) | (LuaType::Number, LuaType::Integer) => {
                OwnedValue::plain(LuaType::Integer)
            }
            (
                LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
                LuaType::Integer | LuaType::Number,
            )
            | (
                LuaType::Integer | LuaType::Number,
                LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
            )
            | (LuaType::StringConst(_) | LuaType::DocStringConst(_), LuaType::String)
            | (LuaType::String, LuaType::StringConst(_) | LuaType::DocStringConst(_))
            | (LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_), LuaType::Boolean)
            | (LuaType::Boolean, LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_)) => left,
            _ => common,
        })
    }
    pub(super) fn filter_flow_member(
        &mut self,
        value: OwnedValue,
        key: &LuaMemberKey,
        literal: Option<&OwnedValue>,
        keep: bool,
    ) -> QueryResult<OwnedValue, E> {
        let mut out = Vec::new();
        for component in algebra::components(value) {
            self.checkpoint()?;
            if let LuaType::Ref(id) | LuaType::Def(id) = component.structural_type()
                && let Some(def) = self.model.type_def_of(id)
                && def.kind == TypeDefKind::Alias
                && let Some(target) = self.model.alias_target(&def)
            {
                let expanded =
                    self.filter_flow_member(OwnedValue::plain(target), key, literal, keep)?;
                if !matches!(
                    expanded.structural_type(),
                    LuaType::Unknown | LuaType::Never
                ) {
                    out.push(component);
                }
                continue;
            }
            let member = self.member_type(&component, key)?;
            let matched = if let Some(literal) = literal {
                match member {
                    Some(member) => self.flow_overlap(member, literal.clone())?,
                    None => false,
                }
            } else {
                let member = member.unwrap_or_else(|| OwnedValue::plain(LuaType::Nil));
                !algebra::components(member).iter().all(|part| {
                    matches!(
                        part.structural_type(),
                        LuaType::Nil
                            | LuaType::BooleanConst(false)
                            | LuaType::DocBooleanConst(false)
                    )
                })
            };
            if matched == keep {
                out.push(component);
            }
        }
        Ok(if out.is_empty() {
            OwnedValue::plain(LuaType::Never)
        } else {
            algebra::from_components(out)
        })
    }
    pub(super) fn flow_member_discriminant(
        &mut self,
        left: &LuaExpr,
        right: &LuaExpr,
    ) -> QueryResult<Option<(SemanticId, LuaMemberKey, OwnedValue)>, E> {
        let (index, literal) = match (left, right) {
            (LuaExpr::IndexExpr(index), LuaExpr::LiteralExpr(literal))
            | (LuaExpr::LiteralExpr(literal), LuaExpr::IndexExpr(index)) => (index, literal),
            _ => return Ok(None),
        };
        let Some(key) = self.flow_member_key(index)? else {
            return Ok(None);
        };
        let Some(literal) = guards::literal_type(literal) else {
            return Ok(None);
        };
        let Some(LuaExpr::NameExpr(name)) = index.get_prefix_expr() else {
            return Ok(None);
        };
        self.checkpoint()?;
        Ok(self
            .model
            .resolve_name(name.get_position())
            .map(|decl| (decl, key, OwnedValue::plain(literal))))
    }
    pub(super) fn flow_member_key(
        &mut self,
        index: &LuaIndexExpr,
    ) -> QueryResult<Option<LuaMemberKey>, E> {
        self.checkpoint()?;
        Ok(match index.get_index_key() {
            Some(LuaIndexKey::Name(name)) => Some(LuaMemberKey::Name(name.get_name_text().into())),
            Some(LuaIndexKey::String(value)) => Some(LuaMemberKey::Name(value.get_value().into())),
            Some(LuaIndexKey::Expr(expr)) => {
                self.dynamic_flow_key(&expr, expr.get_range().start().into())?
            }
            _ => None,
        })
    }
    fn dynamic_flow_key(
        &mut self,
        expr: &LuaExpr,
        offset: u32,
    ) -> QueryResult<Option<LuaMemberKey>, E> {
        self.checkpoint()?;
        if matches!(expr, LuaExpr::NameExpr(_)) {
            let value = self.type_of_expr_at(expr.get_syntax_id(), offset)?;
            if let LuaType::StringConst(value) | LuaType::DocStringConst(value) =
                value.structural_type()
            {
                return Ok(Some(LuaMemberKey::Name(value.as_str().into())));
            }
        }
        let LuaExpr::IndexExpr(index) = expr else {
            return Ok(None);
        };
        let Some(LuaIndexKey::Expr(LuaExpr::NameExpr(name))) = index.get_index_key() else {
            return Ok(None);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let key = self.type_of_decl_at(&decl, offset)?;
        let int = match key.structural_type() {
            LuaType::IntegerConst(value) | LuaType::DocIntegerConst(value) => *value,
            _ => return Ok(None),
        };
        let Some(LuaExpr::NameExpr(name)) = index.get_prefix_expr() else {
            return Ok(None);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let value = self.type_of_decl_at(&decl, offset)?;
        let LuaType::TableConst(table) = value.structural_type() else {
            return Ok(None);
        };
        let owner = SemanticId::member(table.file_id, table.value);
        for member in self.model.members_of_owner(&owner).iter() {
            self.checkpoint()?;
            let Some(member_def) = self
                .model
                .file_facts_of(member.file_id)
                .and_then(|facts| facts.member_by_id(&member.id))
                .filter(|member| member.key == LuaMemberKey::Integer(int))
            else {
                continue;
            };
            // First matching producer wins, even when its value is not literal.
            let literal = member_def
                .value_syntax
                .and_then(|syntax| {
                    self.model
                        .syntax_tree_of(member.file_id)
                        .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                })
                .and_then(LuaLiteralExpr::cast);
            return Ok(match literal.and_then(|literal| literal.get_literal()) {
                Some(LuaLiteralToken::String(value)) => {
                    Some(LuaMemberKey::Name(value.get_value().into()))
                }
                _ => None,
            });
        }
        Ok(None)
    }
}
