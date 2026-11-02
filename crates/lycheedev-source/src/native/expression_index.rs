//! Immutable exact original-byte identities from one file's admitted AST walk.
//! This is a lookup representation, not a second parse or semantic cache.
#![forbid(unsafe_code)]

pub(super) struct ExpressionIndex<T> {
    entries: Box<[(u32, u32, T)]>,
}
impl<T: Copy> ExpressionIndex<T> {
    pub(super) fn build<E>(
        mut entries: Vec<(u32, u32, T)>,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Self, E> {
        check()?;
        // Stable ordering retains traversal order for equal ranges. The old
        // BTreeMap replaced an earlier entry on equal keys; keep that exact
        // last-write behavior, including zero-width parser identities.
        // Sorting scratch belongs only to this file and dies before the next
        // file is admitted. The original AST-node ceiling still bounds rows.
        entries.sort_by_key(|entry| (entry.0, entry.1));
        check()?;
        entries.dedup_by(|later, earlier| {
            if (later.0, later.1) == (earlier.0, earlier.1) {
                *earlier = *later;
                true
            } else {
                false
            }
        });
        check()?;
        let entries = entries.into_boxed_slice();
        check()?;
        Ok(Self { entries })
    }

    pub(super) fn get(&self, start: u32, end: u32) -> Option<&T> {
        self.entries
            .binary_search_by_key(&(start, end), |entry| (entry.0, entry.1))
            .ok()
            .map(|index| &self.entries[index].2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn packed_lookup_matches_map_with_nested_equal_and_zero_width_ranges() {
        let mut rows = vec![
            (8, 12, 1),
            (0, 20, 2),
            (0, 8, 3),
            (8, 12, 4),
            (0, 0, 5),
            (0, 20, 6),
            (u32::MAX, u32::MAX, 7),
        ];
        for i in 0..4096u32 {
            rows.push((i % 113, i % 113 + i % 7, i + 10));
        }
        let expected = rows
            .iter()
            .map(|&(start, end, id)| ((start, end), id))
            .collect::<BTreeMap<_, _>>();
        let index = ExpressionIndex::build(rows, || Ok::<_, ()>(())).unwrap();
        assert_eq!(index.entries.len(), expected.len());
        for start in 0..128 {
            for end in 0..128 {
                assert_eq!(index.get(start, end), expected.get(&(start, end)));
            }
        }
        assert_eq!(index.get(u32::MAX, u32::MAX), Some(&7));
        assert!(index.get(u32::MAX, 0).is_none());
    }

    #[test]
    fn empty_and_single_indexes_preserve_exact_not_covering_lookup() {
        let empty = ExpressionIndex::<u32>::build(vec![], || Ok::<_, ()>(())).unwrap();
        assert!(empty.get(0, 0).is_none());
        let index = ExpressionIndex::build(vec![(3, 7, 9)], || Ok::<_, ()>(())).unwrap();
        assert_eq!(index.get(3, 7), Some(&9));
        assert!(index.get(3, 6).is_none());
        assert!(index.get(4, 7).is_none());
        assert!(index.get(7, 3).is_none());
    }

    #[test]
    fn every_compaction_boundary_preserves_original_control_error() {
        for stopped in 1..=4 {
            let mut checks = 0;
            let error = ExpressionIndex::build(vec![(3, 7, 9), (3, 7, 10)], || {
                checks += 1;
                if checks == stopped {
                    Err(("original-control-stop", stopped))
                } else {
                    Ok(())
                }
            })
            .err()
            .expect("control must stop construction");
            assert_eq!(error, ("original-control-stop", stopped));
            assert_eq!(checks, stopped);
        }
    }
}
