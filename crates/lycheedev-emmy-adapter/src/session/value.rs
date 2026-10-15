//! Owned selected identity alongside an unmodified public structural type.
//!
//! There is no Deref/AsRef conversion to LuaType: a structural view cannot stand
//! in for selected identity, type compatibility, or a completed structure check.
use super::{BindingSite, FrozenShape, ObjectViewKey, SessionToken};
use emmylua_code_analysis::{
    LuaMemberKey, LuaObjectType, LuaTupleStatus, LuaTupleType, LuaType, VariadicType,
};
use std::{
    collections::BTreeMap,
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
            // A selected object or tuple is one scalar value. A selected union
            // is not created by this slice; it must implement its own carrier
            // projection before the backend's LogicalUnion continuation closes.
            return (index == 0).then(|| self.clone());
        }
        self.ty.get_result_slot_type(index).map(Self::plain)
    }
}
