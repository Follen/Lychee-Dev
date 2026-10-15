//! TypeGuard projection first uses the non-contextual signature shell, then the
//! actual owned call VM only for the original insufficient-projection branch.
use super::*;
use emmylua_code_analysis::{FileId, GenericTplId, Signature};
use emmylua_parser::LuaCallExpr;
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn type_guard_target(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<(LuaExpr, OwnedValue)>, E> {
        if let Some((file, _, signature)) = self.flow_callable_signature(call)?
            && let Some(value) = self.flow_signature_return(call)?
            && let Some(guard) = self.guard_inner(&value)
        {
            let guard = self.resolve_guard_generic(file, call, &signature, guard)?;
            let guard = normalize(guard);
            if !matches!(guard.structural_type(), LuaType::Unknown | LuaType::Any)
                && !guard.structural_type().contain_tpl()
            {
                return Ok(call
                    .get_args_list()
                    .and_then(|list| list.get_args().next())
                    .map(|arg| (arg, guard)));
            }
        }
        let mut value = self.type_of_expr(call.get_syntax_id())?;
        if value.structural_type().is_unknown()
            && let Some(signature) = self.flow_signature_return(call)?
        {
            value = signature;
        }
        let Some(guard) = self.guard_inner(&value) else {
            return Ok(None);
        };
        let guard = normalize(guard);
        Ok(call
            .get_args_list()
            .and_then(|list| list.get_args().next())
            .map(|arg| (arg, guard)))
    }
    fn guard_inner(&self, value: &OwnedValue) -> Option<OwnedValue> {
        match value.structural_type() {
            LuaType::TypeGuard(inner) => Some(self.owned_child(value, 0, inner)),
            LuaType::Generic(generic) if generic.get_base_type_id().get_name() == "TypeGuard" => {
                generic
                    .get_params()
                    .first()
                    .map(|param| self.owned_child(value, 0, param))
            }
            _ => None,
        }
    }
    fn resolve_guard_generic(
        &mut self,
        file: FileId,
        call: &LuaCallExpr,
        signature: &Signature,
        guard: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let (id, name, keep_literal) = match guard.structural_type() {
            LuaType::TplRef(tpl) => (
                tpl.get_tpl_id(),
                tpl.get_name(),
                tpl.is_const() || tpl.get_constraint().is_some(),
            ),
            LuaType::StrTplRef(tpl) => (tpl.get_tpl_id(), tpl.get_name(), false),
            _ => return Ok(guard),
        };
        let index = match id {
            GenericTplId::Type(index) | GenericTplId::Func(index) => index as usize,
            _ => return Ok(guard),
        };
        let Some(docs) = &signature.docs else {
            return Ok(guard);
        };
        let Some(param) = docs.generic_params.get(index) else {
            return Ok(guard);
        };
        let name = if name.is_empty() {
            param.name.as_str()
        } else {
            name
        };
        let mut parameter_index = None;
        for (index, parameter) in signature.param_names.iter().enumerate() {
            self.checkpoint()?;
            let Some((_, syntax)) = docs.param_types.iter().find(|(name, _)| name == parameter)
            else {
                continue;
            };
            let ty = self
                .model
                .doc_type_lua_in(file, *syntax, &docs.generic_params);
            if matches!(&ty, LuaType::TplRef(tpl) if tpl.get_name() == name)
                || matches!(&ty, LuaType::StrTplRef(tpl) if tpl.get_name() == name)
                || &ty == guard.structural_type()
            {
                parameter_index = Some(index);
                break;
            }
        }
        let Some(index) = parameter_index else {
            return Ok(guard);
        };
        let Some(arg) = call
            .get_args_list()
            .and_then(|list| list.get_args().nth(index))
        else {
            return Ok(guard);
        };
        let declared = match &arg {
            LuaExpr::NameExpr(name) => match self.model.resolve_name(name.get_position()) {
                Some(decl) => self.type_of_decl(&decl)?,
                None => None,
            },
            _ => None,
        };
        let value = match declared {
            Some(value) => value,
            None => self.type_of_expr(arg.get_syntax_id())?,
        };
        Ok(if keep_literal {
            value
        } else {
            match value.structural_type() {
                LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                    OwnedValue::plain(LuaType::String)
                }
                LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => {
                    OwnedValue::plain(LuaType::Integer)
                }
                _ => value,
            }
        })
    }
}
fn normalize(value: OwnedValue) -> OwnedValue {
    match value.structural_type() {
        LuaType::Ref(id) | LuaType::Def(id) => guards::primitive(id.get_name())
            .map(OwnedValue::plain)
            .unwrap_or(value),
        _ => value,
    }
}
