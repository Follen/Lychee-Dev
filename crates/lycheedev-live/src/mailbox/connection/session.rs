//! Checked ownership of one synchronous, exclusively leased service turn.
//!
//! Driver already owns its active state and ports. This boundary handles the
//! partial-open and terminal paths around it; it does not introduce a worker or
//! move a thread-affine native guard away from its acquiring thread.
use super::{
    Clock, ConnectionBackend, ConnectionBook, Driver, ServiceOutcome, State, TokenSource, channel,
    present_owned,
};
use crate::mailbox::{
    durable_journal::DurableChannelJournal,
    observations::{ChannelError, ChannelResult},
    recovery::synchronize_automatic_budget,
};
use lycheedev_core::control::OperationControl;
use std::path::Path;

pub(super) struct LeaseOnly;
pub(super) struct Native<B>(B);

/// An acquired journal is always present. Only attach can produce a native
/// session, so a runnable session cannot contain a missing native backend.
#[must_use = "finish the session to retain checked resource-close failures"]
pub(super) struct SessionOwner<B> {
    backend: B,
    journal: DurableChannelJournal,
}

impl SessionOwner<LeaseOnly> {
    pub(super) fn leased(journal: DurableChannelJournal) -> Self {
        Self {
            backend: LeaseOnly,
            journal,
        }
    }

    pub(super) fn attach<B: ConnectionBackend>(self, backend: B) -> SessionOwner<Native<B>> {
        SessionOwner {
            backend: Native(backend),
            journal: self.journal,
        }
    }

    pub(super) fn journal_mut(&mut self) -> &mut DurableChannelJournal {
        &mut self.journal
    }
}

// Private completion port: no new public backend/journal abstraction or Send
// bound. The underlying resource destructors remain the unwind fallback.
pub(super) trait CloseNative {
    fn close_native(&mut self) -> ChannelResult<()>;
}
impl CloseNative for LeaseOnly {
    fn close_native(&mut self) -> ChannelResult<()> {
        Ok(())
    }
}
impl<B: ConnectionBackend> CloseNative for Native<B> {
    fn close_native(&mut self) -> ChannelResult<()> {
        self.0.checked_close()
    }
}

impl<B: CloseNative> SessionOwner<B> {
    /// Consume the owner once. Native close always precedes journal release;
    /// neither a business failure nor a native close failure skips lease close.
    /// Keep the existing left-nested business/native/journal error tree.
    pub(super) fn finish(
        mut self,
        outcome: ServiceOutcome,
        should_retire: bool,
    ) -> SessionCompletion<B> {
        let outcome = outcome.join(channel(self.backend.close_native()));
        let released = self.journal.close();
        let can_retire = should_retire && released.is_ok();
        SessionCompletion {
            outcome: outcome.join(channel(released)),
            can_retire,
            _owner: self,
        }
    }
}

impl<B: ConnectionBackend> SessionOwner<Native<B>> {
    pub(super) fn run<C: Clock, T: TokenSource>(
        self,
        mut state: State,
        log: &Path,
        clock: C,
        tokens: T,
        action: impl FnOnce(&mut Driver<B, DurableChannelJournal, C, T>) -> ChannelResult<()>,
    ) -> SessionCompletion<Native<B>> {
        // Validate before Driver's consuming constructor can drop resources on
        // error. Budget synchronization commits neither field unless both are
        // valid, preserving the original failure projection and durable log.
        let checked = state
            .validate()
            .map_err(ChannelError::from)
            .and_then(|_| synchronize_automatic_budget(&mut state).map(|_| ()));
        if let Err(error) = checked {
            let projected =
                present_owned(state, log, clock.unix_millis(), "", self.backend.0.scans());
            return self.finish(
                ServiceOutcome::with_result(projected, Err(error.into())),
                false,
            );
        }

        let mut driver = Driver::from_state(state, self.backend.0, self.journal, clock, tokens)
            .expect("state and automatic budgets validated before resource transfer");
        let result = action(&mut driver);
        let should_retire = result.is_ok() && driver.state.closed;
        let projected = present_owned(
            driver.state,
            log,
            driver.clock.unix_millis(),
            &driver.waiting,
            driver.backend.scans(),
        );
        // Transfer the same handles back; never reconstruct a journal lease or
        // native backend after the action. Driver's durability and recovery
        // state machine remains the sole authority while it is running.
        SessionOwner {
            backend: Native(driver.backend),
            journal: driver.journal,
        }
        .finish(
            ServiceOutcome::with_result(projected, channel(result)),
            should_retire,
        )
    }
}

#[must_use = "return the completion or finish its authorized claim retirement"]
pub(super) struct SessionCompletion<B> {
    outcome: ServiceOutcome,
    can_retire: bool,
    // Keep the same destructor order as the service: checked close, optional
    // claim retirement, then ordinary resource fallback destruction.
    _owner: SessionOwner<B>,
}
impl<B> SessionCompletion<B> {
    pub(super) fn into_outcome(self) -> ServiceOutcome {
        self.outcome
    }

    /// Fixed behavior: a successful closed turn may retire after successful
    /// lease release even when native close reported an error. A failed lease
    /// close keeps the claim for a later explicit recovery invocation.
    pub(super) fn retire(
        self,
        book: &mut ConnectionBook,
        id: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        if self.can_retire {
            self.outcome
                .join(book.retire_closed(id, control).map(|_| ()))
        } else {
            self.outcome
        }
    }
}
