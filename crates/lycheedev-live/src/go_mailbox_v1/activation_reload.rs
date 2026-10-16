//! Included as connection::activation_reload. Explicit reload preserves the
//! original durable request and authenticated complete history before input.
#![forbid(unsafe_code)]
use super::*;
use crate::go_mailbox_v1::{
    durable_journal::activation_history::historical_reload, state::ReloadPhase,
};
fn present_reload(mut result: ProjectResult, request: &str) -> ProjectResult {
    result.operation.clear();
    result.operation_state.clear();
    result.report = None;
    result.report_origin = None;
    result.report_state = "unavailable".into();
    result.cleanup = "none".into();
    result.cleanup_method.clear();
    result.complete = result
        .reload
        .as_ref()
        .is_some_and(|r| r.request == request && r.phase == ReloadPhase::Complete)
        && (result.bound || result.closed);
    result.continuation.goal = "reload".into();
    result.continuation.request_id = request.into();
    if result.complete {
        result.continuation.kind = "completed".into();
        result.continuation.blocker = None;
        result.continuation.remaining_budget_ms = None;
        if !result.closed {
            result.stage = "connected".into();
        }
    }
    result
}
impl<E: ConnectionEnvironment, C: Clock, T: TokenSource, S: ContinuationScheduler>
    ConnectionService<E, C, T, S>
{
    pub fn reload(
        &mut self,
        id: &str,
        request: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        if let Err(e) = validate_request(request) {
            return ServiceOutcome::failure(e);
        }
        // Explicit reload reads the current log before history lookup. Torn tails
        // are evidence for an explicit resume, never an implicit repair here.
        let before = match self.book.status(id, control) {
            Ok(v) => v,
            Err(e) => return ServiceOutcome::failure(e),
        };
        match historical_reload(&before.paths, request, control) {
            Ok(Some(state))
                if state
                    .reload
                    .as_ref()
                    .is_some_and(|r| r.phase == ReloadPhase::Complete) =>
            {
                return ServiceOutcome::Completed(present_reload(
                    present(
                        &state,
                        before.paths.log_path(),
                        self.clock.unix_millis(),
                        "",
                        "",
                    ),
                    request,
                ));
            }
            Err(e) => return ServiceOutcome::failure(e),
            _ => (),
        }
        let (record, mut journal) = match self.book.acquire(id, control) {
            Ok(v) => v,
            Err(e) => return self.acquired(&before, e),
        };
        // Recheck history after acquiring the exact driver lease. Another caller
        // may have finished this request and then started a different reload.
        match historical_reload(&record.paths, request, control) {
            Ok(Some(state))
                if state
                    .reload
                    .as_ref()
                    .is_some_and(|r| r.phase == ReloadPhase::Complete) =>
            {
                let mut o = ServiceOutcome::Completed(present_reload(
                    present(
                        &state,
                        record.paths.log_path(),
                        self.clock.unix_millis(),
                        "",
                        "",
                    ),
                    request,
                ));
                o = o.join(channel(journal.close()));
                return o;
            }
            Err(e) => {
                let mut o = ServiceOutcome::with_result(
                    present_reload(self.project(&record, "", ""), request),
                    Err(e.into()),
                );
                o = o.join(channel(journal.close()));
                return o;
            }
            _ => (),
        }
        let mut state = record.state.clone();
        let validated = if state.closed {
            Err(error("live.channel_closed"))
        } else {
            state.validate().map_err(ChannelError::from).and_then(|_| {
                super::super::recovery::synchronize_automatic_budget(&mut state).map(|_| ())
            })
        };
        if let Err(e) = validated {
            let mut o = ServiceOutcome::with_result(
                present_reload(self.project(&record, "", ""), request),
                Err(e.into()),
            );
            o = o.join(channel(journal.close()));
            return o;
        }
        let backend = match self.environment.open(
            &record.metadata.target,
            Some(&record.metadata.owner),
            control,
        ) {
            Ok(v) => v,
            Err(e) => {
                let mut o = ServiceOutcome::with_result(
                    present_reload(self.project(&record, "", ""), request),
                    Err(e.into()),
                );
                o = o.join(channel(journal.close()));
                return o;
            }
        };
        let mut driver = Driver::from_state(
            state,
            backend,
            journal,
            BorrowClock(&self.clock),
            BorrowTokens(&mut self.tokens),
        )
        .expect("validated before resource transfer");
        let result = (|| {
            if driver
                .state
                .reload
                .as_ref()
                .is_none_or(|r| r.request != request)
            {
                driver.request_reload(control, request)?;
            }
            driver.continue_or_retire(
                control,
                RetirementTarget {
                    process_id: record.metadata.target.window.process_id,
                    process_started_at: record.metadata.target.window.process_started_at,
                },
                false,
                &mut self.scheduler,
            )
        })();
        let retire = result.is_ok() && driver.state.closed;
        let mut outcome = ServiceOutcome::with_result(
            present_reload(
                present(
                    &driver.state,
                    record.paths.log_path(),
                    self.clock.unix_millis(),
                    &driver.waiting,
                    driver.backend.scans(),
                ),
                request,
            ),
            channel(result),
        );
        outcome = outcome.join(channel(driver.backend.checked_close()));
        let closed = driver.journal.close();
        let released = closed.is_ok();
        outcome = outcome.join(channel(closed));
        if retire && released {
            outcome = outcome.join(self.book.retire_closed(id, control).map(|_| ()));
        }
        outcome
    }
}
