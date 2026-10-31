//! WDC3 cross-section string pool with a fixed eight-byte unavailable gap.
use super::fixture::{self, Cell, Table};
fn put(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub fn table(id: u32, gap: usize, text: &[u8]) -> Table {
    assert!(gap + 8 <= 1024 && text.len() <= 1024);
    let mut spec = fixture::table(
        "Sample",
        id,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(1), Cell::Text(String::new())]],
    );
    let one = &spec.raw;
    let original = u32::from_le_bytes(one[80..84].try_into().unwrap()) as usize;
    let start = original + 40;
    let mut raw = one[..112].to_vec();
    raw.extend([0; 40]);
    raw.extend_from_slice(&one[112..original + 8]);
    raw.extend(2u32.to_le_bytes());
    raw.extend(0u32.to_le_bytes());
    let pool = raw.len();
    raw.resize(pool + 1024, b'x');
    raw[pool..pool + text.len()].copy_from_slice(text);
    raw[pool + gap..pool + gap + 8].copy_from_slice(&[2, 0, 0, 0, 99, 0, 0, 0]);
    for (at, value) in [
        (4, 2),
        (16, 1024),
        (32, 2),
        (68, 2),
        (80, start as u32),
        (88, 0),
        (120, (start + 8) as u32),
        (124, 1),
        (128, 1024),
        (start + 4, 12),
    ] {
        put(&mut raw, at, value);
    }
    raw[112..120].copy_from_slice(&0x1234567890abcdefu64.to_le_bytes());
    spec.raw = raw;
    spec
}
pub fn pool(raw: &[u8]) -> usize {
    u32::from_le_bytes(raw[120..124].try_into().unwrap()) as usize + 8
}
