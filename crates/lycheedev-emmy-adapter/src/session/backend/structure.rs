//! Owned selected-object structure gate at the original type-relation boundary.
use super::{OwnedQuery, QueryResult};
use crate::session::{BindingCoverage, OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, LuaTypeNode, LuaUnionType};

enum Node<'a> {
    Owned(&'a OwnedValue),
    Plain(&'a LuaType),
}

impl<E> OwnedQuery<'_, '_, E> {
    /// Validate selected identity/completeness on every visited type node.
    /// This is a structure gate, not a completed assignability/checker proof.
    pub fn structure_allowed(&mut self, value: &OwnedValue) -> QueryResult<bool, E> {
        let mut pending = vec![Node::Owned(value)];
        let mut steps = 0;
        while let Some(node) = pending.pop() {
            self.checkpoint()?;
            steps += 1;
            if steps > 4096 {
                return Ok(false);
            }
            if let Node::Owned(value) = &node
                && value.nominal_identity().is_some()
            {
                if !value.nominal_belongs_to(self.session) {
                    return Ok(false);
                }
                continue;
            }
            if let Node::Owned(value) = &node
                && let Some(metadata) = value.object_info()
            {
                let valid = value.object_belongs_to(self.session) && metadata.complete;
                self.session.record(
                    BindingCoverage {
                        site: metadata.site,
                        operation: "structure_check",
                        state: if valid {
                            "native_checked"
                        } else {
                            "unavailable"
                        },
                        proof: metadata.proof.into(),
                        reason: (!valid).then(|| "object_context_or_shape_unavailable".into()),
                        parameter: None,
                        parameter_index: None,
                        argument: None,
                    },
                    || self.checkpoint(),
                )?;
                if !valid {
                    return Ok(false);
                }
            }
            match node {
                Node::Owned(value) if value.structural_children().is_some() => {
                    if !matches!(
                        value.structural_type(),
                        LuaType::Instance(_) | LuaType::StrTplRef(_)
                    ) {
                        for child in value
                            .structural_children()
                            .expect("checked composite children")
                        {
                            self.checkpoint()?;
                            pending.push(Node::Owned(child));
                        }
                    }
                }
                Node::Owned(value) if !value.is_context_free() => {
                    match value.structural_type() {
                        LuaType::Object(object) => {
                            // Exactly LuaTypeNode's insertion order: index pairs
                            // reversed, value before key, then HashMap values in
                            // their actual iteration order. LIFO visits fields
                            // reversed. Never substitute the sorted carrier map.
                            for (key, ty) in object.get_index_access().iter().rev() {
                                self.checkpoint()?;
                                pending.push(Node::Plain(ty));
                                pending.push(Node::Plain(key));
                            }
                            let fields = value.carried_fields().expect("object field carriers");
                            for (key, ty) in object.get_fields() {
                                self.checkpoint()?;
                                pending.push(fields.get(key).map_or(Node::Plain(ty), Node::Owned));
                            }
                        }
                        LuaType::Tuple(_) | LuaType::Variadic(_) => {
                            for slot in value
                                .carried_slots()
                                .expect("tuple/pack carriers")
                                .iter()
                                .rev()
                            {
                                self.checkpoint()?;
                                pending.push(Node::Owned(slot));
                            }
                        }
                        LuaType::DocFunction(_) => {
                            let (params, ret) =
                                value.carried_function().expect("function carriers");
                            pending.push(Node::Owned(ret));
                            for param in params.iter().rev().flatten() {
                                self.checkpoint()?;
                                pending.push(Node::Owned(param));
                            }
                        }
                        LuaType::Union(union) => {
                            let members = value.carried_union().expect("union carriers");
                            match union.as_ref() {
                                LuaUnionType::Nullable(_) => pending.push(Node::Owned(&members[0])),
                                LuaUnionType::Multi(_) => {
                                    for member in members.iter().rev() {
                                        self.checkpoint()?;
                                        pending.push(Node::Owned(member));
                                    }
                                }
                                LuaUnionType::Basic(_) => {
                                    unreachable!("basic union is context-free")
                                }
                            }
                        }
                        _ => unreachable!("all carrier variants have an explicit structure walk"),
                    }
                }
                node => {
                    // The unmodified public traversal preserves its intentional
                    // omissions (e.g. Instance and function generic constraints).
                    let ty = match node {
                        Node::Owned(value) => value.structural_type(),
                        Node::Plain(ty) => ty,
                    };
                    let mut children = Vec::new();
                    ty.push_direct_children(&mut children);
                    for child in children {
                        self.checkpoint()?;
                        pending.push(Node::Plain(child));
                    }
                }
            }
        }
        self.checkpoint()?;
        Ok(true)
    }
}
