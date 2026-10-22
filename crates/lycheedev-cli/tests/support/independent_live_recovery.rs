//! Real durable Live ownership/journal with synthetic authenticated-host input.
//! No native backend, game process, input or recovery execution is claimed.
use lycheedev_core::control::OperationControl;
use lycheedev_live::mailbox::{
    Identity, SCHEMA, checksum,
    admission::{ClientRecord, ConnectionBook, ConnectionMetadata, ConnectionTarget, WindowRecord, inspect_window_owner},
    connection::{SystemClock, SystemTokens, present},
    driver::Clock,
    durable_journal::{DurableChannelJournal, historical_request},
    journal::{StateJournal, save_state},
    recovery::continuation,
    state::{DurableBudget, Operation, OperationPolicy, OperationStage, ResultBytes, State},
};
use std::{fs, path::Path};

pub(crate) struct RetainedLive {
    book: ConnectionBook,
    journal: DurableChannelJournal,
    original: State,
    metadata: ConnectionMetadata,
    log_bytes: Vec<u8>,
    now: i64,
}
impl RetainedLive {
    pub(crate) fn create(project: &Path, installation: &Path) -> Self {
        let control = OperationControl::default();
        let mut book = ConnectionBook::open(project).unwrap();
        let identity = Identity {
            schema: SCHEMA.into(), runtime: "2".repeat(32), next_slot: 1, slots: 500,
            input_state: SCHEMA.into(), character: "Offline fixture".into(), realm: "Fixture".into(),
            guid: "fixture-guid".into(), build: "12.1.0.69933".into(), product: "retail".into(),
            release: lycheedev_core::VERSION.into(), ..Default::default()
        };
        // ConnectionBook models the persistence boundary after host admission.
        // These fixed target fields are synthetic and never authorize native IO.
        let target = ConnectionTarget {
            client: ClientRecord { directory: installation.to_string_lossy().into_owned(), product: "retail".into(), product_code: "wow".into(), full_build: identity.build.clone(), ..Default::default() },
            window: WindowRecord { process_id: 17, process_started_at: 18, handle: 19, ..Default::default() },
        };
        fs::create_dir_all(target.addon_parent()).unwrap();
        let record = book.create(target, identity, &SystemClock, &mut SystemTokens, &control).unwrap();
        let (record, mut journal) = book.acquire(&record.state.id, &control).unwrap();
        let now = SystemClock.unix_millis();
        let mut state = record.state;
        state.bound = true;
        state.connect_budget = None;
        state.identity.owner = state.owner.clone();
        state.identity.fence = 1;
        let mut operation = Operation::prepared(
            format!("LMO-{}", "3".repeat(32)), "4".repeat(32), "return 'retained report'".into(), 30, OperationPolicy::Opaque,
        );
        operation.request = "independent-owner-original-request".into();
        operation.stage = OperationStage::ResultVerified;
        operation.prepared_nonce = "5".repeat(32);
        operation.challenge = "6".repeat(32);
        operation.result_bytes = ResultBytes(b"{\"executionCount\":1,\"value\":\"retained report\"}".to_vec());
        operation.report_bytes = operation.result_bytes.0.len() as u32;
        operation.report_checksum = checksum(&operation.result_bytes.0);
        operation.recovery_budget = Some(DurableBudget { started_at_ms: now, deadline_ms: now + 30000, last_observed_ms: now, ..Default::default() });
        state.operation = Some(operation);
        save_state(&mut journal, &control, &mut state, "result_verified").unwrap();
        journal.write_result(&control, state.operation.as_ref().unwrap()).unwrap();
        let log_bytes = fs::read(record.paths.log_path()).unwrap();
        let retained = Self { book, journal, original: state, metadata: record.metadata, log_bytes, now };
        retained.assert_preserved();
        retained
    }
    pub(crate) fn assert_preserved(&self) {
        let control = OperationControl::default();
        let record = self.book.status(&self.original.id, &control).unwrap();
        assert_eq!(record.metadata, self.metadata);
        assert_eq!(record.state, self.original, "original CON/request/report/budget must survive Data failure");
        assert_eq!(self.journal.load(&control).unwrap(), self.original);
        let request = &self.original.operation.as_ref().unwrap().request;
        let historical = historical_request(&record.paths, request, &control).unwrap().unwrap();
        assert_eq!(historical, self.original);
        let continued = continuation(&historical, self.now);
        assert_eq!(continued.session, self.original.id);
        assert_eq!(continued.request_id, *request);
        assert_eq!(continued.goal, "operation");
        assert_eq!(continued.remaining_budget_ms, Some(30000));
        let projected = present(&historical, record.paths.log_path(), self.now, "", "");
        assert_eq!(projected.operation_state, "result_verified");
        assert!(!projected.complete, "a verified report still needs original cleanup");
        assert_eq!(projected.report.as_ref().unwrap().0, self.original.operation.as_ref().unwrap().result_bytes.0);
        let owner = inspect_window_owner(&record.metadata.target.addon_parent(), &self.metadata.owner.resource, &control);
        assert!(owner.error.is_none(), "{:?}", owner.error);
        assert!(owner.occupied);
        assert_eq!(owner.owner.as_ref(), Some(&self.metadata.owner));
        match self.book.acquire(&self.original.id, &control) {
            Err(error) => assert!(error.is_busy(), "{error:?}"),
            Ok((_record, mut second)) => { second.close().unwrap(); panic!("Data failure released the independent Live execution lease") }
        }
        // This is the actual chain behind all reads above, not an arbitrary file.
        assert_eq!(fs::read(record.paths.log_path()).unwrap(), self.log_bytes);
    }
    pub(crate) fn close(mut self) {
        self.assert_preserved();
        self.journal.close().unwrap();
        let (record, mut reacquired) = self.book.acquire(&self.original.id, &OperationControl::default()).unwrap();
        assert_eq!(record.state, self.original);
        reacquired.close().unwrap();
    }
}
