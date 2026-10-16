//! Exact immutable membership, compacted per admitted file and syntax kind.
//! A packed range is lossless for two u32 offsets; kind and FileId are separate
//! keys. Equal-range expressions of different kinds are never conflated.
#![forbid(unsafe_code)]

use super::{BindingSite, FileId, LuaSyntaxId};
use emmylua_parser::LuaSyntaxKind;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct ManagedSites(BTreeMap<FileId, ManagedFileSites>);

#[derive(Default)]
pub(super) struct ManagedFileSites {
    names: Box<[u64]>,
    indexes: Box<[u64]>,
    calls: Box<[u64]>,
}

#[derive(Default)]
pub(super) struct ManagedFileBuilder {
    names: Vec<u64>,
    indexes: Vec<u64>,
    calls: Vec<u64>,
}
fn range_key(syntax: LuaSyntaxId) -> u64 {
    let range = syntax.get_range();
    (u64::from(u32::from(range.start())) << 32) | u64::from(u32::from(range.end()))
}
impl ManagedFileBuilder {
    pub(super) fn observe(&mut self, syntax: LuaSyntaxId) {
        let target = match syntax.get_kind() {
            LuaSyntaxKind::NameExpr => &mut self.names,
            LuaSyntaxKind::IndexExpr => &mut self.indexes,
            LuaSyntaxKind::CallExpr => &mut self.calls,
            _ => return,
        };
        target.push(range_key(syntax));
    }
    pub(super) fn finish(self) -> ManagedFileSites {
        fn pack(mut ranges: Vec<u64>) -> Box<[u64]> {
            ranges.sort_unstable();
            ranges.dedup();
            ranges.into_boxed_slice()
        }
        ManagedFileSites {
            names: pack(self.names),
            indexes: pack(self.indexes),
            calls: pack(self.calls),
        }
    }
}
impl ManagedSites {
    pub(super) fn insert(&mut self, file: FileId, sites: ManagedFileSites) {
        self.0.insert(file, sites);
    }
    pub(super) fn contains_read(&self, site: &BindingSite) -> bool {
        let Some(file) = self.0.get(&site.file_id) else {
            return false;
        };
        let ranges = match site.syntax.get_kind() {
            LuaSyntaxKind::NameExpr => &file.names,
            LuaSyntaxKind::IndexExpr => &file.indexes,
            _ => return false,
        };
        ranges.binary_search(&range_key(site.syntax)).is_ok()
    }
    pub(super) fn contains_call(&self, site: &BindingSite) -> bool {
        if site.syntax.get_kind() != LuaSyntaxKind::CallExpr {
            return false;
        }
        self.0
            .get(&site.file_id)
            .is_some_and(|file| file.calls.binary_search(&range_key(site.syntax)).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rowan::TextRange;
    use std::collections::HashSet;

    fn syntax(kind: LuaSyntaxKind, start: u32, end: u32) -> LuaSyntaxId {
        LuaSyntaxId::new(kind.into(), TextRange::new(start.into(), end.into()))
    }

    #[test]
    fn packed_membership_preserves_file_kind_duplicate_and_extreme_range_identities() {
        let mut actual = ManagedSites::default();
        let mut reads = HashSet::new();
        let mut calls = HashSet::new();
        let kinds = [
            LuaSyntaxKind::NameExpr,
            LuaSyntaxKind::IndexExpr,
            LuaSyntaxKind::CallExpr,
            LuaSyntaxKind::LiteralExpr,
        ];
        for file in [0, 17, u32::MAX] {
            let file_id = FileId::new(file);
            let mut builder = ManagedFileBuilder::default();
            for i in 0..1024u32 {
                for kind in kinds {
                    let start = i % 127;
                    let syntax = syntax(kind, start, start + i % 11);
                    builder.observe(syntax);
                    let site = BindingSite { file_id, syntax };
                    if matches!(kind, LuaSyntaxKind::NameExpr | LuaSyntaxKind::IndexExpr) {
                        reads.insert(site);
                    }
                    if kind == LuaSyntaxKind::CallExpr {
                        calls.insert(site);
                    }
                }
            }
            let extreme = syntax(LuaSyntaxKind::NameExpr, u32::MAX, u32::MAX);
            builder.observe(extreme);
            reads.insert(BindingSite {
                file_id,
                syntax: extreme,
            });
            actual.insert(file_id, builder.finish());
        }
        for file in [0, 1, 17, u32::MAX] {
            for start in 0..140u32 {
                for end in start..start + 13 {
                    for kind in kinds {
                        let site = BindingSite {
                            file_id: FileId::new(file),
                            syntax: syntax(kind, start, end),
                        };
                        assert_eq!(actual.contains_read(&site), reads.contains(&site));
                        assert_eq!(actual.contains_call(&site), calls.contains(&site));
                    }
                }
            }
            for kind in kinds {
                let site = BindingSite {
                    file_id: FileId::new(file),
                    syntax: syntax(kind, u32::MAX, u32::MAX),
                };
                assert_eq!(actual.contains_read(&site), reads.contains(&site));
                assert_eq!(actual.contains_call(&site), calls.contains(&site));
            }
        }
    }

    #[test]
    fn packed_membership_keeps_one_u64_per_unique_site_without_hash_capacity() {
        let file_id = FileId::new(1);
        let mut builder = ManagedFileBuilder::default();
        let mut reads = HashSet::new();
        let mut calls = HashSet::new();
        for start in 0..100_000u32 {
            let kind = match start % 3 {
                0 => LuaSyntaxKind::NameExpr,
                1 => LuaSyntaxKind::IndexExpr,
                _ => LuaSyntaxKind::CallExpr,
            };
            let syntax = syntax(kind, start, start + 1);
            builder.observe(syntax);
            let site = BindingSite { file_id, syntax };
            if kind == LuaSyntaxKind::CallExpr {
                calls.insert(site);
            } else {
                reads.insert(site);
            }
        }
        let packed = builder.finish();
        let count = packed.names.len() + packed.indexes.len() + packed.calls.len();
        assert_eq!(count, reads.len() + calls.len());
        let packed_range_payload = count * std::mem::size_of::<u64>();
        // Even the old initialized payload (excluding empty buckets, hash
        // controls and allocator overhead) is at least twice the packed ranges.
        let old_initialized_payload = count * std::mem::size_of::<BindingSite>();
        assert!(packed_range_payload * 2 <= old_initialized_payload);
        assert_eq!(packed_range_payload, 800_000);
    }
}
