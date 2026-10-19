//! Borrow the already-admitted workspace while the original local CASC source
//! owns its authenticated index observations through scans and publication.
use super::{FileScope, Scope};
use crate::{
    blte::DecodePolicy,
    local_query::{File, sql_reader::Reader},
    resource::Budget,
};
use lycheedev_core::{Invocation, Result, selection::DataPin};
use std::sync::Arc;

pub(super) struct Session<'a, 'scope> {
    scope: &'a Scope<'scope>,
    pin: &'a DataPin,
    reader: Reader<'a>,
}
impl<'a, 'scope> Session<'a, 'scope> {
    pub(super) fn new(scope: &'a Scope<'scope>, pin: &'a DataPin) -> Result<Self> {
        Ok(Self {
            scope,
            pin,
            reader: Reader::new(scope.invocation, pin, scope.budget)?,
        })
    }
    pub(super) fn close_after<T>(self, result: Result<T>) -> Result<T> {
        self.reader.close_after(result)
    }
}
impl FileScope for Session<'_, '_> {
    fn reserve_derived_read<'reservation>(
        &'reservation self,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReadLease<'reservation>> {
        self.check()?;
        self.scope.admitted.reserve_derived_read(check)
    }
    fn reserve_derived_read_owned<'reservation>(
        &'reservation self,
        check: &'reservation lycheedev_core::workspace::data_disk::DerivedReadCheck,
    ) -> Result<lycheedev_core::workspace::data_disk::OwnedDerivedReadLease<'reservation>> {
        self.check()?;
        self.scope.admitted.reserve_derived_read_owned(check)
    }
    fn reserve_derived_cache<'reservation>(
        &'reservation self,
        peak: lycheedev_core::workspace::data_disk::AdditionalPeak,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReservation<'reservation>> {
        self.check()?;
        self.scope.admitted.reserve_derived_cache(peak, check)
    }
    fn check(&self) -> Result<()> {
        self.scope.check()?;
        self.reader.check_source()
    }
    fn verify_source(&self) -> Result<()> {
        self.scope.check()?;
        self.reader.verify_source()
    }
    fn check_consumer(&self, invocation: &Invocation, budget: &Arc<Budget>) -> Result<()> {
        self.scope.check_consumer(invocation, budget)?;
        self.reader.check_source()
    }
    fn check_pin(&self, pin: &DataPin) -> Result<()> {
        self.check()?;
        if pin != self.pin {
            return Err(crate::fault(
                "records.file_query",
                4,
                "Prepared session pin differs",
            ));
        }
        Ok(())
    }
    fn resolve_pin(&self) -> Result<lycheedev_core::selection::PinnedSet> {
        self.check()?;
        self.scope.resolve_pin()
    }
    fn read_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.check_pin(pin)?;
        self.reader
            .read_batches(ids, DecodePolicy::Strict, callback)
    }
    fn read_table_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.check_pin(pin)?;
        self.reader
            .read_batches(ids, DecodePolicy::TableAllowMissing, callback)
    }
}
