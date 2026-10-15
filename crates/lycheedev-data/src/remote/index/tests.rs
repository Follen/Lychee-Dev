use super::*;

fn key(value: u128) -> String {
    format!("{value:032x}")
}
struct Fixture {
    raw: Vec<u8>,
    key: String,
    page_bytes: usize,
    pages: usize,
    stride: usize,
}
impl Fixture {
    fn new(offset_bytes: usize, entries: usize) -> Self {
        let page_bytes = 1024;
        let stride = 20 + offset_bytes;
        let per_page = page_bytes / stride;
        let pages = entries.div_ceil(per_page);
        let mut raw = vec![0; pages * (page_bytes + 24) + 28];
        for i in 0..entries {
            let start = (i / per_page) * page_bytes + (i % per_page) * stride;
            raw[start..start + 16].copy_from_slice(&(i as u128 + 1).to_be_bytes());
            raw[start + 16..start + 20].copy_from_slice(&8u32.to_be_bytes());
            if offset_bytes > 0 {
                raw[start + stride - 4..start + stride].copy_from_slice(&42u32.to_be_bytes());
            }
            if offset_bytes == 5 {
                raw[start + 20] = 1;
            }
            if offset_bytes == 6 {
                raw[start + 20..start + 22].copy_from_slice(&1u16.to_be_bytes());
            }
        }
        for p in 0..pages {
            let last = (entries.min((p + 1) * per_page)) as u128;
            let start = pages * page_bytes + p * 16;
            raw[start..start + 16].copy_from_slice(&last.to_be_bytes());
        }
        let start = raw.len() - 28;
        raw[start + 8] = 1;
        raw[start + 11] = 1;
        raw[start + 12] = offset_bytes as u8;
        raw[start + 13] = 4;
        raw[start + 14] = 16;
        raw[start + 15] = 8;
        raw[start + 16..start + 20].copy_from_slice(&(entries as u32).to_le_bytes());
        let mut result = Self {
            raw,
            key: String::new(),
            page_bytes,
            pages,
            stride,
        };
        result.reseal();
        result
    }
    fn reseal(&mut self) {
        for p in 0..self.pages {
            let hash = Md5::digest(&self.raw[p * self.page_bytes..(p + 1) * self.page_bytes]);
            let start = self.pages * self.page_bytes + self.pages * 16 + p * 8;
            self.raw[start..start + 8].copy_from_slice(&hash[..8]);
        }
        let start = self.raw.len() - 28;
        let hash = Md5::digest(&self.raw[self.pages * self.page_bytes..start]);
        self.raw[start..start + 8].copy_from_slice(&hash[..8]);
        let mut check = [0u8; 20];
        check[..12].copy_from_slice(&self.raw[start + 8..start + 20]);
        self.raw[start + 20..].copy_from_slice(&Md5::digest(check)[..8]);
        self.key = crate::casc::hex(&Md5::digest(&self.raw[start..]));
    }
    fn footer(&self) -> Result<Footer> {
        Footer::parse(
            self.raw.len() as i64,
            &self.key,
            &self.raw[self.raw.len() - 28..],
        )
    }
    fn directory(&self) -> Result<Directory> {
        let footer = self.footer()?;
        let (offset, length) = footer.toc_span();
        Directory::parse(
            footer,
            self.raw[offset as usize..offset as usize + length].to_vec(),
            &OperationControl::default(),
        )
    }
    fn locate(&self, needle: &str, archives: &[String]) -> Result<Span> {
        let control = OperationControl::default();
        let index = self.directory()?;
        let p = index.page(needle, &control)?;
        index.locate(
            p,
            needle,
            &self.raw[p.offset as usize..p.offset as usize + p.bytes],
            archives,
            &key(999),
            &control,
        )
    }
}
#[test]
fn footer_toc_selected_page_and_all_offset_layouts_are_authenticated() {
    for width in [0, 4, 5, 6] {
        let f = Fixture::new(width, 110);
        for n in [1, 51, 110] {
            let span = f.locate(&key(n), &[key(800), key(801)]).unwrap();
            assert_eq!(span.bytes, 8);
            assert_eq!(span.offset, if width == 0 { 0 } else { 42 });
            assert_eq!(
                span.object,
                if width == 0 {
                    key(n)
                } else if width == 4 {
                    key(999)
                } else {
                    key(801)
                }
            );
        }
        assert_eq!(
            f.locate(&key(111), &[]).unwrap_err().code,
            "records.content_missing"
        );
        assert_ne!(crate::casc::hex(&Md5::digest(&f.raw)), f.key);
    }
}
#[test]
fn empty_index_has_no_page_and_filename_authenticates_only_footer() {
    let mut f = Fixture::new(4, 0);
    assert_eq!(
        f.directory()
            .unwrap()
            .page(&key(1), &OperationControl::default())
            .unwrap_err()
            .code,
        "records.content_missing"
    );
    f.key = key(1);
    assert_eq!(
        f.footer().unwrap_err().code,
        "records.configuration_integrity"
    );
}
#[test]
fn footer_rejects_limits_layout_size_and_inner_checksum() {
    for (position, value, code) in [
        (8, 2, "records.invalid_metadata"),
        (9, 1, "records.invalid_metadata"),
        (10, 1, "records.invalid_metadata"),
        (11, 0, "records.metadata_limit"),
        (12, 3, "records.invalid_metadata"),
        (13, 3, "records.invalid_metadata"),
        (14, 15, "records.invalid_metadata"),
        (15, 7, "records.invalid_metadata"),
    ] {
        let mut f = Fixture::new(4, 2);
        let start = f.raw.len() - 28;
        f.raw[start + position] = value;
        f.reseal();
        assert_eq!(f.footer().unwrap_err().code, code, "field {position}");
    }
    let mut f = Fixture::new(4, 2);
    let start = f.raw.len() - 28;
    f.raw[start + 16..start + 20].copy_from_slice(&10_000_001u32.to_le_bytes());
    f.reseal();
    assert_eq!(f.footer().unwrap_err().code, "records.metadata_limit");
    let f = Fixture::new(4, 2);
    assert_eq!(
        Footer::parse(f.raw.len() as i64 + 1, &f.key, &f.raw[f.raw.len() - 28..])
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    assert_eq!(
        Footer::parse((1 << 30) + 1, &f.key, &f.raw[f.raw.len() - 28..])
            .unwrap_err()
            .code,
        "records.metadata_limit"
    );
    let mut f = Fixture::new(4, 2);
    let start = f.raw.len() - 28;
    f.raw[start + 20] ^= 1;
    f.key = crate::casc::hex(&Md5::digest(&f.raw[start..]));
    assert_eq!(
        f.footer().unwrap_err().code,
        "records.configuration_integrity"
    );
}
#[test]
fn corrupt_toc_or_selected_page_is_terminal() {
    let mut f = Fixture::new(4, 2);
    let start = f.pages * f.page_bytes;
    f.raw[start] ^= 1;
    assert_eq!(
        f.directory().err().unwrap().code,
        "records.configuration_integrity"
    );
    let mut f = Fixture::new(4, 2);
    f.raw[17] ^= 1;
    assert_eq!(
        f.locate(&key(1), &[]).unwrap_err().code,
        "records.configuration_integrity"
    );
}
#[test]
fn authenticated_toc_requires_nonzero_strictly_increasing_keys() {
    for duplicate in [false, true] {
        let mut f = Fixture::new(4, 100);
        let start = f.pages * f.page_bytes;
        if duplicate {
            let previous = f.raw[start..start + 16].to_vec();
            f.raw[start + 16..start + 32].copy_from_slice(&previous);
        } else {
            f.raw[start..start + 16].fill(0);
        }
        f.reseal();
        assert_eq!(
            f.directory().err().unwrap().code,
            "records.invalid_metadata"
        );
    }
}
#[test]
fn authenticated_page_rejects_zero_duplicate_cross_page_and_padding() {
    for mutation in 0..5 {
        let mut f = Fixture::new(4, 100);
        let per_page = f.page_bytes / f.stride;
        let needle = if mutation == 2 {
            key(per_page as u128 + 1)
        } else {
            key(1)
        };
        match mutation {
            0 => f.raw[..16].fill(0),
            1 => {
                let first = f.raw[..16].to_vec();
                f.raw[f.stride..f.stride + 16].copy_from_slice(&first);
            }
            2 => f.raw[f.page_bytes..f.page_bytes + 16]
                .copy_from_slice(&(per_page as u128).to_be_bytes()),
            3 => f.raw[per_page * f.stride] = 1,
            _ => {
                let last = (per_page - 1) * f.stride;
                f.raw[last..last + 16].copy_from_slice(&(per_page as u128 + 1).to_be_bytes());
            }
        }
        f.reseal();
        assert_eq!(
            f.locate(&needle, &[]).unwrap_err().code,
            "records.invalid_metadata",
            "mutation {mutation}"
        );
    }
}
#[test]
fn matched_span_validates_archive_ordinal_length_and_object() {
    for width in [5, 6] {
        let f = Fixture::new(width, 2);
        assert_eq!(
            f.locate(&key(1), &[key(800)]).unwrap_err().code,
            "records.invalid_metadata"
        );
    }
    let mut f = Fixture::new(4, 2);
    f.raw[16..20].copy_from_slice(&7u32.to_be_bytes());
    f.reseal();
    assert_eq!(
        f.locate(&key(1), &[]).unwrap_err().code,
        "records.invalid_metadata"
    );
    // Nonmatching entries still have structural ordering/padding authenticated,
    // but their archive ordinals/lengths are not interpreted by the Go lookup.
    assert_eq!(f.locate(&key(2), &[]).unwrap().bytes, 8);
    let f = Fixture::new(6, 2);
    assert_eq!(
        f.locate(&key(1), &[key(800), "bad".into()])
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
}
#[test]
fn cancellation_applies_before_parse_page_or_lookup() {
    let f = Fixture::new(4, 2);
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    let footer = f.footer().unwrap();
    let (offset, length) = footer.toc_span();
    assert_eq!(
        Directory::parse(
            footer,
            f.raw[offset as usize..offset as usize + length].to_vec(),
            &control
        )
        .err()
        .unwrap()
        .code,
        "command.cancelled"
    );
    let index = f.directory().unwrap();
    assert_eq!(
        index.page(&key(1), &control).unwrap_err().code,
        "command.cancelled"
    );
    let request = index.page(&key(1), &OperationControl::default()).unwrap();
    assert_eq!(
        index
            .locate(
                request,
                &key(1),
                &f.raw[..f.page_bytes],
                &[],
                &key(999),
                &control
            )
            .unwrap_err()
            .code,
        "command.cancelled"
    );
}
