//! Captured original bytes and a manifest precede one atomic external publish.
use crate::request::DataContext;
use super::{image, input, output};
use crate::{capture, phase_profile::Phase};
use lycheedev_core::{
    Outcome, Result, ToolkitError,
    evidence::{self, CaptureInput},
    storage::Destination,
};
use serde_json::{Value, json};

/// Records only returned, committed identities. No error or successful callback
/// can replay a capture/output. Unknown commits without returned IDs stay unknown.
struct Progress {
    captures: [[u8; 68]; 3],
    count: usize,
    output_published: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            captures: [[0; 68]; 3],
            count: 0,
            output_published: false,
        }
    }
}
impl Progress {
    fn committed(&mut self, capture: &Value) {
        // Core's returned capture has already validated its immutable identity.
        let id = capture["id"].as_str().expect("committed capture identity");
        self.captures[self.count].copy_from_slice(id.as_bytes());
        self.count += 1;
    }
    fn finish<T>(&self, result: Result<T>) -> Result<T> {
        result.map_err(|mut error| {
            if self.count != 0 {
                let primary = error.clone();
                let ids: Vec<_> = self.captures[..self.count]
                    .iter()
                    .map(|id| std::str::from_utf8(id).expect("capture ID is ASCII"))
                    .collect();
                error.details = json!({"schema":"lycheedev.asset-committed-error.v1",
                    "primary":primary, "completedCaptures":ids,
                    "outputPublished":self.output_published});
                error
                    .message
                    .push_str("\nasset captures already committed: ");
                for (at, id) in self.captures[..self.count].iter().enumerate() {
                    if at != 0 {
                        error.message.push_str(", ");
                    }
                    error
                        .message
                        .push_str(std::str::from_utf8(id).expect("capture ID is ASCII"));
                }
            }
            if self.output_published {
                error.message.push_str("\nasset output already published");
            }
            // The complete prior typed error, including cleanup/publication,
            // survives under primary. Use existing JSON error.details for typed
            // committed facts; no new top-level envelope or JSONL fields.
            error
        })
    }
}
struct Options<'a> {
    encoding: &'a str,
    mipmap: usize,
    channels: String,
    pixels: usize,
    maximum: usize,
}
pub fn run(ctx: &DataContext, request: &super::request::ExportRequest) -> Result<Outcome> {
    ctx.control.check()?;
    let options = Options {
        encoding: request.encoding.as_str(), mipmap: request.mipmap,
        channels: request.channels.clone(), pixels: request.max_pixels, maximum: request.maximum_bytes,
    };
    let destination = output::destination_scoped(ctx, &request.output, request.overwrite)?;
    let mut progress = Progress::default();
    let result = input::with_casc_verified(ctx, request.file_id, &mut |source| {
        progress.committed(source.capture);
        publish(ctx, &destination, source, &options, &mut progress)
    });
    let result = output::finish_closes(result, destination.close(), &ctx.control);
    progress.finish(result)
}
fn publish(
    ctx: &DataContext,
    destination: &Destination,
    source: input::VerifiedView<'_>,
    options: &Options<'_>,
    progress: &mut Progress,
) -> Result<Outcome> {
    let Options {
        encoding,
        mipmap,
        channels,
        pixels,
        maximum,
    } = options;
    // Fixed Go image/capture/output work is outside FileQuery usage on both
    // sources. Keep actual Rust work/allocation in the same safety ledger.
    let safety = source.budget.safety_only();
    let meter = &safety;
    let encoded = if *encoding == "raw" {
        None
    } else {
        Some(image::encode(
            source.blob,
            encoding,
            *mipmap,
            channels,
            *pixels,
            *maximum,
            meter,
            &ctx.control,
        )?)
    };
    let source_id = source.capture["id"]
        .as_str()
        .expect("committed source identity");
    let artifact = if let Some(encoded) = &encoded {
        let capture_phase = meter.phase(Phase::ArtifactCapture);
        let artifact = evidence::commit_capture_budgeted_with_dependencies_and_metadata(
            &ctx.home,
            &mut *source.metadata,
            CaptureInput {
                bytes: &encoded.bytes,
                maximum_bytes: *maximum,
                media_type: &format!("image/{encoding}"),
                provenance: json!({"kind":"asset-image","locator":source.capture["id"],"snapshot":source.snapshot,"dataBuild":source.data_build}),
                complete: true,
                truncated: false,
            },
            &[source_id],
            &ctx.control,
            meter,
        )?;
        drop(capture_phase);
        progress.committed(&artifact);
        artifact
    } else {
        source.capture.clone()
    };
    let mut manifest = json!({"schema":"lycheedev.asset-export.v1","snapshot":source.snapshot,"encoding":encoding,"output":destination.path().to_string_lossy(),"content":artifact["blob"],"source":source.reading,"sourceCapture":source.capture});
    if let Some(encoded) = &encoded {
        manifest["image"] = serde_json::to_value(&encoded.info)?;
        manifest["artifactCapture"] = artifact.clone();
    }
    let body = capture::json_bytes(&manifest, meter, 1 << 20, || {
        ToolkitError::invalid("serialized JSON exceeds admission")
    })?;
    let artifact_id = artifact["id"]
        .as_str()
        .expect("committed artifact identity");
    let dependencies: &[&str] = if encoded.is_some() {
        &[source_id, artifact_id]
    } else {
        &[source_id]
    };
    let capture_phase = meter.phase(Phase::ManifestCapture);
    let capture = evidence::commit_capture_budgeted_with_dependencies_and_metadata(
        &ctx.home,
        &mut *source.metadata,
        CaptureInput {
            bytes: &body,
            maximum_bytes: 1 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"asset-export-manifest","locator":source.capture["id"],"snapshot":source.snapshot,"dataBuild":source.data_build}),
            complete: true,
            truncated: false,
        },
        dependencies,
        &ctx.control,
        meter,
    )?;
    drop(capture_phase);
    progress.committed(&capture);
    let bytes = artifact["blob"]["bytes"]
        .as_u64()
        .expect("committed artifact length");
    let output_phase = meter.phase(Phase::Output);
    let _copy_buffer = source.budget.reserve_retained(65536)?;
    let mut observed = |state| {
        if state == lycheedev_core::storage::StagedPublication::Published {
            progress.output_published = true;
        }
    };
    if let Some(encoded) = &encoded {
        output::publish_reader_scoped(
            ctx,
            destination,
            &mut std::io::Cursor::new(&encoded.bytes),
            bytes,
            *maximum,
            Some(source.budget),
            &mut observed,
        )?;
    } else {
        output::publish_reader_scoped(
            ctx,
            destination,
            source.blob,
            bytes,
            *maximum,
            Some(source.budget),
            &mut observed,
        )?;
    }

    drop(output_phase);
    let mut outcome = Outcome::new(manifest);
    outcome.selection = json!({"snapshot":source.snapshot});
    outcome.provenance = vec![source.capture.clone(), capture];
    if *encoding != "raw" {
        outcome.provenance.push(artifact);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests;
