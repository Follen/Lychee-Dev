//! Captured original bytes and a manifest precede one atomic external publish.
use super::{fault, image, input, output};
use crate::capture;
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    evidence::{self, CaptureInput},
    storage::Destination,
};
use serde_json::{Value, json};
use std::path::Path;

fn number(inv: &Invocation, flag: &str, default: usize) -> Result<usize> {
    inv.flag(flag)
        .map(str::parse)
        .transpose()
        .map_err(|_| ToolkitError::arguments(format!("invalid {flag}")))
        .map(|value| value.unwrap_or(default))
}
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
pub fn run(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    if inv.flag_bool("--cdn") && inv.flag("--installation").is_some() {
        return Err(ToolkitError::arguments(
            "choose either --cdn or --installation, not both",
        ));
    }
    let id = inv
        .flag("--file-id")
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))?
        .unwrap_or(0);
    if (!inv.flag_bool("--cdn")
        && inv
            .flag("--installation")
            .filter(|v| !v.is_empty())
            .is_none())
        || inv.flag("--snapshot").filter(|v| !v.is_empty()).is_none()
        || id == 0
    {
        return Err(ToolkitError::arguments(
            "asset export requires --snapshot, --file-id, and either --installation or --cdn",
        ));
    }
    let output_path = inv
        .flag("--output")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ToolkitError::arguments("asset export requires --output"))?;
    let encoding = inv
        .flag("--encoding")
        .filter(|value| !value.is_empty())
        .unwrap_or("raw");
    let mipmap = number(inv, "--mipmap", 0)?;
    let channels = inv.flag("--channels").unwrap_or("");
    let pixels = number(inv, "--max-pixels", 0)?;
    if encoding == "raw" && (mipmap != 0 || !channels.is_empty() || pixels != 0)
        || !["raw", "png", "webp"].contains(&encoding)
        || mipmap > 15
        || encoding != "raw" && pixels > 64 << 20
        || channels.chars().any(|channel| !"rgba".contains(channel))
        || channels
            .chars()
            .any(|channel| channels.matches(channel).count() != 1)
    {
        return Err(fault("records.export_options", 2));
    }
    let destination = output::destination(
        &inv.home,
        Path::new(output_path),
        inv.flag_bool("--overwrite"),
    )?;
    let options = Options {
        encoding,
        mipmap,
        maximum: inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?,
        pixels: if pixels == 0 { 16 << 20 } else { pixels },
        channels: if channels.is_empty() {
            "rgba".into()
        } else {
            "rgba"
                .chars()
                .filter(|name| channels.contains(*name))
                .collect()
        },
    };
    let mut progress = Progress::default();
    let result = if inv.flag_bool("--cdn") {
        input::with_remote_casc_verified(inv, id, &mut |source| {
            progress.committed(source.capture);
            publish(inv, &destination, source, &options, &mut progress)
        })
    } else {
        // Preserve the existing local source selection/verified body path; only
        // borrow its bytes for the shared export consumer and checked-close it.
        match input::capture_casc_verified(inv) {
            Err(error) => Err(error),
            Ok(mut source) => {
                progress.committed(&source.capture);
                let result = publish(
                    inv,
                    &destination,
                    input::VerifiedView {
                        blob: &mut source.blob,
                        reading: &source.reading,
                        capture: &source.capture,
                        snapshot: &source.snapshot,
                        data_build: &source.data_build,
                        budget: &source.budget,
                    },
                    &options,
                    &mut progress,
                );
                source.blob.close_after(result)
            }
        }
    };
    // The remote callback returns before VerifiedBlob, Session and admission
    // checked-close. Destination still has its inherited native Drop boundary.
    progress.finish(result)
}
fn publish(
    inv: &Invocation,
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
    // Fixed Go export image/capture/output work is outside FileQuery usage.
    // Keep the existing local accounting behavior until its own admitted path
    // is migrated; remote uses the same shared safety ledger without Go units.
    let safety = source.budget.safety_only();
    let meter = if inv.flag_bool("--cdn") {
        &safety
    } else {
        source.budget.as_ref()
    };
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
            &inv.control,
        )?)
    };
    let source_id = source.capture["id"]
        .as_str()
        .expect("committed source identity");
    let artifact = if let Some(encoded) = &encoded {
        let artifact = evidence::commit_capture_budgeted_with_dependencies(
            &inv.home,
            CaptureInput {
                bytes: &encoded.bytes,
                maximum_bytes: *maximum,
                media_type: &format!("image/{encoding}"),
                provenance: json!({"kind":"asset-image","locator":source.capture["id"],"snapshot":source.snapshot,"dataBuild":source.data_build}),
                complete: true,
                truncated: false,
            },
            &[source_id],
            &inv.control,
            meter,
        )?;
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
    let capture = evidence::commit_capture_budgeted_with_dependencies(
        &inv.home,
        CaptureInput {
            bytes: &body,
            maximum_bytes: 1 << 20,
            media_type: "application/json",
            provenance: json!({"kind":"asset-export-manifest","locator":source.capture["id"],"snapshot":source.snapshot,"dataBuild":source.data_build}),
            complete: true,
            truncated: false,
        },
        dependencies,
        &inv.control,
        meter,
    )?;
    progress.committed(&capture);
    let bytes = artifact["blob"]["bytes"]
        .as_u64()
        .expect("committed artifact length");
    let _copy_buffer = source.budget.reserve_retained(65536)?;
    meter.charge(crate::resource::Cost::work(bytes.div_ceil(65536) as usize))?;
    if let Some(encoded) = &encoded {
        output::publish(inv, destination, &encoded.bytes, *maximum)?;
    } else {
        output::publish_reader(inv, destination, source.blob, bytes, *maximum)?;
    }
    progress.output_published = true;
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
