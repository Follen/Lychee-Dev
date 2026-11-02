//! Compose only checksum-verified, committed SQLite WAL frames in memory.
use super::*;
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn checksum(bytes: &[u8], mut sums: (u32, u32), big: bool) -> (u32, u32) {
    for pair in bytes.chunks_exact(8) {
        let decode = |bytes: &[u8]| {
            if big {
                u32::from_be_bytes(bytes.try_into().unwrap())
            } else {
                u32::from_le_bytes(bytes.try_into().unwrap())
            }
        };
        sums.0 = sums.0.wrapping_add(decode(&pair[..4])).wrapping_add(sums.1);
        sums.1 = sums.1.wrapping_add(decode(&pair[4..])).wrapping_add(sums.0);
    }
    sums
}
pub(super) fn compose(database: &[u8], wal: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    control.check()?;
    if database.len() < 100
        || database.len() > MAX_IMAGE
        || &database[..16] != b"SQLite format 3\0"
        || wal.len() > MAX_IMAGE
    {
        return Err(fault(
            "DD browser database snapshot is invalid or exceeds its budget",
        ));
    }
    let page = u16::from_be_bytes(database[16..18].try_into().unwrap()) as usize;
    let page = if page == 1 { 65536 } else { page };
    if !(512..=65536).contains(&page) || !page.is_power_of_two() || database.len() % page != 0 {
        return Err(fault("DD browser database page layout is invalid"));
    }
    let mut result = database.to_vec();
    if !wal.is_empty() {
        if wal.len() < 32
            || ![0x377f0682, 0x377f0683].contains(&word(wal, 0))
            || word(wal, 4) != 3007000
            || word(wal, 8) as usize != page
        {
            return Err(fault("DD browser WAL header is unsupported or incomplete"));
        }
        let big = word(wal, 0) == 0x377f0683;
        let mut sums = checksum(&wal[..24], (0, 0), big);
        if sums != (word(wal, 24), word(wal, 28)) {
            return Err(fault("DD browser WAL header checksum failed"));
        }
        let mut frames = Vec::new();
        let mut committed = None;
        for (index, frame) in wal[32..].chunks_exact(page + 24).enumerate() {
            control.check()?;
            if frame[8..16] != wal[16..24] {
                break;
            }
            let candidate = checksum(&frame[24..], checksum(&frame[..8], sums, big), big);
            if candidate != (word(frame, 16), word(frame, 20)) {
                break;
            }
            let number = word(frame, 0) as usize;
            let size = word(frame, 4) as usize;
            if number == 0 || number > 32768 || size > 32768 {
                return Err(fault("DD browser WAL page identity exceeds its budget"));
            }
            sums = candidate;
            frames.push((number, 32 + index * (page + 24) + 24));
            if size != 0 {
                if size * page > MAX_IMAGE {
                    return Err(fault("DD browser committed database exceeds its budget"));
                }
                committed = Some((frames.len(), size));
            }
        }
        if let Some((count, size)) = committed {
            result.resize(size * page, 0);
            for &(number, offset) in &frames[..count] {
                control.check()?;
                if number <= size {
                    result[(number - 1) * page..number * page]
                        .copy_from_slice(&wal[offset..offset + page]);
                }
            }
        }
    }
    // A deserialized private image has no filesystem WAL. All committed pages
    // above are already present; SQLite reads it as an ordinary read-only DB.
    result[18..20].copy_from_slice(&[1, 1]);
    Ok(result)
}
