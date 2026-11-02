//! Owned journal metadata without a whole-State JSON buffer or bulk code/report
//! clone. Only INPUT has a lossy custom DTO codec; keep that codec authoritative.
use super::*;

fn observation(value: &InputObservation) -> Result<InputObservation> {
    let invalid = || ContractError("live.channel_journal_invalid");
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

fn transaction(value: &Transaction) -> Result<Transaction> {
    let e = &value.envelope;
    Ok(Transaction {
        release_wait_started_at_ms: value.release_wait_started_at_ms,
        envelope: SlotEnvelope {
            schema: e.schema.clone(),
            index: e.index,
            start_slot: e.start_slot,
            runtime: e.runtime.clone(),
            owner: e.owner.clone(),
            fence: e.fence,
            nonce: e.nonce.clone(),
            ticket: e.ticket.clone(),
            action: e.action.clone(),
            guid: e.guid.clone(),
            build: e.build.clone(),
            code: CodeBytes::default(),
            code_bytes: e.code_bytes,
            code_checksum: e.code_checksum,
            budget: e.budget,
            challenge: e.challenge.clone(),
            prepared_nonce: e.prepared_nonce.clone(),
            report_bytes: e.report_bytes,
            report_checksum: e.report_checksum,
        },
        phase: value.phase,
        receipt: value.receipt.clone(),

        input_observation: value
            .input_observation
            .as_ref()
            .map(observation)
            .transpose()?,
    })
}

fn operation(value: &Operation) -> Result<Operation> {
    // The former full JSON roundtrip applied ResultBytes' read bound even to
    // an unvalidated State. Retain that error before any artifact is published.
    if value.result_bytes.0.len() > MAX_PAYLOAD {
        return Err(ContractError("live.channel_journal_invalid"));
    }
    Ok(Operation {
        recovery_budget: value.recovery_budget.clone(),
        cleanup_method: value.cleanup_method,
        attempt: value.attempt,
        request: value.request.clone(),
        origin: value.origin.clone(),
        id: value.id.clone(),
        ticket: value.ticket.clone(),
        code: CodeBytes::default(),
        budget: value.budget,
        policy: value.policy,
        stage: value.stage,
        prepared_nonce: value.prepared_nonce.clone(),
        challenge: value.challenge.clone(),
        report_bytes: value.report_bytes,
        report_checksum: value.report_checksum,
        result_bytes: ResultBytes::default(),
        legacy_result: (),
    })
}

fn input(value: &InputAttempt) -> Result<InputAttempt> {
    Ok(InputAttempt {
        id: value.id.clone(),
        exchange: value.exchange.clone(),
        runtime: value.runtime.clone(),
        kind: value.kind,
        after_millis: value.after_millis,
        recovery_after_millis: value.recovery_after_millis,

        observation: value.observation.as_ref().map(observation).transpose()?,
        outcome: value.outcome.clone(),
        // This is an ordinary diagnostic field, NOT an InputObservation's
        // filtered inputAttempt. Do not apply its nested codec here.
        after_diagnostic: value.after_diagnostic.clone(),
    })
}

fn replacement(value: &RuntimeReplacementProof) -> Result<RuntimeReplacementProof> {
    let w = &value.witness;
    Ok(RuntimeReplacementProof {
        schema: value.schema.clone(),
        process_id: value.process_id,
        process_started_at: value.process_started_at,
        current: value.current.clone(),
        witness: RuntimeReplacementWitness {
            first: observation(&w.first)?,
            first_sequence: w.first_sequence,
            address: w.address,
            length: w.length,
            before_sha256: w.before_sha256.clone(),
            after_sha256: w.after_sha256.clone(),
            sequence: w.sequence,
            observation: observation(&w.observation)?,
        },
    })
}

impl State {
    /// Same persisted metadata as JSON-copy then artifact extraction. Exhaustive
    /// struct literals make new persisted fields require an explicit decision.
    /// Code normalization/artifact deduplication is owned by journal::snapshot.
    pub(crate) fn snapshot_metadata(&self) -> Result<Self> {
        Ok(Self {
            progress_version: self.progress_version,
            blocker: self.blocker.clone(),
            connect_budget: self.connect_budget.clone(),
            close_budget: self.close_budget.clone(),
            process_end: self.process_end,
            runtime_end: self.runtime_end.as_ref().map(replacement).transpose()?,
            artifacts: BTreeMap::new(),
            archive: self.archive.clone(),
            schema: self.schema.clone(),
            id: self.id.clone(),
            owner: self.owner.clone(),
            identity: self.identity.clone(),
            bound: self.bound,
            closed: self.closed,
            transaction: self.transaction.as_ref().map(transaction).transpose()?,
            operation: self.operation.as_ref().map(operation).transpose()?,
            reload: self.reload.clone(),
            recovery: self
                .recovery
                .as_ref()
                .map(|r| -> Result<RuntimeRecovery> {
                    Ok(RuntimeRecovery {
                        from: r.from.clone(),
                        transaction: r.transaction.as_ref().map(transaction).transpose()?,
                        phase: r.phase,
                    })
                })
                .transpose()?,
            input: self.input.as_ref().map(input).transpose()?,
            closing: self.closing,
        })
    }
}
