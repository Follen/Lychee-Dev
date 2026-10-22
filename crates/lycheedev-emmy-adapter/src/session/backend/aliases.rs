//! Alias projection retaining selected identity through the public type algebra.
use super::{OwnedQuery, QueryResult};
use crate::session::OwnedValue;
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn expand_aliases(&mut self, value: OwnedValue) -> QueryResult<OwnedValue, E> {
        self.expand_owned_aliases(value)
    }
}
