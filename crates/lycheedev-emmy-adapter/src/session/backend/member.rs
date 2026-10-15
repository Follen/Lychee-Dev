//! Selected member provenance and the corresponding direct Object-index path.
//! General runtime owners and callable expansion continue on this same backend.
use super::{ExpressionValue, OwnedQuery, OwnedQueryCapability, QueryResult};
use crate::session::{BindingCoverage, FieldWriteOrigin, OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{FileId, LuaMemberKey, LuaType, SemanticId};

/// Member facts with the full owned value, including nested selected identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedMemberInfo {
    pub key: LuaMemberKey,
    pub typ: OwnedValue,
    pub id: Option<SemanticId>,
    pub file_id: Option<FileId>,
    pub is_method: bool,
}

impl<E> OwnedQuery<'_, '_, E> {
    /// The selected-object member producer. None means this value is not a
    /// directly selected object; it is not an empty member list or permission
    /// to erase a nested carrier through the upstream member query.
    /// Every call really validates/enumerates again, including cache hits.
    pub fn selected_members(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<Vec<OwnedMemberInfo>>, E> {
        self.checkpoint()?;
        let Some(metadata) = value.object_info() else {
            return Ok(None);
        };
        if !value.object_belongs_to(self.session) {
            self.member_receipt(value, "unavailable", Some("object_context_mismatch"))?;
            return Ok(Some(Vec::new()));
        }
        // Clone the existing bounded shape handle under the one session lock.
        // No provider requery, new freeze, second database or ledger snapshot.
        let frozen = self
            .session
            .shapes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(metadata.view)
            .cloned();
        self.checkpoint()?;
        let Some(frozen) = frozen else {
            return Ok(Some(Vec::new()));
        };
        if frozen.object.identity != metadata.identity || frozen.object.epoch != metadata.epoch {
            self.member_receipt(
                value,
                "unavailable",
                Some("object_frozen_identity_mismatch"),
            )?;
            return Ok(Some(Vec::new()));
        }
        let fields = value
            .carried_fields()
            .expect("owned object carries its fields");
        let mut members = Vec::with_capacity(frozen.shape.fields.len());
        // Producer order is the frozen provider order, never carrier BTreeMap order.
        for field in &frozen.shape.fields {
            self.checkpoint()?;
            let key = LuaMemberKey::Name(field.key.clone().into());
            let Some(typ) = fields.get(&key) else {
                continue;
            };
            let (file_id, id) = match &field.destination_write {
                FieldWriteOrigin::Xml(_) => (None, None),
                FieldWriteOrigin::Lua(span) => {
                    let mut found = None;
                    if let Some(facts) = self.model.file_facts_of(span.file_id) {
                        for member in &facts.members {
                            self.checkpoint()?;
                            if member.key == key
                                && member.id.member_key_range().is_some_and(|range| {
                                    span.range.start <= u32::from(range.start())
                                        && u32::from(range.end()) <= span.range.end
                                })
                            {
                                found = Some(member.id.clone());
                                break;
                            }
                        }
                    }
                    (Some(span.file_id), found)
                }
            };
            members.push(OwnedMemberInfo {
                key,
                typ: typ.clone(),
                id,
                file_id,
                is_method: matches!(typ.structural_type(), LuaType::DocFunction(f) if f.is_colon_define()),
            });
        }
        self.member_receipt(
            value,
            if metadata.complete {
                "bound"
            } else {
                "unavailable"
            },
            (!metadata.complete).then_some("object_shape_incomplete"),
        )?;
        self.checkpoint()?;
        Ok(Some(members))
    }

    fn member_receipt(
        &mut self,
        value: &OwnedValue,
        state: &'static str,
        reason: Option<&'static str>,
    ) -> QueryResult<(), E> {
        let metadata = value.object_info().expect("selected member receipt");
        self.session.record(
            BindingCoverage {
                site: metadata.site,
                operation: "member_lookup",
                state,
                proof: metadata.proof.into(),
                reason: reason.map(Into::into),
                parameter: None,
                parameter_index: None,
                argument: None,
            },
            || self.checkpoint(),
        )
    }

    pub(super) fn index_selected_member(
        &mut self,
        owner: ExpressionValue<OwnedValue>,
        key: &LuaMemberKey,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        if matches!(owner.value.structural_type(), LuaType::Any) {
            return Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Any)));
        }
        if owner.value.object_info().is_none() || owner.owner.is_some() {
            return self.continuation(OwnedQueryCapability::IndexMember, None, None, None);
        }
        // Old VM order: member_type fallback, all callable member candidates,
        // then member_info for the method-owner decision. Do not coalesce these
        // real queries just because their eventual receipts deduplicate.
        let fallback = self
            .selected_members(&owner.value)?
            .unwrap()
            .into_iter()
            .find(|info| &info.key == key)
            .map(|info| info.typ);
        let candidates = self.selected_members(&owner.value)?.unwrap();
        for info in candidates.iter().filter(|info| &info.key == key) {
            self.checkpoint()?;
            // Identity-level callable recovery can infer a closure regardless
            // of its current projected type. Alias/union/generic expansion can
            // also generate several candidates. Neither may be skipped.
            if info.id.is_some()
                || matches!(
                    info.typ.structural_type(),
                    LuaType::Union(_)
                        | LuaType::Intersection(_)
                        | LuaType::Ref(_)
                        | LuaType::Def(_)
                        | LuaType::Generic(_)
                )
            {
                return self.continuation(OwnedQueryCapability::MemberCallable, None, None, None);
            }
            // One unique object field with no source member identity has zero
            // candidates, or exactly its one DocFunction; no overload union.
        }
        let method = self
            .selected_members(&owner.value)?
            .unwrap()
            .into_iter()
            .find(|info| &info.key == key);
        debug_assert!(method.as_ref().is_none_or(|info| info.id.is_none()));
        if let Some(ty) = fallback {
            // The broad Function branch performs one additional member_info
            // lookup before the generic-method recovery check sees no identity.
            if matches!(ty.structural_type(), LuaType::Function) {
                let _ = self.selected_members(&owner.value)?;
            }
            self.checkpoint()?;
            return Ok(ExpressionValue {
                value: ty,
                owner: None,
                closure_syntax: None,
                receiver: Some(owner.value),
            });
        }
        // The fixed VM treats a missing Object key as Nil even when the shape
        // is incomplete. The member_lookup receipt retains that uncertainty.
        Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Nil)))
    }

    pub(super) fn index_selected_dynamic(
        &mut self,
        owner: ExpressionValue<OwnedValue>,
        key: ExpressionValue<OwnedValue>,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        match key.value.structural_type() {
            LuaType::StringConst(s) | LuaType::DocStringConst(s) => {
                self.index_selected_member(owner, &LuaMemberKey::Name(s.as_str().into()))
            }
            LuaType::IntegerConst(i) | LuaType::DocIntegerConst(i) => {
                self.index_selected_member(owner, &LuaMemberKey::Integer(*i))
            }
            LuaType::Union(_) | LuaType::Ref(_) | LuaType::Def(_) => {
                self.continuation(OwnedQueryCapability::IndexDynamic, None, None, None)
            }
            _ if owner.value.object_info().is_some() => {
                // Dynamic String/Number on Object does not enumerate fields in
                // the fixed VM. Other nonliteral keys also produce Unknown.
                Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Unknown)))
            }
            _ => self.continuation(OwnedQueryCapability::IndexDynamic, None, None, None),
        }
    }
}
