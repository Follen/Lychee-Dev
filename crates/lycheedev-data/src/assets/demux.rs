//! VP9 AVI use case. Every frame borrows the single authenticated input buffer;
//! all output names preflight before writing, with the manifest published last.
use crate::request::DataContext;
use super::{avi, fault, input, output};
use lycheedev_core::{
    Outcome, Result, ToolkitError,
    evidence::{self, CaptureInput},
    storage,
};
use serde_json::{Value, json};
mod wire;
const MANIFEST: &str = "demux-manifest.json";

#[derive(Default)]
struct Progress {
    captures: Vec<[u8; 68]>,
    published: Vec<String>,
}
impl Progress {
    fn before_capture(&mut self) -> Result<()> {
        self.captures
            .try_reserve(1)
            .map_err(|_| fault("records.metadata_limit", 3))
    }
    fn committed(&mut self, capture: &Value) {
        let mut id = [0; 68];
        id.copy_from_slice(
            capture["id"]
                .as_str()
                .expect("committed capture identity")
                .as_bytes(),
        );
        self.captures.push(id);
    }
    fn finish<T>(&self, result: Result<T>) -> Result<T> {
        result.map_err(|mut error| {
            if !self.captures.is_empty() || !self.published.is_empty() {
                let primary = error.clone();
                let ids: Vec<_> = self.captures.iter().map(|id| std::str::from_utf8(id).expect("capture ASCII")).collect();
                error.details = json!({"schema":"lycheedev.demux-committed-error.v1", "primary":primary,
                    "completedCaptures":ids,"publishedOutputs":self.published});
                if !ids.is_empty() { error.message.push_str("\ndemux captures already committed: "); error.message.push_str(&ids.join(", ")); }
                if !self.published.is_empty() { error.message.push_str("\ndemux outputs already published: "); error.message.push_str(&self.published.join(", ")); }
            }
            error
        })
    }
}
struct Meter<'a> {
    ctx: &'a DataContext,
    budget: Option<crate::resource::Budget>,
}
impl lycheedev_core::evidence::CaptureAccounting for Meter<'_> {
    fn evidence_domain(&self) -> lycheedev_core::metadata::EvidenceDomain {
        lycheedev_core::metadata::EvidenceDomain::Data
    }

    fn check(&self) -> Result<()> {
        self.ctx.control.check()?;
        self.budget
            .as_ref()
            .map_or(Ok(()), crate::resource::Budget::check)
    }
    fn admit_work(&self, n: usize) -> Result<()> {
        self.check()?;
        self.budget
            .as_ref()
            .map_or(Ok(()), |b| b.charge(crate::resource::Cost::work(n)))
    }
    fn admit_allocation(&self, n: usize) -> Result<()> {
        self.check()?;
        self.budget
            .as_ref()
            .map_or(Ok(()), |b| b.charge(crate::resource::Cost::metadata(n)))
    }
}

fn capture(
    ctx: &DataContext,
    bytes: &[u8],
    maximum: usize,
    media_type: &str,
    provenance: Value,
    complete: bool,
    truncated: bool,
    meter: &Meter<'_>,
) -> Result<Value> {
    evidence::commit_capture_budgeted(
        &ctx.home,
        CaptureInput {
            bytes,
            maximum_bytes: maximum,
            media_type,
            provenance,
            complete,
            truncated,
        },
        &ctx.control,
        meter,
    )
}
pub fn run(ctx: &DataContext, request: &super::request::DemuxRequest) -> Result<Outcome> {
    ctx.control.check()?;
    let maximum = request.maximum_bytes;
    let max_frames = request.max_frames;
    let directory = output::OutputDirectory::open_scoped(ctx, &request.output, request.overwrite)?;
    let mut progress = Progress::default();
    let result = (|| {
        directory.preflight_scoped(ctx, MANIFEST, None)?;
        match &request.input {
            super::request::DemuxInput::Path(path) => {
            let metadata =
                lycheedev_core::metadata::Metadata::open_controlled(&ctx.home, true, &ctx.control)?;
            let result = (|| {
                let (bytes, canonical) = input::local(ctx, path, maximum)?;
                progress.before_capture()?;
                let source_capture = capture(
                    ctx,
                    &bytes,
                    maximum,
                    "video/x-msvideo",
                    json!({"kind":"asset-demux-source","locator":format!("file:{canonical}"),"snapshot":""}),
                    true,
                    false,
                    &Meter { ctx, budget: None },
                )?;
                progress.committed(&source_capture);
                let source = json!({"kind":"file","input":path,"path":canonical,"bytes":bytes.len(),"sha256":source_capture["blob"]["sha256"]});
                consume(
                    ctx,
                    &directory,
                    &bytes,
                    source,
                    source_capture,
                    "",
                    "",
                    &path.to_string_lossy(),
                    &request.output.to_string_lossy(),
                    maximum,
                    max_frames,
                    request.allow_partial,
                    None,
                    &mut progress,
                )
            })();
            lycheedev_core::metadata::owned_write::finish_owned(result, metadata.close())
        }, super::request::DemuxInput::Casc { file_id } => {
            // Reserve the receipt slot before the input helper can commit it.
            progress.before_capture()?;
            input::with_casc_bytes(
                ctx,
                *file_id,
                maximum,
                &mut |input, budget| {
                    progress.committed(&input.capture);
                    let name = format!("casc:fdid:{}", *file_id);
                    let source = json!({"kind":"casc","input":name,"fileDataID":file_id,"snapshot":input.snapshot,"pin":input.reading["pin"],"contentKey":input.reading["entry"]["contentKey"],"encodingKey":input.reading["payloadEncodingKey"],"bytes":input.bytes.len(),"sha256":input.capture["blob"]["sha256"]});
                    consume(
                        ctx,
                        &directory,
                        &input.bytes,
                        source,
                        input.capture,
                        &input.snapshot,
                        &input.data_build,
                        &name,
                        &request.output.to_string_lossy(),
                        maximum,
                        max_frames,
                        request.allow_partial,
                        Some(budget),
                        &mut progress,
                    )
                },
            )
        }}
    })();
    let result = output::finish_closes(result, directory.close(), &ctx.control);
    progress.finish(result)
}
#[allow(clippy::too_many_arguments)]
fn consume(
    ctx: &DataContext,
    directory: &output::OutputDirectory,
    bytes: &[u8],
    source: Value,
    source_capture: Value,
    snapshot: &str,
    data_build: &str,
    input_name: &str,
    output_path: &str,
    maximum: usize,
    max_frames: usize,
    allow_partial: bool,
    budget: Option<&crate::resource::Budget>,
    progress: &mut Progress,
) -> Result<Outcome> {
    let meter = Meter {
        ctx,
        budget: budget.map(crate::resource::Budget::safety_only),
    };
    let parsed = avi::parse_scoped(
        &bytes,
        avi::Limits {
            max_frames,
            max_frame_bytes: 256 << 20,
            max_total_bytes: 1 << 30,
            allow_partial,
        },
        &ctx.control,
        budget,
    )?;
    // Frame maps, names, receipt clones and Vec slots remain live together.
    // No payload copy is included: every frame still borrows the input.
    evidence::CaptureAccounting::admit_allocation(
        &meter,
        parsed
            .frames
            .len()
            .checked_mul(8192)
            .ok_or_else(|| fault("records.metadata_limit", 3))?,
    )?;
    let names: Vec<String> = parsed
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| format!("frame-{:06}.{}", index + 1, frame.chunk))
        .collect();
    for name in &names {
        ctx.control.check()?;
        directory.preflight_scoped(ctx, name, budget)?;
    }
    directory.preflight_scoped(ctx, MANIFEST, budget)?;
    let mut captures = Vec::with_capacity(parsed.frames.len() + 2);
    captures.push(source_capture.clone());
    let mut frames = Vec::with_capacity(parsed.frames.len());
    for (index, frame) in parsed.frames.iter().enumerate() {
        ctx.control.check()?;
        progress.before_capture()?;
        let frame_capture = capture(
            ctx,
            frame.payload,
            256 << 20,
            "video/VP9",
            json!({"kind":"asset-demux-frame","locator":format!("{input_name}#frame={index}"),"snapshot":snapshot,"dataBuild":data_build}),
            true,
            false,
            &meter,
        )?;
        progress.committed(&frame_capture);
        let mut frame_info = json!({"index":index,"type":"key","timestamp":frame.timestamp,"duration":frame.duration,"size":frame.payload.len(),"chunk":frame.chunk,"offset":frame.offset,"output":names[index],"sha256":frame_capture["blob"]["sha256"],"blob":frame_capture["blob"],"capture":frame_capture["id"]});
        if !frame.sub_frames.is_empty() {
            frame_info["subFrames"] = serde_json::to_value(&frame.sub_frames)?;
        }
        frames.push(frame_info);
        captures.push(frame_capture);
    }
    let mut manifest = json!({"schema":"lycheedev.asset-demux.v1","input":input_name,"source":source,"output":output_path,"width":parsed.width,"height":parsed.height,"frameRate":parsed.frame_rate,"frameCount":parsed.frames.len(),"enumeratedFrames":parsed.enumerated_frames,"enumeratedBytes":parsed.enumerated_bytes,"frames":frames,"complete":!parsed.truncated,"truncated":parsed.truncated,"limits":{"maxInputBytes":maximum,"maxFrames":max_frames,"maxFrameBytes":256<<20,"maxOutputBytes":1u64<<30},"sourceCapture":source_capture["id"]});
    if !snapshot.is_empty() {
        manifest["snapshot"] = json!(snapshot);
    }
    if !parsed.truncation_reason.is_empty() {
        manifest["truncationReason"] = json!(parsed.truncation_reason);
    }
    if !parsed.warnings.is_empty() {
        manifest["warnings"] = serde_json::to_value(&parsed.warnings)?;
    }
    // Serialize with an admission bound before allocating a publication body.
    let body = wire::encode(&manifest, &meter)?;
    let source_id = source_capture["id"].as_str().expect("committed source");
    let required: Vec<_> = captures
        .iter()
        .skip(1)
        .chain(captures.first())
        .map(|capture| capture["id"].as_str().expect("committed capture"))
        .collect();
    progress.before_capture()?;
    let manifest_capture = evidence::commit_capture_budgeted_with_references(
        &ctx.home,
        CaptureInput {
            bytes: &body,
            maximum_bytes: 16 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"asset-demux-manifest","locator":input_name,"snapshot":snapshot,"dataBuild":data_build}),
            complete: !parsed.truncated,
            truncated: parsed.truncated,
        },
        &[source_id],
        &required,
        &ctx.control,
        &meter,
    )?;
    progress.committed(&manifest_capture);
    captures.insert(1, manifest_capture);
    // The canonical output directory, not workspace home, is the lock domain.
    // One original OS handle protects the whole set and completion marker.
    let _copy_buffer = budget.map(|b| b.reserve_retained(65536)).transpose()?;
    let lease = directory.lease(ctx, budget)?;
    let published = (|| {
        for name in &names {
            ctx.control.check()?;
            directory.preflight_scoped(ctx, name, budget)?;
        }
        directory.preflight_scoped(ctx, MANIFEST, budget)?;
        progress
            .published
            .try_reserve(names.len() + 1)
            .map_err(|_| fault("records.metadata_limit", 3))?;
        for (name, frame) in names.iter().zip(&parsed.frames) {
            let receipt = directory.path.join(name).to_string_lossy().into_owned();
            let mut receipt = Some(receipt);
            directory.publish_scoped(ctx, name, frame.payload, 1 << 30, budget, &mut |state| {
                if state == storage::StagedPublication::Published {
                    progress
                        .published
                        .push(receipt.take().expect("one publication"));
                }
            })?;
        }
        let receipt = directory.path.join(MANIFEST).to_string_lossy().into_owned();
        let mut receipt = Some(receipt);
        directory.publish_scoped(ctx, MANIFEST, &body, 16 << 20, budget, &mut |state| {
            if state == storage::StagedPublication::Published {
                progress
                    .published
                    .push(receipt.take().expect("one publication"));
            }
        })?;
        Ok(())
    })();
    lycheedev_core::metadata::owned_write::finish_owned(published, lease.close())?;
    let mut outcome = Outcome::new(manifest);
    outcome.complete = !parsed.truncated;
    outcome.selection = json!({"snapshot":snapshot});
    outcome.provenance = captures;
    if parsed.truncated {
        outcome.warnings.push(json!(
            "Demux output is truncated; inspect truncationReason and limits."
        ));
    }
    Ok(outcome)
}

#[cfg(test)]
mod scoped_tests {
    use super::*;
    #[test]
    fn actual_committed_capture_survives_late_primary_close_and_unknown_output() {
        let home = std::env::temp_dir().join(format!(
            "lychee-demux-facts-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let control = lycheedev_core::control::OperationControl::default();
        let committed = evidence::commit_capture_bounded(
            &home,
            CaptureInput {
                bytes: b"source",
                maximum_bytes: 6,
                media_type: "video/x-msvideo",
                provenance: json!({"kind":"asset-demux-source","locator":"file:fixture"}),
                complete: true,
                truncated: false,
            },
            &control,
        )
        .unwrap();
        let mut progress = Progress::default();
        progress.before_capture().unwrap();
        progress.committed(&committed);
        let primary = ToolkitError::new("records.metadata_limit", "actual operation stopped", 3);
        let mut published = primary.clone();
        published.details = json!({"schema":"lycheedev.asset-publication-error.v1","primary":primary,"publication":{"state":"unknown","path":"fixture-output"}});
        let joined = lycheedev_core::metadata::owned_write::finish_owned::<()>(
            Err(published),
            Err(ToolkitError::new("external.io", "injected-close", 5)),
        );
        let error = progress.finish(joined).unwrap_err();
        assert_eq!(error.code, "records.metadata_limit");
        assert_eq!(error.exit_code, 3);
        assert_eq!(error.details["completedCaptures"], json!([committed["id"]]));
        assert_eq!(error.details["publishedOutputs"], json!([]));
        assert_eq!(
            error.details["primary"]["details"]["primary"]["details"]["publication"]["state"],
            "unknown"
        );
        assert_eq!(
            error.details["primary"]["details"]["cleanup"]["message"],
            "injected-close"
        );
        assert_eq!(
            evidence::read_capture(&home, committed["id"].as_str().unwrap()).unwrap(),
            committed
        );
        std::fs::remove_dir_all(home).unwrap();
    }
}
