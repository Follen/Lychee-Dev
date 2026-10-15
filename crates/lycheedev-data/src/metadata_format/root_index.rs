//! The authenticated Root owner travels with its compact immutable group index.
use super::*;
use crate::resource::{Budget, Cost, OwnedRetainedReservation};
use std::sync::Arc;
pub(super) struct Group {
    pub ids: Vec<u32>,
    pub keys_start: usize,
    pub names_start: usize,
    pub key_stride: usize,
    pub has_name: bool,
    pub flags: u32,
    pub locale: u32,
    pub group: usize,
    pub _allocation: OwnedRetainedReservation,
}
pub(crate) trait RootContent {
    fn content(&self) -> &Content;
}
pub(crate) struct RootMatches {
    records: Vec<RootRecord>,
    _allocation: OwnedRetainedReservation,
}
impl std::ops::Deref for RootMatches {
    type Target = [RootRecord];
    fn deref(&self) -> &[RootRecord] {
        &self.records
    }
}
pub(crate) struct RootIndex<C: RootContent> {
    groups: Vec<Group>,
    content: C,
    limits: RootLimits,
    budget: Arc<Budget>,
}
impl<C: RootContent> RootIndex<C> {
    pub(crate) fn open(
        content: C,
        limits: RootLimits,
        budget: Arc<Budget>,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        let mut groups = Vec::new();
        scan_root(
            content.content(),
            &[],
            limits,
            None,
            check,
            Some(&mut groups),
            Some(&budget),
        )?;
        Ok(Self {
            groups,
            content,
            limits,
            budget,
        })
    }
    pub(crate) fn lookup(
        &self,
        id: u32,
        mut supplemental: impl FnMut() -> Result<()>,
    ) -> Result<RootMatches> {
        supplemental()?;
        self.budget.check()?;
        let _scratch = self
            .budget
            .reserve_logical_scratch(product(self.limits.matches, 256)? as u64)?;
        let allocation = self
            .budget
            .reserve_retained(product(self.limits.matches, 256)?)?;
        let content = self.content.content();
        let mut result = Vec::new();
        for group in &self.groups {
            supplemental()?;
            self.budget.charge(Cost::work(1))?;
            let at = group.ids.partition_point(|value| *value < id);
            if group.ids.get(at) != Some(&id) {
                continue;
            }
            if result.len() >= self.limits.matches {
                return Err(exceeded());
            }
            let key =
                content.checked_span(group.keys_start + at * group.key_stride, group.key_stride)?;
            let name_hash = if group.has_name {
                let raw = if group.key_stride == 24 {
                    &key[16..24]
                } else {
                    content.checked_span(group.names_start + at * 8, 8)?
                };
                u64::from_le_bytes(raw.try_into().unwrap())
            } else {
                0
            };
            result.push(RootRecord {
                file_data_id: id,
                content_key: casc::hex(&key[..16]),
                locale_mask: group.locale,
                content_flags: group.flags,
                name_hash,
                has_name_hash: group.has_name,
                group: group.group,
            });
        }
        supplemental()?;
        self.budget.check()?;
        Ok(RootMatches {
            records: result,
            _allocation: allocation.into_owned(&self.budget)?,
        })
    }
}
