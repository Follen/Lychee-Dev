//! Owned type projection and scalar-only shape selection for argument checks.
use super::*;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn pt_generic_base_match(
        &mut self,
        arg: &OwnedValue,
        param: &OwnedValue,
    ) -> QueryResult<bool, E> {
        let p = match param.structural_type() {
            LuaType::Generic(g) => Some(g.get_base_type_id()),
            _ => None,
        };
        let a = match arg.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => Some(id.clone()),
            LuaType::Generic(g) => Some(g.get_base_type_id()),
            _ => None,
        };
        match (p, a) {
            (Some(p), Some(a)) if p != a => self.type_check_subtype(arg, param),
            _ => self.type_check(arg, param),
        }
    }

    pub(super) fn pt_normalize_arg(&mut self, value: &OwnedValue) -> QueryResult<OwnedValue, E> {
        let expanded = self.expand_owned_aliases(value.clone())?;
        if contains_mapped(expanded.structural_type()) {
            return Ok(value.clone());
        }
        self.eval_owned_conditionals(expanded)
    }

    pub(super) fn pt_call_argument_type(
        &mut self,
        arg: &LuaExpr,
        value: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        if let Some(value) = self.binding_type_of_expr(arg.get_syntax_id())? {
            return Ok(value);
        }
        let constrained = self
            .pt_generic_constraint_type(value)?
            .unwrap_or_else(|| value.clone());
        if let LuaExpr::ClosureExpr(closure) = arg
            && let Ok(function) =
                self.signature_in(self.model.file_id(), closure.get_syntax_id())?
        {
            return Ok(function);
        }
        if matches!(constrained.structural_type(), LuaType::Function)
            && let LuaExpr::NameExpr(name) = arg
            && let Some(decl) = self.model.resolve_name(name.get_position())
        {
            if let Some(function) = self.access_decl_signature(&decl)? {
                return Ok(function);
            }
            if let Some(facts) = self.model.file_facts()
                && let Some(decl) = facts.decl_by_id(&decl)
                && let Some(syntax) = decl.doc_type_syntax
            {
                let ty = OwnedValue::plain(self.model.doc_type_lua_rich(syntax));
                if matches!(ty.structural_type(), LuaType::DocFunction(_)) {
                    return Ok(ty);
                }
                if let Some(components) = ty.union_components() {
                    for component in components {
                        if matches!(component.structural_type(), LuaType::DocFunction(_)) {
                            return Ok(component);
                        }
                    }
                }
            }
        }
        Ok(constrained)
    }

    fn pt_generic_constraint_type(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let (LuaType::Ref(id) | LuaType::Def(id)) = value.structural_type() else {
            return Ok(None);
        };
        if self.model.type_def_of(id).is_some() {
            return Ok(None);
        }
        let name = id.get_name();
        let Some(signatures) = self.model.signatures() else {
            return Ok(None);
        };
        for signature in signatures {
            self.checkpoint()?;
            // Preserve the original early None at a signature lacking docs.
            let Some(docs) = signature.docs.as_ref() else {
                return Ok(None);
            };
            if let Some(param) = docs
                .generic_params
                .iter()
                .find(|param| param.name.as_str() == name)
                && let Some(syntax) = param.constraint
            {
                let ty = self.model.doc_type_lua(syntax);
                if !matches!(ty, LuaType::Unknown) {
                    return Ok(Some(OwnedValue::plain(ty)));
                }
            }
        }
        Ok(None)
    }

    pub(super) fn pt_doc_function(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::DocFunction(_) => Ok(Some(value.clone())),
            LuaType::Signature(id) => self.signature_by_id(id),
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(None);
                };
                if def.kind != TypeDefKind::Alias {
                    return Ok(None);
                }
                let Some(target) = self.model.alias_target(&def).map(OwnedValue::plain) else {
                    return Ok(None);
                };
                Ok(matches!(target.structural_type(), LuaType::DocFunction(_)).then_some(target))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn pt_function_arg_compatible(
        &mut self,
        arg: &OwnedValue,
        param: &OwnedValue,
        bindings: &Bindings,
    ) -> QueryResult<bool, E> {
        if matches!(arg.structural_type(), LuaType::Function)
            && is_function_like_type(param.structural_type())
        {
            return Ok(true);
        }
        let actual = self.pt_doc_function(arg)?;
        let expected = self.pt_doc_function(param)?;
        match (actual, expected) {
            (Some(actual), Some(expected)) => {
                self.pt_functions_compatible(&actual, &expected, bindings)
            }
            _ => Ok(matches!(
                (arg.structural_type(), param.structural_type()),
                (
                    LuaType::DocFunction(_) | LuaType::Signature(_),
                    LuaType::DocFunction(_) | LuaType::Signature(_)
                )
            )),
        }
    }

    pub(super) fn pt_union_component_accepts(
        &mut self,
        arg: &OwnedValue,
        component: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if arg == component {
            return Ok(true);
        }
        match (arg.structural_type(), component.structural_type()) {
            (LuaType::StringConst(a), LuaType::StringConst(b))
            | (LuaType::DocStringConst(a), LuaType::DocStringConst(b))
                if a != b =>
            {
                Ok(false)
            }
            (LuaType::IntegerConst(a), LuaType::IntegerConst(b))
            | (LuaType::DocIntegerConst(a), LuaType::DocIntegerConst(b))
                if a != b =>
            {
                Ok(false)
            }
            (LuaType::BooleanConst(a), LuaType::BooleanConst(b))
            | (LuaType::DocBooleanConst(a), LuaType::DocBooleanConst(b))
                if a != b =>
            {
                Ok(false)
            }
            _ => self.type_check(arg, component),
        }
    }
}

pub(super) fn effective_param(param: &OwnedValue, arg: &OwnedValue) -> OwnedValue {
    if let Some(components) = param.union_components() {
        if components.iter().any(|ty| {
            matches!(
                ty.structural_type(),
                LuaType::TplRef(_) | LuaType::StrTplRef(_)
            )
        }) {
            return param.clone();
        }
        if !matches!(arg.structural_type(), LuaType::Nil) {
            let non_nil: Vec<&OwnedValue> = components
                .iter()
                .filter(|ty| !matches!(ty.structural_type(), LuaType::Nil))
                .collect();
            let function_like = matches!(
                arg.structural_type(),
                LuaType::DocFunction(_) | LuaType::Signature(_) | LuaType::Function
            );
            if function_like
                && let Some(component) = non_nil.iter().find(|ty| {
                    matches!(
                        ty.structural_type(),
                        LuaType::DocFunction(_) | LuaType::Signature(_) | LuaType::TplRef(_)
                    )
                })
            {
                return (*component).clone();
            }
            if let Some(component) = non_nil.first() {
                return (*component).clone();
            }
        }
    }
    param.clone()
}

pub(super) fn is_function_like_type(ty: &LuaType) -> bool {
    match ty {
        LuaType::DocFunction(_) | LuaType::Signature(_) => true,
        LuaType::Union(union) => union.into_vec().iter().any(is_function_like_type),
        LuaType::Intersection(intersection) => {
            intersection.get_types().iter().any(is_function_like_type)
        }
        _ => false,
    }
}

pub(super) fn contains_mapped(ty: &LuaType) -> bool {
    use LuaType::*;
    match ty {
        Mapped(_) => true,
        Array(array) => contains_mapped(array.get_base()),
        Tuple(tuple) => tuple.get_types().iter().any(contains_mapped),
        Union(union) => union.into_vec().iter().any(contains_mapped),
        Intersection(intersection) => intersection.get_types().iter().any(contains_mapped),
        Object(object) => {
            object.get_fields().values().any(contains_mapped)
                || object
                    .get_index_access()
                    .iter()
                    .any(|(k, v)| contains_mapped(k) || contains_mapped(v))
        }
        Variadic(variadic) => match variadic.as_ref() {
            VariadicType::Base(base) => contains_mapped(base),
            VariadicType::Multi(types) => types.iter().any(contains_mapped),
        },
        Call(call) => call.get_operands().iter().any(contains_mapped),
        Generic(generic) => generic.get_params().iter().any(contains_mapped),
        _ => false,
    }
}

pub(super) fn skip_arg(arg_ty: &LuaType, param_ty: &LuaType) -> bool {
    if matches!(arg_ty, LuaType::Unknown | LuaType::Any) {
        return true;
    }
    matches!(arg_ty, LuaType::Nil) && matches!(param_ty, LuaType::Any | LuaType::Unknown)
}
