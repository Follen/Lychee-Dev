//! Own preparation, admission, capture and consuming close around the row loop.
use super::{
    Projection, StreamBegin, StreamEvent, StreamRequest, StreamSummary, TableProof, execute, wire,
};
use crate::{capture, db2, fault, resource::Budget, table};
use lycheedev_core::{Result, evidence};
use std::{sync::Arc, time::Instant};

struct Emitter<'a> {
    sink: &'a mut dyn FnMut(StreamEvent<'_>) -> Result<()>,
    started: bool,
    write_failed: bool,
}
impl Emitter<'_> {
    fn emit(&mut self, event: StreamEvent<'_>) -> Result<()> {
        let begin = matches!(event, StreamEvent::Begin(_));
        let result = (self.sink)(event);
        if result.is_err() {
            self.write_failed = true;
        } else if begin {
            self.started = true;
        }
        result
    }
}

pub(super) fn run_after_finish(
    request: StreamRequest<'_>,
    sink: &mut dyn FnMut(StreamEvent<'_>) -> Result<()>,
    after_finish: &mut impl FnMut(Result<StreamSummary>) -> Result<StreamSummary>,
) -> Result<StreamSummary> {
    let inv = request.invocation;
    let budget = Arc::new(Budget::query_controlled(
        Instant::now() + request.timeout,
        inv.control.clone(),
    ));
    // Transitional provider adapter: always lend the original invocation and
    // the same Arc<Budget>. Do not rebuild either from the parsed request.
    let mut scope = crate::remote::provider::OpenedScope::open(inv, &budget)?.admit()?;
    let mut emitter = Emitter {
        sink,
        started: false,
        write_failed: false,
    };
    let mut committed_captures = Vec::new();
    let result = (|| {
        let pin = scope.resolve_pin()?;
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| fault("command.failed", 5, "records.data_pin_required"))?;
        scope.check_pin(data)?;
        let mut execute = |reader: &mut dyn crate::remote::provider::FileScope| {
            let db2::Prepared {
                pin,
                bundle,
                file,
                layout,
                definition,
                availability,
                limits,
            } = db2::prepare_scoped(
                inv,
                request.name,
                &budget,
                Some(&mut *reader),
                Some(pin.clone()),
            )?;
            let view = table::open_budgeted(
                &file.bytes,
                &definition,
                limits,
                &availability.skipped,
                &availability.ranges,
                budget.clone(),
            )?;
            let projection = Projection::resolve(request.fields, request.filter, &definition)?;
            let begin = StreamBegin {
                proof: TableProof {
                    content_verified: file.reading["contentVerified"] == true,
                    partial_content: file.reading["partialContent"].clone(),
                    missing: file.reading["missing"].clone(),
                    unavailable_partitions: serde_json::to_value(&availability.partitions)?,
                    key_source: file.reading["keySource"].clone(),
                    requested_name: request.name.to_owned(),
                    identity: bundle.identity,
                    layout_hash: format!("{:08X}", layout.layout_hash),
                    definition_sha256: definition.sha256,
                    content_key: file.reading["entry"]["contentKey"].clone(),
                    logical_rows: view.logical_count(),
                },
                snapshot: pin.id.clone(),
                source: file.reading["source"].clone(),
                projection,
                filter: request.filter_text.to_owned(),
                limit: request.limit,
            };
            budget.check()?;
            emitter.emit(StreamEvent::Begin(&begin))?;
            let walk = execute::execute_rows(
                &view,
                &begin.projection,
                request.filter,
                request.limit,
                &budget,
                &mut |event| emitter.emit(event),
            )?;
            // Fixed Go snapshots the DataContext after walking and before the
            // manifest capture. Frames intentionally have no memoryBudget.
            inv.control.check()?;
            reader.check_consumer(inv, &budget)?;
            reader.verify_source()?;
            let resources = budget.logical_snapshot()?;
            let memory_budget = budget.memory_snapshot()?;
            budget.check()?;
            let manifest = wire::manifest(&begin, walk);
            let raw =
                capture::json_bytes(&manifest, &budget, evidence::DEFAULT_CAPTURE_BYTES, || {
                    fault("vault.blob_limit", 4, "blob exceeds budget")
                })?;
            let capture = evidence::commit_capture_budgeted(
                &inv.home,
                evidence::CaptureInput {
                    bytes: &raw,
                    maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES,
                    media_type: "application/json",
                    provenance: wire::capture_provenance(request.name, &pin),
                    complete: walk.complete(),
                    truncated: walk.truncated,
                },
                &inv.control,
                budget.as_ref(),
            )?;
            // Publication is not rolled back by a later check, close or sink.
            committed_captures.push(capture);
            budget.check()?;
            let resource_usage = budget.snapshot()?;
            Ok(StreamSummary {
                begin,
                pin: pin.data.as_ref().expect("prepare checked DataPin").clone(),
                resources,
                memory_budget,
                resource_usage,
                walk,
                captures: Vec::new(),
            })
        };
        if request.cdn {
            scope.with_session(data, |reader| execute(reader))
        } else {
            scope.with_local_session(data, |reader| execute(reader))
        }
    })();
    // A terminal success promises the whole owned operation. All consuming
    // closes, including the optional session, precede the End event.
    let result = scope.finish(result);
    let result = after_finish(result);
    let result = match result {
        Ok(mut summary) => emitter.emit(StreamEvent::End(summary.walk)).map(|()| {
            summary.captures = std::mem::take(&mut committed_captures);
            summary
        }),
        Err(error) => Err(error),
    };
    match result {
        Ok(summary) => Ok(summary),
        Err(mut error) => {
            if !committed_captures.is_empty() {
                error = wire::with_committed_captures(error, committed_captures);
            }
            if emitter.started && !emitter.write_failed {
                let code = if error.exit_code == 7
                    || error.code == "command.deadline"
                    || error.code == "operation.deadline"
                {
                    "records.cancelled"
                } else {
                    &error.code
                };
                // Exactly one error marker, only after a successful Begin.
                // Never invoke a sink again once any sink invocation failed.
                if let Err(frame_error) = emitter.emit(StreamEvent::Error {
                    code,
                    message: &error.message,
                }) {
                    error = wire::with_marker_error(error, frame_error);
                }
            }
            Err(error)
        }
    }
}
