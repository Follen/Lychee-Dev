//! Original-source byte coordinates, independent of the parser and source maps.
//! A CRLF/LFCR pair advances the line at its FIRST byte (including a query
//! between the pair's bytes). BOM and UTF-8 bytes have no special meaning here.

const SCAN_CHUNK: usize = 16 * 1024;

pub(super) struct OriginalLines {
    len: usize,
    newline_offsets: Vec<u32>,
}

impl OriginalLines {
    /// Two bounded linear passes avoid geometric allocation and over-retention.
    /// The payload is at most four bytes per admitted source byte. `poll` is
    /// called before every chunk and after both passes, including empty input.
    pub(super) fn build<E>(
        text: &str,
        remaining_bytes: usize,
        mut poll: impl FnMut() -> Result<(), E>,
    ) -> Result<Option<Self>, E> {
        poll()?;
        if text.len() > u32::MAX as usize { return Ok(None); }
        let mut count = 0usize;
        scan(text.as_bytes(), &mut poll, |_| { count += 1; })?;
        let Some(bytes) = count.checked_mul(std::mem::size_of::<u32>()) else { return Ok(None); };
        if bytes > remaining_bytes || bytes > isize::MAX as usize { return Ok(None); }
        let mut newline_offsets = Vec::new();
        if newline_offsets.try_reserve_exact(count).is_err() { poll()?; return Ok(None); }
        // Account actual Vec capacity, not only the requested element count.
        let capacity_bytes = newline_offsets.capacity().checked_mul(std::mem::size_of::<u32>());
        if capacity_bytes.is_none_or(|bytes| bytes > remaining_bytes) { poll()?; return Ok(None); }
        scan(text.as_bytes(), &mut poll, |offset| newline_offsets.push(offset as u32))?;
        Ok(Some(Self { len: text.len(), newline_offsets }))
    }

    pub(super) fn retained_bytes(&self) -> usize {
        // Checked before allocation acceptance; no growth after construction.
        self.newline_offsets.capacity() * std::mem::size_of::<u32>()
    }

    pub(super) fn line_at(&self, offset: usize) -> usize {
        let offset = offset.min(self.len);
        self.newline_offsets.partition_point(|&start| (start as usize) < offset) + 1
    }
}

fn scan<E>(
    bytes: &[u8],
    poll: &mut impl FnMut() -> Result<(), E>,
    mut newline: impl FnMut(usize),
) -> Result<(), E> {
    let mut pending_pair = None;
    for (chunk_index, chunk) in bytes.chunks(SCAN_CHUNK).enumerate() {
        poll()?;
        for (index, &byte) in chunk.iter().enumerate() {
            if let Some(first) = pending_pair.take() {
                if byte != first && matches!(byte, b'\r' | b'\n') {
                    continue;
                }
            }
            if matches!(byte, b'\r' | b'\n') {
                newline(chunk_index * SCAN_CHUNK + index);
                pending_pair = Some(byte);
            }
        }
    }
    poll()
}

/// Allocation/cap fallback preserves accepted inputs and typed control errors.
/// Prefix slicing is by bytes; it intentionally permits offsets inside UTF-8.
pub(super) fn controlled_line_at<E>(text: &str, offset: usize, mut poll: impl FnMut() -> Result<(), E>) -> Result<usize, E> {
    let mut line = 1;
    scan(&text.as_bytes()[..offset.min(text.len())], &mut poll, |_| line += 1)?;
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(text: &str, poll: impl FnMut() -> Result<(), String>) -> Result<OriginalLines, String> {
        OriginalLines::build(text, usize::MAX, poll).map(|index| index.unwrap())
    }

    // The previous production scanner is retained verbatim as the oracle.
    fn original_line_at(text:&str,offset:usize)->usize {
        let bytes=text.as_bytes();let mut line=1;let mut i=0;
        while i<offset && i<bytes.len(){let ch=bytes[i];i+=1;if ch==b'\r'||ch==b'\n'{
            if i<bytes.len() && bytes[i]!=ch && (bytes[i]==b'\r'||bytes[i]==b'\n'){i+=1;}line+=1;
        }}line
    }

    fn assert_all_offsets(text: &str) {
        let index = build(text, || Ok(())).unwrap();
        for offset in (0..=text.len() + 2).chain([usize::MAX]) {
            assert_eq!(controlled_line_at(text, offset, || Ok::<_, String>(())).unwrap(), original_line_at(text, offset));
            assert_eq!(index.line_at(offset), original_line_at(text, offset), "{text:?} at {offset}");
        }
        assert!(index.newline_offsets.len() <= text.len());
    }

    #[test]
    fn all_original_offsets_match_scanner() {
        for text in ["", "abc", "\n", "\r", "\r\n", "\n\r", "\r\r", "\n\n",
            "a\r\nb\n\rc\r\rd\n\n", "\u{feff}function 中文()\r\n return 'é'\nend\r"] {
            assert_all_offsets(text);
        }
        // Exhaust every newline pairing, not just the named examples.
        for length in 0..=7u32 {
            for mut value in 0..3usize.pow(length) {
                let mut text = String::new();
                for _ in 0..length { text.push(['x', '\r', '\n'][value % 3]); value /= 3; }
                assert_all_offsets(&text);
            }
        }
    }

    #[test]
    fn pairs_across_control_chunks_and_cancellation() {
        for pair in ["\r\n", "\n\r", "\r\r", "\n\n"] {
            let text = format!("{}{pair}尾", "x".repeat(SCAN_CHUNK - 1));
            let index = build(&text, || Ok(())).unwrap();
            for offset in [0, SCAN_CHUNK - 1, SCAN_CHUNK, SCAN_CHUNK + 1, text.len(), text.len() + 1, usize::MAX] {
                assert_eq!(index.line_at(offset), original_line_at(&text, offset));
                assert_eq!(controlled_line_at(&text, offset, || Ok::<_, String>(())).unwrap(), original_line_at(&text, offset));
            }
            let mut polls = 0;
            build(&text, || { polls += 1; Ok(()) }).unwrap();
            assert_eq!(polls, 7); // Initial poll, then two chunks and final poll, twice.
            for stop in 1..=polls {
                let mut seen = 0;
                let result = build(&text, || {
                    seen += 1;
                    if seen == stop { Err("cancelled".into()) } else { Ok(()) }
                });
                assert_eq!(result.err().as_deref(), Some("cancelled"));
                assert_eq!(seen, stop);
            }
        }
    }


    #[test]
    fn aggregate_cap_fallback_and_typed_cancellation() {
        for cap in [0, 3, 4, 7, 8, 9] {
            let index = OriginalLines::build("\n\n", cap, || Ok::<_, usize>(())).unwrap();
            assert_eq!(index.is_some(), cap >= 8);
        }
        let mut remaining = 64usize;
        let mut retained = Vec::new();
        for text in ["", "x", "\n\r", "\n\n", "\n\n\n\n", "\r\n" , "\n\n\n\n\n\n\n\n\n\n\n\n"] {
            let index = OriginalLines::build(text, remaining, || Ok::<_, usize>(())).unwrap();
            for offset in 0..=text.len() + 1 {
                let actual = match &index {
                    Some(index) => index.line_at(offset),
                    None => controlled_line_at(text, offset, || Ok::<_, usize>(())).unwrap(),
                };
                assert_eq!(actual, original_line_at(text, offset));
            }
            if let Some(index) = index {
                remaining = remaining.checked_sub(index.retained_bytes()).unwrap();
                retained.push(index);
            }
            assert!(retained.iter().map(OriginalLines::retained_bytes).sum::<usize>() <= 64);
        }
        let text = "\n".repeat(SCAN_CHUNK * 2 + 1);
        assert!(OriginalLines::build(&text, 0, || Ok::<_, usize>(())).unwrap().is_none());
        let mut polls = 0;
        assert_eq!(controlled_line_at(&text, usize::MAX, || { polls += 1; Ok::<_, usize>(()) }).unwrap(), text.len() + 1);
        assert_eq!(polls, 4);
        // Fallback still visits the prefix for each query: include that cost,
        // rather than claiming the indexed improvement applies after the cap.
        let mut fallback_events = 0;
        for offset in 0..=64 {
            scan(&text.as_bytes()[..offset], &mut || Ok::<_, usize>(()), |_| fallback_events += 1).unwrap();
            assert_eq!(controlled_line_at(&text, offset, || Ok::<_, usize>(())).unwrap(), offset + 1);
        }
        assert_eq!(fallback_events, (64 * 65) / 2);
        for stop in 1..=polls {
            let mut seen = 0;
            let result = controlled_line_at(&text, usize::MAX, || {
                seen += 1;
                if seen == stop { Err(73usize) } else { Ok(()) }
            });
            assert_eq!(result, Err(73));
            assert_eq!(seen, stop);
        }
    }

    #[test]
    fn deterministic_work_avoids_repeated_prefix_scans() {
        let text = "x\n".repeat(4096);
        let index = build(&text, || Ok(())).unwrap();
        let mut old_visited = 0usize;
        let mut indexed_comparisons = 0usize;
        for offset in (0..text.len()).step_by(8) {
            old_visited += offset; // No paired bytes in this fixture.
            let line = index.newline_offsets.partition_point(|&start| {
                indexed_comparisons += 1;
                (start as usize) < offset
            }) + 1;
            assert_eq!(line, original_line_at(&text, offset));
            assert_eq!(line, index.line_at(offset));
        }
        // scan visits every byte exactly once per pass; this is an operation
        // count, NOT a wall-clock or Go/Rust performance acceptance claim.
        assert!(2 * text.len() + indexed_comparisons < old_visited / 10);
    }
}
