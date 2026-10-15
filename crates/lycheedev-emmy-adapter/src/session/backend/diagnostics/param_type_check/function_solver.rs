//! FIFO higher-order function solver, adapted from fixed EmmyLua
//! semantic_model/infer/function_solver.rs (Copyright CppCXY, MIT).
//! This is distinct from the general recursive function-type relation.
use super::*;
use std::collections::{HashSet, VecDeque};

enum Constraint {
    Function {
        actual: OwnedValue,
        expected: OwnedValue,
    },
    Assign {
        source: OwnedValue,
        target: OwnedValue,
    },
}

type FunctionKey = (String, Option<(Vec<Option<OwnedValue>>, OwnedValue)>);

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn pt_functions_compatible(
        &mut self,
        actual: &OwnedValue,
        expected: &OwnedValue,
        bindings: &Bindings,
    ) -> QueryResult<bool, E> {
        let mut bindings = bindings.clone();
        let mut queue = VecDeque::new();
        queue.push_back(Constraint::Function {
            actual: actual.clone(),
            expected: expected.clone(),
        });
        let mut seen = HashSet::new();
        let mut failed = false;
        while let Some(constraint) = queue.pop_front() {
            self.checkpoint()?;
            match constraint {
                Constraint::Function { actual, expected } => {
                    let key = (function_key(&actual), function_key(&expected));
                    if !seen.insert(key) {
                        continue;
                    }
                    let failed_before = failed;
                    failed |= self.pt_expand_function_pair(
                        &actual,
                        &expected,
                        &mut bindings,
                        &mut queue,
                    )?;
                    if !failed_before && failed {
                        break;
                    }
                }
                Constraint::Assign { source, target } => {
                    if !self.pt_solver_types_assignable(&source, &target)? {
                        failed = true;
                        break;
                    }
                }
            }
        }
        Ok(!failed)
    }

    fn pt_solver_parameter_assignable(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if let Some(components) = source.union_components() {
            for component in &components {
                if !self.pt_solver_parameter_assignable(component, target)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        self.pt_solver_types_assignable(source, target)
    }

    fn pt_solver_types_assignable(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if !self.structure_allowed(source)? || !self.structure_allowed(target)? {
            return Ok(false);
        }
        if source == target {
            return Ok(true);
        }
        if self.type_check(source, target)? {
            return Ok(true);
        }
        if let Some(components) = target.union_components() {
            for component in &components {
                if self.pt_solver_types_assignable(source, component)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if let Some(components) = source.union_components() {
            for component in &components {
                if self.pt_solver_types_assignable(component, target)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn pt_expand_function_pair(
        &mut self,
        actual: &OwnedValue,
        expected: &OwnedValue,
        bindings: &mut Bindings,
        queue: &mut VecDeque<Constraint>,
    ) -> QueryResult<bool, E> {
        let actual_params = actual
            .function_parameters()
            .expect("actual function parameters");
        let expected_params = expected
            .function_parameters()
            .expect("expected function parameters");
        let mut failed = false;
        for (index, expected_param) in expected_params.iter().enumerate() {
            self.checkpoint()?;
            let Some(expected_param) = expected_param else {
                continue;
            };
            let actual_param = actual_params
                .get(index)
                .and_then(|ty| ty.clone())
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            // Expected substitution precedes actual substitution in the original.
            let expected_param = self.substitute_owned(expected_param.clone(), bindings)?;
            let actual_param = self.substitute_owned(actual_param, bindings)?;
            unify_bindings_or_queue(&actual_param, &expected_param, bindings, queue);
            if expected_param.structural_type().is_self_infer()
                || actual_param.structural_type().is_self_infer()
            {
                continue;
            }
            match (
                actual_param.structural_type(),
                expected_param.structural_type(),
            ) {
                (LuaType::DocFunction(_), LuaType::DocFunction(_)) => {
                    queue.push_back(Constraint::Function {
                        actual: actual_param,
                        expected: expected_param,
                    });
                }
                (_, LuaType::DocFunction(_)) => {
                    failed = true;
                }
                _ => {
                    if !self.pt_solver_parameter_assignable(&expected_param, &actual_param)? {
                        failed = true;
                    }
                }
            }
        }
        // Even a parameter mismatch still evaluates every remaining parameter
        // and both returns before the caller stops the FIFO, as upstream does.
        let actual_ret =
            self.substitute_owned(actual.function_return().expect("actual return"), bindings)?;
        let expected_ret = self.substitute_owned(
            expected.function_return().expect("expected return"),
            bindings,
        )?;
        match (actual_ret.structural_type(), expected_ret.structural_type()) {
            (LuaType::DocFunction(_), LuaType::DocFunction(_)) => {
                queue.push_back(Constraint::Function {
                    actual: actual_ret,
                    expected: expected_ret,
                });
            }
            _ => {
                if !self.pt_solver_types_assignable(&actual_ret, &expected_ret)? {
                    failed = true;
                }
            }
        }
        Ok(failed)
    }
}

fn unify_bindings_or_queue(
    actual: &OwnedValue,
    expected: &OwnedValue,
    bindings: &mut Bindings,
    queue: &mut VecDeque<Constraint>,
) {
    if let LuaType::TplRef(tpl) = actual.structural_type() {
        let id = tpl.get_tpl_id();
        match bindings.get(&id) {
            Some(existing) if existing == expected => {}
            Some(_) => queue.push_back(Constraint::Assign {
                source: expected.clone(),
                target: actual.clone(),
            }),
            None => {
                bindings.insert(id, expected.clone());
            }
        }
    }
}

fn function_key(function: &OwnedValue) -> FunctionKey {
    let LuaType::DocFunction(meta) = function.structural_type() else {
        unreachable!("function pair")
    };
    let mut fingerprint = String::new();
    for (name, ty) in meta.get_params() {
        fingerprint.push_str(name);
        fingerprint.push(':');
        if let Some(ty) = ty {
            fingerprint.push_str(&format!("{:?}", ty));
        }
        fingerprint.push(';');
    }
    fingerprint.push_str("->");
    fingerprint.push_str(&format!("{:?}", meta.get_ret()));
    // Keep the original fingerprint for ordinary types. A structural snapshot
    // alone cannot identify selected children: include precisely the parameter
    // and return carriers (not unrelated function flags) in the cycle key.
    let carriers = (!function.is_context_free()).then(|| {
        (
            function.function_parameters().expect("function parameters"),
            function.function_return().expect("function return"),
        )
    });
    (fingerprint, carriers)
}
