//! Query-local borrowed observations, with one flat pointer array instead of a
//! B-tree node and independently allocated Vec for every observed source site.
use super::{Site, UseFact};
use serde_json::Value;

#[derive(Default)]
pub(super) struct ResearchUses<'a> {
    rows: Vec<&'a UseFact>,
}
impl<'a> ResearchUses<'a> {
    pub(super) fn push(&mut self, fact: &'a UseFact) {
        self.rows.push(fact);
    }
    pub(super) fn finish<E>(&mut self, mut check: impl FnMut() -> Result<(), E>) -> Result<(), E> {
        check()?;
        // Stable ties preserve the former per-site Vec's exact graph traversal
        // order. The unchanged graph construction ceiling bounds this sort;
        // its scratch dies here, before any relation JSON is allocated.
        self.rows
            .sort_by(|left, right| key(&left.site).cmp(&key(&right.site)));
        check()
    }
    pub(super) fn get(&self, site: &Site) -> Option<&[&'a UseFact]> {
        let key = key(site);
        let first = self.rows.partition_point(|row| self::key(&row.site) < key);
        let count = self.rows[first..].partition_point(|row| self::key(&row.site) == key);
        (count > 0).then_some(&self.rows[first..first + count])
    }
}
fn key(site: &Site) -> (&str, u32, u32) {
    (&site.file, site.start, site.end)
}

pub(super) fn assemble_projection(rows: Vec<Value>, analysis: Value) -> Value {
    let mut result = serde_json::Map::new();
    result.insert("relations".into(), Value::Array(rows));
    result.insert("analysis".into(), analysis);
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::Decision;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn flat_observations_match_original_site_map_and_duplicate_order() {
        let facts = (0..4096)
            .map(|id| UseFact {
                id,
                site: Site {
                    file: format!("file-{}.lua", id % 11),
                    start: (id % 29) as u32,
                    end: (id % 29 + id % 7) as u32,
                },
                context: format!("context-{id}"),
                decision: Decision::NotManaged,
                writes: vec![],
                proof_id: format!("proof-{id}"),
                object_view: None,
                host_return_slot: None,
            })
            .collect::<Vec<_>>();
        let mut expected = BTreeMap::<_, Vec<_>>::new();
        let mut index = ResearchUses::default();
        for fact in &facts {
            expected.entry(key(&fact.site)).or_default().push(fact);
            index.push(fact);
        }
        index.finish(|| Ok::<_, ()>(())).unwrap();
        for fact in &facts {
            let actual = index.get(&fact.site).unwrap();
            let previous = &expected[&key(&fact.site)];
            assert_eq!(actual.len(), previous.len());
            for (left, right) in actual.iter().zip(previous) {
                assert!(std::ptr::eq(*left, *right));
            }
        }
        assert!(
            index
                .get(&Site {
                    file: "absent.lua".into(),
                    start: 0,
                    end: 0
                })
                .is_none()
        );
        assert!(ResearchUses::default().get(&facts[0].site).is_none());
    }

    #[test]
    fn flat_observation_sort_propagates_both_original_control_checks() {
        for stop in 1..=2 {
            let mut count = 0;
            let mut index = ResearchUses::default();
            assert_eq!(
                index.finish(|| {
                    count += 1;
                    if count == stop {
                        Err("original-stop")
                    } else {
                        Ok(())
                    }
                }),
                Err("original-stop")
            );
            assert_eq!(count, stop);
        }
    }

    #[test]
    fn relation_projection_moves_complete_json_and_preserves_wire_bytes() {
        let rows = vec![json!({"path":"界.xml","nested":{"proof":"same exact evidence"}})];
        let analysis = json!({"graphControl":{"paths":[1,2,3]},"state":"partial"});
        let expected = serde_json::to_vec(&json!({"relations":rows,"analysis":analysis})).unwrap();
        let rows_ptr = rows.as_ptr();
        let proof_ptr = rows[0]["nested"]["proof"].as_str().unwrap().as_ptr();
        let control_ptr = analysis["graphControl"]["paths"]
            .as_array()
            .unwrap()
            .as_ptr();
        let result = assemble_projection(rows, analysis);
        assert_eq!(result["relations"].as_array().unwrap().as_ptr(), rows_ptr);
        assert_eq!(
            result["relations"][0]["nested"]["proof"]
                .as_str()
                .unwrap()
                .as_ptr(),
            proof_ptr
        );
        assert_eq!(
            result["analysis"]["graphControl"]["paths"]
                .as_array()
                .unwrap()
                .as_ptr(),
            control_ptr
        );
        assert_eq!(serde_json::to_vec(&result).unwrap(), expected);
    }
}
