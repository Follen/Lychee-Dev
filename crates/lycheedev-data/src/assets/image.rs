//! Lossless image artifacts retain their allocations in the source request's
//! budget. Original BLP, encoded artifact and export manifest remain distinct.
use super::{fault, texture};
use crate::{blte::stream_io_error, resource::{Budget, Cost, RetainedReservation}};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, evidence::VerifiedBlob};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Info {
    texture: texture::Description,
    channels: String,
    #[serde(rename = "pixelsSHA256")]
    pixels_sha256: String,
}
pub(crate) struct Encoded<'a> {
    pub bytes: Vec<u8>,
    pub info: Info,
    _allocation: Option<RetainedReservation<'a>>,
}
struct Output<'a> {
    bytes: Vec<u8>,
    maximum: usize,
    budget: &'a Budget,
    allocation: Option<RetainedReservation<'a>>,
    error: Option<ToolkitError>,
}
impl Output<'_> {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.budget.check()?;
        if bytes.len() > self.maximum - self.bytes.len() {
            return Err(fault("records.image_output_limit", 3));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = required.max(self.bytes.capacity().saturating_mul(2)).max(65536).min(self.maximum);
            // The previous buffer can coexist with its replacement during a
            // realloc. Release that reservation only after allocation succeeds.
            let allocation = self.budget.reserve_retained(capacity)?;
            self.bytes.try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| fault("records.query_resource_budget", 3))?;
            self.allocation = Some(allocation);
        }
        for block in bytes.chunks(65536) {
            self.budget.charge(Cost::work(1))?;
            self.bytes.extend_from_slice(block);
        }
        Ok(())
    }
}
impl Write for Output<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(error) = &self.error { return Err(io::Error::other(error.clone())); }
        match self.append(bytes) {
            Ok(()) => Ok(bytes.len()),
            Err(error) => {
                self.error = Some(error.clone());
                Err(io::Error::other(error))
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> { self.budget.check().map_err(io::Error::other) }
}
fn png_error(error: png::EncodingError) -> ToolkitError {
    match error {
        png::EncodingError::IoError(error) => stream_io_error(error),
        error => ToolkitError::new("command.failed", error.to_string(), 5),
    }
}
fn png(output: &mut Output<'_>, pixels: &[u8], width: u32, height: u32) -> Result<()> {
    // StreamWriter retains current/previous/filter rows and deflate state.
    let _codec = output.budget.reserve_retained(width as usize * 4 * 3 + (1 << 20))?;
    let budget = output.budget;
    let mut encoder = png::Encoder::new(output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(png_error)?;
    {
        let mut stream = writer.stream_writer_with_size(65536).map_err(png_error)?;
        for block in pixels.chunks(65536) {
            budget.charge(Cost::work(1))?;
            stream.write_all(block).map_err(stream_io_error)?;
        }
        stream.finish().map_err(png_error)?;
    }
    writer.finish().map_err(png_error)
}
fn webp(output: &mut Output<'_>, pixels: &[u8], width: u32, height: u32) -> Result<()> {
    // image-webp 0.2.4 clones RGBA and buffers a VP8L frame before writing RIFF.
    // Admit the clone, frame Vec growth and bounded Huffman working storage.
    // Five encoded bytes/pixel covers the frequency-coded literals/run extras;
    // the factor two covers Vec capacity growth. No external codec is invoked.
    let _codec = output.budget.reserve_retained(pixels.len() + (pixels.len() / 4 * 5 + 65536) * 2 + 65536)?;
    output.budget.charge(Cost::work(pixels.len().div_ceil(16384) * 4))?;
    image_webp::WebPEncoder::new(output).encode(pixels, width, height, image_webp::ColorType::Rgba8)
        .map_err(|error| match error {
            image_webp::EncodingError::IoError(error) => stream_io_error(error),
            error => ToolkitError::new("command.failed", error.to_string(), 5),
        })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode<'a>(
    blob: &mut VerifiedBlob,
    encoding: &str,
    mipmap: usize,
    channels: &str,
    max_pixels: usize,
    maximum: usize,
    budget: &'a Budget,
    control: &OperationControl,
) -> Result<Encoded<'a>> {
    let (mut image, _pixels) = texture::decode_verified(blob, mipmap, max_pixels as u64, budget, control)?;
    let width = image.description.width;
    let height = image.description.height;
    if encoding == "webp" && (width > 16384 || height > 16384) {
        return Err(fault("texture.pixel_limit", 3));
    }
    let mut digest = Sha256::new();
    for block in image.pixels.chunks_mut(16384) {
        budget.charge(Cost::work(1))?;
        for pixel in block.as_chunks_mut::<4>().0 {
            if channels.len() == 1 {
                let selected = b"rgba".iter().position(|&name| name == channels.as_bytes()[0]).expect("normalized channels");
                let value = pixel[selected];
                pixel.copy_from_slice(&[value, value, value, 255]);
            } else {
                for (component, name) in b"rgba".iter().enumerate() {
                    if !channels.as_bytes().contains(name) {
                        pixel[component] = if component == 3 { 255 } else { 0 };
                    }
                }
            }
        }
        digest.update(block);
    }
    let info = Info { texture: image.description, channels: channels.into(),
        pixels_sha256: crate::casc::hex(&digest.finalize()) };
    let mut output = Output { bytes: Vec::new(), maximum, budget, allocation: None, error: None };
    let result = match encoding {
        "png" => png(&mut output, &image.pixels, width, height),
        "webp" => webp(&mut output, &image.pixels, width, height),
        _ => unreachable!("normalized image encoding"),
    };
    if let Some(error) = output.error { return Err(error); }
    result?;
    budget.check()?;
    Ok(Encoded { bytes: output.bytes, info, _allocation: output.allocation })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn both_codecs_preserve_output_limit_and_release_temporary_allocations() {
        for encoding in ["png", "webp"] {
            let control = OperationControl::default();
            let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
            let mut output = Output { bytes: Vec::new(), maximum: 1, budget: &budget, allocation: None, error: None };
            let error = if encoding == "png" {
                png(&mut output, &[1, 2, 3, 0], 1, 1)
            } else {
                webp(&mut output, &[1, 2, 3, 0], 1, 1)
            }.unwrap_err();
            assert_eq!(error.code, "records.image_output_limit");
            assert!(output.bytes.is_empty());
            control.cancel_handle().store(true, Ordering::Release);
            let error = stream_io_error(output.write(b"x").unwrap_err());
            assert_eq!(error.code, "records.image_output_limit");
            drop(output);
            assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
        }
    }
}
