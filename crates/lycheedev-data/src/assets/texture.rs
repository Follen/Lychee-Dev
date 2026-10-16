// SPDX-License-Identifier: MIT
// BLP2 layout/block semantics ported from the frozen records/texture module.
use super::fault;
use crate::resource::{Budget, Cost, RetainedReservation};
use lycheedev_core::{Result, control::OperationControl, evidence::VerifiedBlob};
use serde::Serialize;
use std::io::Read;
mod parallel;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    pub format: &'static str,
    pub storage: &'static str,
    pub width: u32,
    pub height: u32,
    pub mipmap: usize,
    pub levels: usize,
    pub alpha_bits: u8,
}
pub struct Image {
    pub pixels: Vec<u8>,
    pub description: Description,
}
struct Layout {
    description: Description,
    offset: usize,
    size: usize,
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().expect("validated header"))
}
pub fn decode(
    raw: &[u8],
    mipmap: usize,
    max_pixels: u64,
    control: &OperationControl,
) -> Result<Image> {
    let selected = layout(raw, raw.len(), mipmap, max_pixels, control)?;
    decode_pixels(raw, &raw[selected.offset..selected.offset + selected.size], selected.description, control)
}
fn layout(
    raw: &[u8],
    length: usize,
    mipmap: usize,
    max_pixels: u64,
    control: &OperationControl,
) -> Result<Layout> {
    control.check()?;
    let invalid = || fault("texture.invalid_format", 4);
    let unsupported = || fault("texture.unsupported_format", 3);
    if max_pixels == 0 || max_pixels > 64 << 20 {
        return Err(fault("texture.pixel_limit", 3));
    }
    if mipmap >= 16 {
        return Err(fault("texture.mipmap_unavailable", 3));
    }
    if raw.len() < 4 {
        return Err(invalid());
    }
    if &raw[..4] != b"BLP2" {
        return Err(unsupported());
    }
    if raw.len() < 148 {
        return Err(invalid());
    }
    if word(raw, 4) != 1 {
        return Err(unsupported());
    }
    let (encoding, alpha, mode) = (raw[8], raw[9], raw[10]);
    let (width, height) = (u64::from(word(raw, 12)), u64::from(word(raw, 16)));
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    // Byte 11 is flags, including the observed Retail 0x11, not a bool.
    let (storage, header) = match encoding {
        1 if [0, 1, 4, 8].contains(&alpha) => ("palette", 1172),
        2 if mode == 0 && alpha <= 1 => ("bc1", 148),
        2 if mode == 1 && [0, 4, 8].contains(&alpha) => ("bc2", 148),
        2 if mode == 7 && alpha == 8 => ("bc3", 148),
        3 if [0, 8].contains(&alpha) => ("bgra8", 148),
        _ => return Err(unsupported()),
    };
    if raw.len() < header {
        return Err(invalid());
    }
    let mut spans = [(0usize, 0usize); 16];
    let mut levels = 0;
    for index in 0..16 {
        let offset = word(raw, 20 + 4 * index) as usize;
        let size = word(raw, 84 + 4 * index) as usize;
        if offset == 0 && size == 0 {
            continue;
        }
        if offset < header || size == 0 || offset > length || size > length - offset {
            return Err(invalid());
        }
        for &(previous, length) in &spans[..index] {
            if length != 0 && offset < previous + length && previous < offset + size {
                return Err(invalid());
            }
        }
        spans[index] = (offset, size);
        levels += 1;
    }
    let (offset, size) = spans[mipmap];
    if size == 0 {
        return Err(fault("texture.mipmap_unavailable", 3));
    }
    let (width, height) = ((width >> mipmap).max(1), (height >> mipmap).max(1));
    if width > max_pixels || height > max_pixels / width {
        return Err(fault("texture.pixel_limit", 3));
    }
    let count = width * height;
    let expected = match storage {
        "palette" => count + (count * u64::from(alpha)).div_ceil(8),
        "bc1" => width.div_ceil(4) * height.div_ceil(4) * 8,
        "bc2" | "bc3" => width.div_ceil(4) * height.div_ceil(4) * 16,
        _ => count * 4,
    };
    if size as u64 != expected {
        return Err(invalid());
    }
    Ok(Layout {
        description: Description { format: "BLP2", storage, width: width as u32,
            height: height as u32, mipmap, levels, alpha_bits: alpha },
        offset, size,
    })
}
fn decode_pixels(
    raw: &[u8],
    data: &[u8],
    description: Description,
    control: &OperationControl,
) -> Result<Image> {
    decode_pixels_budgeted(raw, data, description, control, None)
}
fn decode_pixels_budgeted(
    raw: &[u8],
    data: &[u8],
    description: Description,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<Image> {
    let count = description.width as usize * description.height as usize;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count * 4)
        .map_err(|_| fault("texture.pixel_limit", 3))?;
    pixels.resize(count * 4, 0);
    if let Some(budget) = budget {
        parallel::fill(&mut pixels, raw, data, &description, control, budget)?;
    } else {
        fill_strip(&mut pixels, 0, raw, data, &description, control, None)?;
    }
    control.check()?;
    Ok(Image { pixels, description })
}

/// One disjoint band of complete four-row block groups (except the bottom
/// edge). Input offsets stay global; paint receives only this band's output.
/// Palette alpha bits also keep their global index across band boundaries.
fn fill_strip(
    pixels: &mut [u8],
    first_row: usize,
    raw: &[u8],
    data: &[u8],
    description: &Description,
    control: &OperationControl,
    budget: Option<&Budget>,
) -> Result<()> {
    let check = || {
        control.check()?;
        budget.map_or(Ok(()), Budget::check)
    };
    let width = description.width as usize;
    let count = width * description.height as usize;
    let height = pixels.len() / (width * 4);
    let (storage, alpha) = (description.storage, description.alpha_bits);
    if storage == "palette" || storage == "bgra8" {
        for (local, dst) in pixels.chunks_exact_mut(4).enumerate() {
            if local & 4095 == 0 {
                check()?;
            }
            let index = first_row * width + local;
            if storage == "bgra8" {
                dst.copy_from_slice(&[
                    data[index * 4 + 2],
                    data[index * 4 + 1],
                    data[index * 4],
                    data[index * 4 + 3],
                ]);
            } else {
                let at = 148 + 4 * usize::from(data[index]);
                dst.copy_from_slice(&[raw[at + 2], raw[at + 1], raw[at], 255]);
                if alpha > 0 {
                    let bit = index * usize::from(alpha);
                    let mask = (1u16 << alpha) - 1;
                    let value = u16::from(data[count + bit / 8] >> (bit % 8)) & mask;
                    dst[3] = (value * 255 / mask) as u8;
                }
            }
        }
    } else {
        let step = if storage == "bc1" { 8 } else { 16 };
        let mut at = first_row / 4 * width.div_ceil(4) * step;
        for y in (0..height).step_by(4) {
            check()?;
            for x in (0..width).step_by(4) {
                // Bound cancellation even for a valid one-row, very wide mip.
                if x & 4095 == 0 {
                    check()?;
                }
                paint(pixels, width, height, x, y, &data[at..at + step], storage);
                at += step;
            }
        }
    }
    check()
}

/// The CASC boundary already authenticated this held blob. Decode only the
/// selected mip and palette, retaining its RGBA allocation under the same budget.
pub(crate) fn decode_verified<'a>(
    blob: &mut VerifiedBlob,
    mipmap: usize,
    max_pixels: u64,
    budget: &'a Budget,
    control: &OperationControl,
) -> Result<(Image, RetainedReservation<'a>)> {
    let length = blob.reference().bytes as usize;
    let _scratch = budget.reserve_retained(1172 + 65536)?;
    let mut header = vec![0; length.min(148)];
    blob.read_exact(&mut header).map_err(crate::blte::stream_io_error)?;
    if header.len() == 148 && header[8] == 1 {
        header.resize(length.min(1172), 0);
        blob.read_exact(&mut header[148..]).map_err(crate::blte::stream_io_error)?;
    }
    let selected = layout(&header, length, mipmap, max_pixels, control)?;
    let pixels = selected.description.width as usize * selected.description.height as usize * 4;
    let allocation = budget.reserve_retained(pixels)?;
    let _mip = budget.reserve_retained(selected.size)?;
    let mut skip = selected.offset - header.len();
    let mut buffer = [0; 65536];
    while skip != 0 {
        budget.charge(Cost::work(1))?;
        let n = skip.min(buffer.len());
        blob.read_exact(&mut buffer[..n]).map_err(crate::blte::stream_io_error)?;
        skip -= n;
    }
    let mut data = vec![0; selected.size];
    for block in data.chunks_mut(65536) {
        budget.charge(Cost::work(1))?;
        blob.read_exact(block).map_err(crate::blte::stream_io_error)?;
    }
    budget.charge(Cost::work(pixels.div_ceil(16384)))?;
    let image = decode_pixels_budgeted(
        &header, &data, selected.description, control, Some(budget),
    )?;
    Ok((image, allocation))
}
fn expand(packed: u16) -> [u8; 4] {
    let (r, g, b) = ((packed >> 11) & 31, (packed >> 5) & 63, packed & 31);
    [
        (r << 3 | r >> 2) as u8,
        (g << 2 | g >> 4) as u8,
        (b << 3 | b >> 2) as u8,
        255,
    ]
}
fn paint(
    dst: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    data: &[u8],
    storage: &str,
) {
    let color_at = if storage == "bc1" { 0 } else { 8 };
    let first = u16::from_le_bytes(data[color_at..color_at + 2].try_into().unwrap());
    let second = u16::from_le_bytes(data[color_at + 2..color_at + 4].try_into().unwrap());
    let mut colors = [
        expand(first),
        expand(second),
        [0, 0, 0, 255],
        [0, 0, 0, 255],
    ];
    let [start, end, low, high] = &mut colors;
    for (((&a, &b), mixed), other) in start
        .iter()
        .zip(end.iter())
        .zip(low.iter_mut())
        .zip(high.iter_mut())
        .take(3)
    {
        let (a, b) = (u16::from(a), u16::from(b));
        if storage == "bc1" && first <= second {
            *mixed = ((a + b) / 2) as u8;
        } else {
            *mixed = ((2 * a + b) / 3) as u8;
            *other = ((a + 2 * b) / 3) as u8;
        }
    }
    if storage == "bc1" && first <= second {
        colors[3] = [0; 4];
    }
    let indices = word(data, color_at + 4);
    let mut opacity = [0u8; 8];
    let mut alpha_indices = 0u64;
    if storage == "bc3" {
        opacity[0] = data[0];
        opacity[1] = data[1];
        let divisor = if opacity[0] > opacity[1] {
            7
        } else {
            opacity[6] = 0;
            opacity[7] = 255;
            5
        };
        for index in 1..divisor {
            opacity[index + 1] = (((divisor - index) * usize::from(opacity[0])
                + index * usize::from(opacity[1]))
                / divisor) as u8;
        }
        for index in 0..6 {
            alpha_indices |= u64::from(data[2 + index]) << (8 * index);
        }
    }
    for row in 0..4 {
        for column in 0..4 {
            if x + column >= width || y + row >= height {
                continue;
            }
            let index = row * 4 + column;
            let mut color = colors[((indices >> (2 * index)) & 3) as usize];
            match storage {
                "bc2" => color[3] = ((data[index / 2] >> (4 * (index % 2))) & 15) * 17,
                "bc3" => color[3] = opacity[((alpha_indices >> (3 * index)) & 7) as usize],
                _ => {}
            }
            let at = ((y + row) * width + x + column) * 4;
            dst[at..at + 4].copy_from_slice(&color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(encoding: u8, alpha: u8, mode: u8, width: u32, height: u32, data: &[u8]) -> Vec<u8> {
        let header = if encoding == 1 { 1172 } else { 148 };
        let mut raw = vec![0; header];
        raw[..4].copy_from_slice(b"BLP2");
        raw[4] = 1;
        raw[8..12].copy_from_slice(&[encoding, alpha, mode, 0x11]);
        raw[12..16].copy_from_slice(&width.to_le_bytes());
        raw[16..20].copy_from_slice(&height.to_le_bytes());
        raw[20..24].copy_from_slice(&(header as u32).to_le_bytes());
        raw[84..88].copy_from_slice(&(data.len() as u32).to_le_bytes());
        raw.extend_from_slice(data);
        raw
    }
    #[test]
    fn palette_alpha_widths_and_flags_preserve_straight_rgb() {
        for (alpha, tail, expected) in [
            (0, vec![], vec![255, 255]),
            (1, vec![2], vec![0, 255]),
            (4, vec![0xf1], vec![17, 255]),
            (8, vec![13, 255], vec![13, 255]),
        ] {
            let mut payload = vec![0, 0];
            payload.extend(tail);
            let mut raw = fixture(1, alpha, 0, 2, 1, &payload);
            raw[148..152].copy_from_slice(&[3, 5, 7, 99]);
            let image = decode(&raw, 0, 2, &OperationControl::default()).unwrap();
            assert_eq!(image.pixels, [7, 5, 3, expected[0], 7, 5, 3, expected[1]]);
            assert_eq!(image.description.storage, "palette");
        }
    }
    #[test]
    fn block_interpolation_transparency_and_clipped_edges_are_exact() {
        let control = OperationControl::default();
        let block = [0x00, 0xf8, 0xe0, 0x07, 0xe4, 0xe4, 0xe4, 0xe4];
        let image = decode(&fixture(2, 0, 0, 3, 1, &block), 0, 3, &control).unwrap();
        assert_eq!(
            image.pixels,
            [255, 0, 0, 255, 0, 255, 0, 255, 170, 85, 0, 255]
        );
        let transparent = [0, 0, 255, 255, 255, 255, 255, 255];
        assert_eq!(
            decode(&fixture(2, 1, 0, 1, 1, &transparent), 0, 1, &control)
                .unwrap()
                .pixels,
            [0, 0, 0, 0]
        );
        let mut bc2 = vec![0x21; 8];
        bc2.extend(block);
        assert_eq!(
            decode(&fixture(2, 4, 1, 2, 1, &bc2), 0, 2, &control)
                .unwrap()
                .pixels,
            [255, 0, 0, 17, 0, 255, 0, 34]
        );
        let mut bc3 = vec![10, 20, 0xfe, 0, 0, 0, 0, 0];
        bc3.extend(block);
        assert_eq!(
            decode(&fixture(2, 8, 7, 2, 1, &bc3), 0, 2, &control)
                .unwrap()
                .pixels,
            [255, 0, 0, 0, 0, 255, 0, 255]
        );
    }
    #[test]
    fn sparse_mips_overlap_truncation_pixel_budget_and_cancel_are_distinct() {
        let control = OperationControl::default();
        let mut raw = fixture(3, 8, 0, 2, 2, &[1, 2, 3, 4]);
        raw[20..24].fill(0);
        raw[84..88].fill(0);
        raw[24..28].copy_from_slice(&148u32.to_le_bytes());
        raw[88..92].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(decode(&raw, 1, 1, &control).unwrap().pixels, [3, 2, 1, 4]);
        assert_eq!(
            decode(&raw, 0, 4, &control).err().unwrap().code,
            "texture.mipmap_unavailable"
        );
        raw[20..24].copy_from_slice(&148u32.to_le_bytes());
        raw[84..88].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(
            decode(&raw, 1, 4, &control).err().unwrap().code,
            "texture.invalid_format"
        );
        let raw = fixture(3, 8, 0, 2, 1, &[0; 8]);
        assert_eq!(
            decode(&raw, 0, 1, &control).err().unwrap().code,
            "texture.pixel_limit"
        );
        assert_eq!(
            decode(&raw[..raw.len() - 1], 0, 2, &control)
                .err()
                .unwrap()
                .code,
            "texture.invalid_format"
        );
        control
            .token()
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(decode(&raw, 0, 2, &control).err().unwrap().exit_code, 7);
    }
}
