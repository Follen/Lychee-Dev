//! Owned selected identity alongside an unmodified public structural type.
//!
//! There is no Deref/AsRef conversion to LuaType: a structural view cannot stand
//! in for selected identity, type compatibility, or a completed structure check.
use super::{BindingSite, FrozenShape, ObjectViewKey, SessionToken};
use emmylua_code_analysis::{
    LuaFunctionType, LuaMemberKey, LuaObjectType, LuaTupleStatus, LuaTupleType, LuaType,
    LuaUnionType, VariadicType,
};
use std::{
    collections::{BTreeMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct OwnedValue {
    ty: LuaType,
    evidence: Option<Arc<ValueEvidence>>,
}
#[derive(Clone, Debug)]
enum ValueEvidence {
    Object {
        binding: ObjectBinding,
        fields: BTreeMap<LuaMemberKey, OwnedValue>,
    },
    Tuple(Vec<OwnedValue>),
    Pack(Vec<OwnedValue>),
    Union(Vec<OwnedValue>),
    Structural(Vec<OwnedValue>),
    Function {
        params: Vec<Option<OwnedValue>>,
        ret: OwnedValue,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ObjectBinding {
    token: SessionToken,
    context: String,
    identity: String,
    epoch: String,
    view: ObjectViewKey,
    site: BindingSite,
    proof: String,
    complete: bool,
}
impl Hash for ObjectBinding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Same token equality contract as the old BoundObjectType. The address
        // is only a local hash input, never serialized or used as provenance.
        state.write_usize(Arc::as_ptr(&self.token.0) as usize);
        self.context.hash(state);
        self.identity.hash(state);
        self.epoch.hash(state);
        self.view.hash(state);
        self.site.hash(state);
        self.proof.hash(state);
        self.complete.hash(state);
    }
}
impl PartialEq for OwnedValue {
    fn eq(&self, other: &Self) -> bool {
        // Do not compare the public structure and then the same carried tree a
        // second time. In particular old object equality rejects foreign tokens
        // before walking fields. Constructors keep these representations paired.
        match (self.evidence.as_deref(), other.evidence.as_deref()) {
            (None, None) => self.ty == other.ty,
            (
                Some(ValueEvidence::Object {
                    binding: a,
                    fields: af,
                }),
                Some(ValueEvidence::Object {
                    binding: b,
                    fields: bf,
                }),
            ) => a == b && af == bf,
            (Some(ValueEvidence::Tuple(a)), Some(ValueEvidence::Tuple(b))) => {
                match (&self.ty, &other.ty) {
                    (LuaType::Tuple(at), LuaType::Tuple(bt)) => at.status == bt.status && a == b,
                    _ => false,
                }
            }
            (Some(ValueEvidence::Pack(a)), Some(ValueEvidence::Pack(b))) => a == b,
            (Some(ValueEvidence::Union(a)), Some(ValueEvidence::Union(b))) => {
                match (&self.ty, &other.ty) {
                    (LuaType::Union(at), LuaType::Union(bt)) => match (at.as_ref(), bt.as_ref()) {
                        (LuaUnionType::Nullable(_), LuaUnionType::Nullable(_)) => a[0] == b[0],
                        (LuaUnionType::Multi(_), LuaUnionType::Multi(_)) => {
                            if a.len() != b.len() {
                                return false;
                            }
                            let mut set: HashSet<_> = a.iter().collect();
                            for value in b {
                                if !set.remove(value) {
                                    return false;
                                }
                            }
                            set.is_empty()
                        }
                        _ => false,
                    },
                    _ => false,
                }
            }
            (
                Some(ValueEvidence::Function {
                    params: ap,
                    ret: ar,
                }),
                Some(ValueEvidence::Function {
                    params: bp,
                    ret: br,
                }),
            ) => {
                let (LuaType::DocFunction(a), LuaType::DocFunction(b)) = (&self.ty, &other.ty)
                else {
                    return false;
                };
                a.get_async_state() == b.get_async_state()
                    && a.is_colon_define() == b.is_colon_define()
                    && a.is_variadic() == b.is_variadic()
                    && a.get_generic_params() == b.get_generic_params()
                    && a.get_params()
                        .iter()
                        .map(|(name, _)| name)
                        .eq(b.get_params().iter().map(|(name, _)| name))
                    && ap == bp
                    && ar == br
            }
            (Some(ValueEvidence::Structural(a)), Some(ValueEvidence::Structural(b))) => {
                if self.ty != other.ty {
                    return false;
                }
                match (&self.ty, &other.ty) {
                    (LuaType::Object(left), LuaType::Object(right)) => {
                        let prefix = left.get_index_access().len() * 2;
                        if a[..prefix] != b[..prefix] {
                            return false;
                        }
                        let left: BTreeMap<_, _> =
                            left.get_fields().keys().zip(&a[prefix..]).collect();
                        let right: BTreeMap<_, _> =
                            right.get_fields().keys().zip(&b[prefix..]).collect();
                        left == right
                    }
                    _ => a == b,
                }
            }
            _ => false,
        }
    }
}
impl Eq for OwnedValue {}
impl Hash for OwnedValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.evidence.as_deref() {
            None => {
                0u8.hash(state);
                self.ty.hash(state);
            }
            Some(ValueEvidence::Object { binding, fields }) => {
                1u8.hash(state);
                binding.hash(state);
                fields.hash(state);
            }
            Some(ValueEvidence::Tuple(slots)) => {
                2u8.hash(state);
                if let LuaType::Tuple(tuple) = &self.ty {
                    tuple.status.hash(state);
                }
                slots.hash(state);
            }
            Some(ValueEvidence::Pack(slots)) => {
                3u8.hash(state);
                slots.hash(state);
            }
            Some(ValueEvidence::Union(members)) => {
                4u8.hash(state);
                if matches!(&self.ty, LuaType::Union(u) if matches!(u.as_ref(), LuaUnionType::Nullable(_)))
                {
                    0u8.hash(state);
                    members[0].hash(state);
                } else {
                    1u8.hash(state);
                    members.len().hash(state);
                    let mut sum = 0u64;
                    for value in members {
                        let mut hash = DefaultHasher::new();
                        value.hash(&mut hash);
                        sum = sum.wrapping_add(hash.finish());
                    }
                    sum.hash(state);
                }
            }
            Some(ValueEvidence::Structural(children)) => {
                6u8.hash(state);
                self.ty.hash(state);
                if let LuaType::Object(object) = &self.ty {
                    let prefix = object.get_index_access().len() * 2;
                    children[..prefix].hash(state);
                    let fields: BTreeMap<_, _> = object
                        .get_fields()
                        .keys()
                        .zip(&children[prefix..])
                        .collect();
                    fields.hash(state);
                } else {
                    children.hash(state);
                }
            }
            Some(ValueEvidence::Function { params, ret }) => {
                5u8.hash(state);
                let LuaType::DocFunction(function) = &self.ty else {
                    unreachable!("function carrier type");
                };
                function.get_async_state().hash(state);
                function.is_colon_define().hash(state);
                function.is_variadic().hash(state);
                function.get_generic_params().hash(state);
                for (name, _) in function.get_params() {
                    name.hash(state);
                }
                params.hash(state);
                ret.hash(state);
            }
        }
    }
}

/// Read-only evidence inspection, not a member lookup or structure-check receipt.
/// The private token is deliberately not exposed or reconstructible from this view.
#[derive(Clone, Copy, Debug)]
pub struct OwnedObjectInfo<'a> {
    pub context: &'a str,
    pub identity: &'a str,
    pub epoch: &'a str,
    pub view: &'a super::ObjectViewKey,
    pub site: BindingSite,
    pub proof: &'a str,
    pub complete: bool,
}

impl OwnedValue {
    pub(crate) fn plain(ty: LuaType) -> Self {
        Self { ty, evidence: None }
    }

    /// Composite children are in LuaTypeNode::push_direct_children insertion
    /// order, not visitation order. Instance retains its base as one child even
    /// though the original structure gate deliberately does not visit it.
    /// Existing bound Object/Tuple/Pack/Union/Function constructors remain the
    /// authoritative constructors for their own identity-bearing variants.
    pub(crate) fn with_structural_children(ty: LuaType, children: Vec<Self>) -> Self {
        if children.iter().all(Self::is_context_free) {
            return Self::plain(ty);
        }
        Self {
            ty,
            evidence: Some(Arc::new(ValueEvidence::Structural(children))),
        }
    }
    pub(crate) fn structural_children(&self) -> Option<&[Self]> {
        match self.evidence.as_deref()? {
            ValueEvidence::Structural(children) => Some(children),
            _ => None,
        }
    }

    /// Consuming conversion is crate-private and refuses every nested carrier.
    /// A restricted semantic operation can continue explicitly instead of
    /// accidentally erasing a selected object inside a pack/tuple.
    pub(crate) fn into_context_free_type(self) -> Result<LuaType, Self> {
        if self.is_context_free() {
            Ok(self.ty)
        } else {
            Err(self)
        }
    }

    /// For public shape inspection/rendering only. Equality and semantic
    /// operations must retain this OwnedValue, not reconstruct it from the view.
    pub fn structural_type(&self) -> &LuaType {
        &self.ty
    }
    pub fn is_context_free(&self) -> bool {
        self.evidence.is_none()
    }
    pub(crate) fn object_belongs_to(&self, session: &super::OwnedBindingSession) -> bool {
        match self.evidence.as_deref() {
            Some(ValueEvidence::Object { binding, .. }) => {
                binding.token == session.token && binding.context == session.identity()
            }
            _ => false,
        }
    }
    pub fn object_info(&self) -> Option<OwnedObjectInfo<'_>> {
        let ValueEvidence::Object { binding, .. } = self.evidence.as_deref()? else {
            return None;
        };
        Some(OwnedObjectInfo {
            context: &binding.context,
            identity: &binding.identity,
            epoch: &binding.epoch,
            view: &binding.view,
            site: binding.site,
            proof: &binding.proof,
            complete: binding.complete,
        })
    }
    /// Read-only carrier inspection. Does not resolve members or validate this
    /// value against any caller's session; the owned member consumer must do so.
    pub fn carried_fields(&self) -> Option<&BTreeMap<LuaMemberKey, Self>> {
        match self.evidence.as_deref()? {
            ValueEvidence::Object { fields, .. } => Some(fields),
            _ => None,
        }
    }
    /// Carried tuple/pack slots; None means all slots are context-free, not that
    /// a tuple/pack has zero elements. This is inspection, not Lua scalarization.
    pub fn carried_slots(&self) -> Option<&[Self]> {
        match self.evidence.as_deref()? {
            ValueEvidence::Tuple(slots) | ValueEvidence::Pack(slots) => Some(slots),
            _ => None,
        }
    }

    /// Union components in the public union's exact order, including trailing
    /// Nil for Nullable. Inspection does not validate compatibility or identity.
    pub fn carried_union(&self) -> Option<&[Self]> {
        match self.evidence.as_deref()? {
            ValueEvidence::Union(members) => Some(members),
            _ => None,
        }
    }

    pub(crate) fn function(
        metadata: &LuaFunctionType,
        params: Vec<Option<Self>>,
        ret: Self,
    ) -> Self {
        assert_eq!(metadata.get_params().len(), params.len());
        let selected = !ret.is_context_free()
            || params
                .iter()
                .flatten()
                .any(|value| !value.is_context_free());
        let types = metadata
            .get_params()
            .iter()
            .zip(&params)
            .map(|((name, _), ty)| (name.clone(), ty.as_ref().map(|value| value.ty.clone())))
            .collect();
        let ty = LuaType::DocFunction(Arc::new(LuaFunctionType::new(
            metadata.get_async_state(),
            metadata.is_colon_define(),
            metadata.is_variadic(),
            types,
            ret.ty.clone(),
            Some(metadata.get_generic_params().to_vec()),
        )));
        Self {
            ty,
            evidence: selected.then(|| Arc::new(ValueEvidence::Function { params, ret })),
        }
    }

    pub fn carried_function(&self) -> Option<(&[Option<Self>], &Self)> {
        match self.evidence.as_deref()? {
            ValueEvidence::Function { params, ret } => Some((params, ret)),
            _ => None,
        }
    }
    pub(crate) fn function_return(&self) -> Option<Self> {
        let LuaType::DocFunction(function) = &self.ty else {
            return None;
        };
        Some(self.carried_function().map_or_else(
            || Self::plain(function.get_ret().clone()),
            |(_, ret)| ret.clone(),
        ))
    }
    pub(crate) fn function_parameters(&self) -> Option<Vec<Option<Self>>> {
        let LuaType::DocFunction(function) = &self.ty else {
            return None;
        };
        Some(self.carried_function().map_or_else(
            || {
                function
                    .get_params()
                    .iter()
                    .map(|(_, ty)| ty.clone().map(Self::plain))
                    .collect()
            },
            |(params, _)| params.to_vec(),
        ))
    }

    pub(crate) fn object(
        frozen: FrozenShape,
        site: BindingSite,
        complete: bool,
        fields: Vec<(LuaMemberKey, OwnedValue)>,
    ) -> Self {
        // Build the public hash map in original provider order, not sorted order.
        let structural = fields
            .iter()
            .map(|(key, value)| (key.clone(), value.ty.clone()))
            .collect();
        let fields = fields.into_iter().collect();
        Self {
            ty: LuaType::Object(Arc::new(LuaObjectType::new_with_fields(
                structural,
                Vec::new(),
            ))),
            evidence: Some(Arc::new(ValueEvidence::Object {
                binding: ObjectBinding {
                    token: frozen.token,
                    context: frozen.object.view.context.clone(),
                    identity: frozen.object.identity,
                    epoch: frozen.object.epoch,
                    view: frozen.object.view,
                    site,
                    proof: frozen.shape.proof.clone(),
                    complete,
                },
                fields,
            })),
        }
    }
    pub(crate) fn rebuild_object_fields(&self, fields: Vec<(LuaMemberKey, Self)>) -> Self {
        let Some(ValueEvidence::Object { binding, .. }) = self.evidence.as_deref() else {
            unreachable!("selected object rebuild");
        };
        let LuaType::Object(object) = &self.ty else {
            unreachable!("selected object type");
        };
        Self {
            ty: LuaType::Object(Arc::new(LuaObjectType::new_with_fields(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), value.ty.clone()))
                    .collect(),
                object.get_index_access().to_vec(),
            ))),
            evidence: Some(Arc::new(ValueEvidence::Object {
                binding: binding.clone(),
                fields: fields.into_iter().collect(),
            })),
        }
    }

    /// LuaUnionType::from_vec itself, without LuaType::from_vec flatten/dedup.
    pub(crate) fn raw_union(mut members: Vec<Self>) -> Self {
        let selected = members.iter().any(|value| !value.is_context_free());
        let union = LuaUnionType::from_vec(members.iter().map(|value| value.ty.clone()).collect());
        if matches!(&union, LuaUnionType::Nullable(_)) && matches!(members[0].ty, LuaType::Nil) {
            members.swap(0, 1);
        }
        Self {
            ty: LuaType::Union(Arc::new(union)),
            evidence: selected.then(|| Arc::new(ValueEvidence::Union(members))),
        }
    }

    pub(crate) fn pack(slots: Vec<Self>) -> Self {
        let selected = slots.iter().any(|slot| !slot.is_context_free());
        if selected {
            let types = slots.iter().map(|slot| slot.ty.clone()).collect();
            Self {
                ty: LuaType::Variadic(Arc::new(VariadicType::Multi(types))),
                evidence: Some(Arc::new(ValueEvidence::Pack(slots))),
            }
        } else {
            let types = slots.into_iter().map(|slot| slot.ty).collect();
            Self::plain(LuaType::Variadic(Arc::new(VariadicType::Multi(types))))
        }
    }
    pub(crate) fn tuple_checked<E>(
        slots: Vec<Self>,
        status: LuaTupleStatus,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Self, E> {
        let mut selected = false;
        for slot in &slots {
            check()?;
            selected |= !slot.is_context_free();
        }
        let mut types = Vec::with_capacity(slots.len());
        if selected {
            for slot in &slots {
                check()?;
                types.push(slot.ty.clone());
            }
            check()?;
            Ok(Self {
                ty: LuaType::Tuple(Arc::new(LuaTupleType::new(types, status))),
                evidence: Some(Arc::new(ValueEvidence::Tuple(slots))),
            })
        } else {
            for slot in slots {
                check()?;
                types.push(slot.ty);
            }
            check()?;
            Ok(Self::plain(LuaType::Tuple(Arc::new(LuaTupleType::new(
                types, status,
            )))))
        }
    }
    pub(crate) fn result_slot(&self, index: usize) -> Option<Self> {
        if let Some(evidence) = &self.evidence {
            if let ValueEvidence::Structural(children) = evidence.as_ref()
                && matches!(&self.ty, LuaType::Variadic(value) if matches!(value.as_ref(), VariadicType::Base(_)))
            {
                // Upstream returns Base itself for every slot (without applying
                // another result-slot operation to a nested Base/pack).
                return children.first().cloned();
            }
            if let ValueEvidence::Pack(slots) = evidence.as_ref() {
                let last = slots.len().checked_sub(1)?;
                if index < last {
                    return slots[index].result_slot(0);
                }
                let tail = &slots[last];
                return tail.result_slot(index - last).or_else(|| {
                    (!matches!(tail.ty, LuaType::Variadic(_))).then(|| Self::plain(LuaType::Nil))
                });
            }
            if let ValueEvidence::Union(members) = evidence.as_ref() {
                if index == 0 && !self.ty.contain_multi_return() {
                    return Some(self.clone());
                }
                let slots: Vec<_> = members
                    .iter()
                    .map(|member| member.result_slot(index))
                    .collect();
                if !slots.iter().any(Option::is_some) {
                    return None;
                }
                return Some(Self::union(
                    slots
                        .into_iter()
                        .map(|slot| slot.unwrap_or_else(|| Self::plain(LuaType::Nil)))
                        .collect(),
                ));
            }
            // A selected object or tuple is one scalar value.
            return (index == 0).then(|| self.clone());
        }
        self.ty.get_result_slot_type(index).map(Self::plain)
    }
}

impl OwnedValue {
    /// Exact LuaType::from_vec shape rules, deduplicating full selected identity
    /// before constructing a public structural union. One input is unchanged;
    /// only a multi-input call flattens one union layer.
    pub(crate) fn union_checked<E>(
        values: Vec<Self>,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Self, E> {
        check()?;
        if values.is_empty() {
            return Ok(Self::plain(LuaType::Nil));
        }
        if values.len() == 1 {
            return Ok(values.into_iter().next().unwrap());
        }
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        for value in values {
            check()?;
            for member in value.union_components().unwrap_or_else(|| vec![value]) {
                check()?;
                if seen.insert(member.clone()) {
                    members.push(member);
                }
            }
        }
        check()?;
        if members.is_empty() {
            return Ok(Self::plain(LuaType::Nil));
        }
        if members.len() == 1 {
            return Ok(members.pop().unwrap());
        }
        if members.iter().all(Self::is_context_free) {
            // This constructor deliberately does not perform another flatten or
            // dedup pass. It preserves Basic/Nullable/Multi representation.
            return Ok(Self::plain(LuaType::Union(Arc::new(
                LuaUnionType::from_vec(members.into_iter().map(|v| v.ty).collect()),
            ))));
        }
        let mut types = Vec::with_capacity(members.len());
        for member in &members {
            check()?;
            types.push(member.ty.clone());
        }
        let union = LuaUnionType::from_vec(types);
        if matches!(&union, LuaUnionType::Nullable(_)) && matches!(members[0].ty, LuaType::Nil) {
            members.swap(0, 1);
        }
        check()?;
        Ok(Self {
            ty: LuaType::Union(Arc::new(union)),
            evidence: Some(Arc::new(ValueEvidence::Union(members))),
        })
    }

    // Existing result-slot contracts are synchronous just like the upstream
    // get_result_slot_type. Query owners still check their original control on
    // both sides; this wrapper cannot swallow an error or invent a new deadline.
    fn union(values: Vec<Self>) -> Self {
        match Self::union_checked(values, || Ok::<_, std::convert::Infallible>(())) {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }
    pub(crate) fn union_components(&self) -> Option<Vec<Self>> {
        let LuaType::Union(union) = &self.ty else {
            return None;
        };
        Some(match self.carried_union() {
            Some(members) => members.to_vec(),
            None => union.into_vec().into_iter().map(Self::plain).collect(),
        })
    }

    pub(crate) fn logical_components<E>(
        &self,
        truthy: bool,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<Self>, E> {
        let mut pending = vec![self.clone()];
        let mut result = Vec::new();
        while let Some(value) = pending.pop() {
            check()?;
            if let Some(members) = value.union_components() {
                for member in members.into_iter().rev() {
                    check()?;
                    pending.push(member);
                }
                continue;
            }
            match value.structural_type() {
                LuaType::Boolean => result.push(Self::plain(LuaType::BooleanConst(truthy))),
                LuaType::BooleanConst(b) | LuaType::DocBooleanConst(b) if *b == truthy => {
                    result.push(value)
                }
                LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_) => {}
                LuaType::Nil => {
                    if !truthy {
                        result.push(value);
                    }
                }
                LuaType::Unknown | LuaType::Any => result.push(value),
                _ => {
                    if truthy {
                        result.push(value);
                    }
                }
            }
        }
        check()?;
        Ok(result)
    }
}
