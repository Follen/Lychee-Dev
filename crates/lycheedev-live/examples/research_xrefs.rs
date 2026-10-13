//! Research-only discovery against an independently known source value, not admission.
use lycheedev_live::locator::Pe;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let pe = Pe::parse(&bytes).unwrap();
    let target = u64::from_str_radix(a[2].trim_start_matches("0x"), 16).unwrap();
    let mut refs = Vec::new();
    for s in &pe.sections {
        if s.flags & 0x20000000 == 0 {
            continue;
        }
        let data = &bytes
            [s.file_offset as usize..(s.file_offset + s.raw_size.min(s.virtual_size)) as usize];
        for i in 0..data.len().saturating_sub(7) {
            if data[i] == 0x48 && data[i + 1] == 0x8b && data[i + 2] & 0xc7 == 0x05 {
                let disp = i32::from_le_bytes(data[i + 3..i + 7].try_into().unwrap()) as i64;
                let rva = s.rva + i as u64;
                if (rva + 7).checked_add_signed(disp) == Some(target) {
                    refs.push(serde_json::json!({"rva":rva,"bytes":hex::encode(&data[i.saturating_sub(16)..(i+64).min(data.len())])}));
                }
            }
        }
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"schema":"lycheedev.source-discovery.v1","module":pe.sha256,"sourceValue":target,"refs":refs,"executionAdmission":false,"semanticProof":false})).unwrap());
}
