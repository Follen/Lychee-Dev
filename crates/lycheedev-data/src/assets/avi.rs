// SPDX-License-Identifier: MIT
// RIFF/AVI and VP9 superframe semantics ported from frozen records/video.
use super::fault;
use lycheedev_core::{Result, control::OperationControl};

#[derive(Debug)]
pub struct Frame<'a> {
    pub timestamp: f64,
    pub duration: f64,
    pub chunk: &'static str,
    pub offset: usize,
    pub payload: &'a [u8],
    pub sub_frames: Vec<usize>,
}
#[derive(Debug)]
pub struct Container<'a> {
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub frames: Vec<Frame<'a>>,
    pub enumerated_frames: usize,
    pub enumerated_bytes: u64,
    pub truncated: bool,
    pub truncation_reason: &'static str,
    pub warnings: Vec<&'static str>,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub max_frames: usize,
    pub max_frame_bytes: usize,
    pub max_total_bytes: usize,
    pub allow_partial: bool,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frames: 100_000,
            max_frame_bytes: 256 << 20,
            max_total_bytes: 1 << 30,
            allow_partial: false,
        }
    }
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("bounded chunk"))
}
struct Chunk<'a> {
    id: &'a [u8],
    size: usize,
    end: usize,
}
fn chunk(data: &[u8], pos: usize, bound: usize) -> Result<Chunk<'_>> {
    if pos > bound || bound - pos < 8 {
        return Err(fault("records.avi_truncated", 4));
    }
    let size = word(data, pos + 4) as usize;
    if size > bound - pos - 8 {
        return Err(fault("records.avi_truncated", 4));
    }
    Ok(Chunk {
        id: &data[pos..pos + 4],
        size,
        end: pos + 8 + size,
    })
}
impl Chunk<'_> {
    fn next(&self, bound: usize) -> usize {
        (self.end + (self.size & 1)).min(bound)
    }
}
pub fn parse<'a>(
    data: &'a [u8],
    limits: Limits,
    control: &OperationControl,
) -> Result<Container<'a>> {
    parse_scoped(data, limits, control, None)
}
fn check(control: &OperationControl, budget: Option<&crate::resource::Budget>) -> Result<()> {
    control.check()?;
    budget.map_or(Ok(()), crate::resource::Budget::check)
}
pub(crate) fn parse_scoped<'a>(
    data: &'a [u8],
    mut limits: Limits,
    control: &OperationControl,
    budget: Option<&crate::resource::Budget>,
) -> Result<Container<'a>> {
    check(control, budget)?;
    if limits.max_frames == 0 {
        limits.max_frames = 100_000;
    }
    if limits.max_frame_bytes == 0 {
        limits.max_frame_bytes = 256 << 20;
    }
    if limits.max_total_bytes == 0 {
        limits.max_total_bytes = 1 << 30;
    }
    if data.len() < 12 {
        return Err(fault("records.avi_truncated", 4));
    }
    if &data[..4] != b"RIFF" || &data[8..12] != b"AVI " {
        return Err(fault("records.avi_invalid", 4));
    }
    let declared = u64::from(word(data, 4)) + 8;
    if declared > data.len() as u64 {
        return Err(fault("records.avi_truncated", 4));
    }
    let mut result = Container {
        width: 0,
        height: 0,
        frame_rate: 30.0,
        frames: vec![],
        enumerated_frames: 0,
        enumerated_bytes: 0,
        truncated: false,
        truncation_reason: "",
        warnings: vec![],
    };
    if declared != data.len() as u64 {
        result.warnings.push("riff_size_mismatch");
    }
    let mut movi = None;
    let mut pos = 12;
    while data.len() - pos >= 8 {
        check(control, budget)?;
        let current = chunk(data, pos, data.len())?;
        match current.id {
            b"LIST" => {
                if current.size < 4 {
                    return Err(fault("records.avi_invalid", 4));
                }
                if &data[pos + 8..pos + 12] == b"movi" {
                    if movi.is_none() {
                        movi = Some((pos + 12, current.end));
                    }
                } else {
                    result.scan_headers(data, pos + 12, current.end, 1, control, budget)?;
                }
            }
            b"movi" => {
                if movi.is_none() {
                    movi = Some((pos + 8, current.end));
                }
            }
            b"avih" => {
                if current.size < 4 {
                    return Err(fault("records.avi_invalid", 4));
                }
                let period = word(data, pos + 8);
                if period > 0 {
                    result.frame_rate = 1_000_000.0 / f64::from(period);
                }
            }
            _ => {}
        }
        pos = current.next(data.len());
    }
    let (mut pos, end) = movi.ok_or_else(|| fault("records.avi_movi_missing", 4))?;
    let duration = (1_000_000.0 / result.frame_rate).trunc();
    let mut timestamp = 0.0;
    let mut retained = 0usize;
    while end - pos >= 8 {
        check(control, budget)?;
        let current = chunk(data, pos, end)?;
        let kind = match current.id {
            b"00dc" => "00dc",
            b"00db" => "00db",
            _ => {
                pos = current.next(end);
                continue;
            }
        };
        if current.size > limits.max_frame_bytes {
            return Err(fault("records.frame_limit_exceeded", 3));
        }
        result.enumerated_frames += 1;
        result.enumerated_bytes += current.size as u64;
        if !result.truncated {
            let reason = if result.frames.len() >= limits.max_frames {
                "frame_limit"
            } else if current.size > limits.max_total_bytes - retained {
                "byte_limit"
            } else {
                ""
            };
            if !reason.is_empty() {
                if !limits.allow_partial {
                    return Err(fault("records.frame_limit_exceeded", 3));
                }
                result.truncated = true;
                result.truncation_reason = reason;
            } else {
                if result.frames.len() == result.frames.capacity() {
                    let capacity = result.frames.capacity().saturating_mul(2).max(4);
                    if let Some(budget) = budget {
                        // Admit the whole replacement while the old allocation
                        // may still exist; private safety does not change Go usage.
                        budget
                            .safety_only()
                            .charge(crate::resource::Cost::metadata(
                                capacity
                                    .checked_mul(std::mem::size_of::<Frame<'_>>())
                                    .ok_or_else(|| fault("records.metadata_limit", 3))?,
                            ))?;
                    }
                    result
                        .frames
                        .try_reserve_exact(capacity - result.frames.len())
                        .map_err(|_| fault("records.frame_limit_exceeded", 3))?;
                }
                if let Some(budget) = budget {
                    // VP9's four candidate layouts each allocate at most eight
                    // usize sizes; the successful one stays with this frame.
                    budget
                        .safety_only()
                        .charge(crate::resource::Cost::metadata(
                            4 * 8 * std::mem::size_of::<usize>(),
                        ))?;
                }
                let payload = &data[pos + 8..current.end];
                result.frames.push(Frame {
                    timestamp,
                    duration,
                    chunk: kind,
                    offset: pos + 8,
                    payload,
                    sub_frames: superframe_sizes(payload),
                });
                timestamp += duration;
                retained += current.size;
            }
        }
        pos = current.next(end);
    }
    check(control, budget)?;
    Ok(result)
}
impl Container<'_> {
    fn scan_headers(
        &mut self,
        data: &[u8],
        mut pos: usize,
        end: usize,
        depth: usize,
        control: &OperationControl,
        budget: Option<&crate::resource::Budget>,
    ) -> Result<()> {
        if depth > 16 {
            return Err(fault("records.avi_invalid", 4));
        }
        while end - pos >= 8 {
            check(control, budget)?;
            let current = chunk(data, pos, end)?;
            match current.id {
                b"LIST" => {
                    if current.size < 4 {
                        return Err(fault("records.avi_invalid", 4));
                    }
                    self.scan_headers(data, pos + 12, current.end, depth + 1, control, budget)?;
                }
                b"avih" => {
                    if current.size < 4 {
                        return Err(fault("records.avi_invalid", 4));
                    }
                    let period = word(data, pos + 8);
                    if period > 0 {
                        self.frame_rate = 1_000_000.0 / f64::from(period);
                    }
                }
                b"strf" if current.size >= 40 => {
                    self.width = word(data, pos + 12);
                    self.height = word(data, pos + 16);
                }
                _ => {}
            }
            pos = current.next(end);
        }
        Ok(())
    }
}
pub fn superframe_sizes(payload: &[u8]) -> Vec<usize> {
    if payload.len() < 2 {
        return vec![];
    }
    let marker = payload[payload.len() - 1];
    if marker & 0xe0 != 0xc0 {
        return vec![];
    }
    let frames = usize::from(marker & 7) + 1;
    if frames == 1 {
        return vec![];
    }
    let magnitude = usize::from((marker >> 3) & 3) + 1;
    for copies in [1, 2] {
        for bookend in [0, 1] {
            let size = bookend + copies * frames * magnitude + 1;
            if size > payload.len() {
                continue;
            }
            let start = payload.len() - size;
            if bookend == 1 && payload[start] != marker {
                continue;
            }
            let mut sizes = Vec::with_capacity(frames);
            let mut pos = start + bookend;
            let mut total = 0;
            let mut valid = true;
            for table in 0..copies {
                for frame in 0..frames {
                    let mut value = 0usize;
                    for byte in 0..magnitude {
                        value |= usize::from(payload[pos + byte]) << (8 * byte);
                    }
                    pos += magnitude;
                    if value == 0 {
                        valid = false;
                    }
                    if table == 0 {
                        sizes.push(value);
                        total += value;
                    } else if sizes[frame] != value {
                        valid = false;
                    }
                }
            }
            if valid && total == start {
                return sizes;
            }
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
        let mut raw = id.to_vec();
        raw.extend((bytes.len() as u32).to_le_bytes());
        raw.extend(bytes);
        if bytes.len() & 1 != 0 {
            raw.push(0);
        }
        raw
    }
    fn sample() -> Vec<u8> {
        let mut body = b"AVI ".to_vec();
        body.extend(record(b"avih", &40_000u32.to_le_bytes()));
        let mut frames = b"movi".to_vec();
        frames.extend(record(b"00dc", &[1, 2, 3]));
        frames.extend(record(b"JUNK", &[0]));
        frames.extend(record(b"00db", &[4, 5]));
        body.extend(record(b"LIST", &frames));
        record(b"RIFF", &body)
    }
    #[test]
    fn frames_keep_exact_payload_offsets_timing_and_total_after_partial() {
        let raw = sample();
        let control = OperationControl::default();
        let parsed = parse(&raw, Limits::default(), &control).unwrap();
        assert_eq!(parsed.frame_rate, 25.0);
        assert_eq!(parsed.enumerated_frames, 2);
        assert_eq!(parsed.enumerated_bytes, 5);
        assert_eq!(parsed.frames[0].payload, &[1, 2, 3]);
        assert_eq!(parsed.frames[1].payload, &[4, 5]);
        assert_eq!(parsed.frames[1].timestamp, 40_000.0);
        assert_eq!(
            &raw[parsed.frames[1].offset..parsed.frames[1].offset + 2],
            &[4, 5]
        );
        let partial = parse(
            &raw,
            Limits {
                max_frames: 1,
                allow_partial: true,
                ..Limits::default()
            },
            &control,
        )
        .unwrap();
        assert_eq!(partial.frames.len(), 1);
        assert_eq!(partial.enumerated_frames, 2);
        assert_eq!(partial.truncation_reason, "frame_limit");
        assert_eq!(
            parse(
                &raw,
                Limits {
                    max_frames: 1,
                    ..Limits::default()
                },
                &control
            )
            .unwrap_err()
            .code,
            "records.frame_limit_exceeded"
        );
        assert_eq!(
            parse(
                &raw,
                Limits {
                    max_frames: 1,
                    max_frame_bytes: 1,
                    allow_partial: true,
                    ..Limits::default()
                },
                &control
            )
            .unwrap_err()
            .code,
            "records.frame_limit_exceeded"
        );
    }
    #[test]
    fn malformed_and_truncated_extents_never_become_partial_success() {
        let control = OperationControl::default();
        let raw = sample();
        assert_eq!(
            parse(
                &raw[..raw.len() - 1],
                Limits {
                    allow_partial: true,
                    ..Limits::default()
                },
                &control
            )
            .unwrap_err()
            .code,
            "records.avi_truncated"
        );
        let mut bad = raw.clone();
        bad[0] = 0;
        assert_eq!(
            parse(&bad, Limits::default(), &control).unwrap_err().code,
            "records.avi_invalid"
        );
        let missing = record(b"RIFF", b"AVI ");
        assert_eq!(
            parse(&missing, Limits::default(), &control)
                .unwrap_err()
                .code,
            "records.avi_movi_missing"
        );
        let mut mismatch = raw.clone();
        mismatch[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(
            parse(&mismatch, Limits::default(), &control)
                .unwrap()
                .warnings,
            ["riff_size_mismatch"]
        );
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            parse(&raw, Limits::default(), &control)
                .unwrap_err()
                .exit_code,
            7
        );
    }
    #[test]
    fn vp9_superframe_bookends_and_duplicate_tables_must_cover_payload_exactly() {
        assert_eq!(superframe_sizes(&[1, 2, 3, 4, 5, 0xc1, 2, 3, 0xc1]), [2, 3]);
        assert_eq!(superframe_sizes(&[1, 2, 3, 4, 5, 2, 3, 2, 3, 0xc1]), [2, 3]);
        assert!(superframe_sizes(&[1, 2, 3, 4, 5, 0xc1, 2, 2, 0xc1]).is_empty());
        assert!(superframe_sizes(&[1, 2, 3, 4, 5, 2, 3, 1, 3, 0xc1]).is_empty());
    }
}
