//! Same-scope incoming file rows that cannot change under direction=both.
//! File identity and XML ownership are authenticated by the borrowed projection;
//! selected/outgoing files and the separate XML tail always use the original path.
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use lycheedev_core::Result;

pub(crate) struct FileProjectionRange {
    pub(super) file: usize,
    pub(super) first: usize,
    pub(super) end: usize,
    pub(super) steps: usize,
    pub(super) unresolved: usize,
}

// The enclosing RetainedProjection applies the existing 1 MiB serialized
// retention policy to BOTH these metadata and rows. This is not an RSS bound.
// Only owned JSON rows move here; no source, syntax index or graph is cloned.
#[derive(Serialize)]
pub(crate) struct ContextFiles(BTreeMap<usize, FileProjection>);

#[derive(Serialize)]
pub(super) struct FileProjection {
    rows: Vec<Value>,
    steps: usize,
    unresolved: usize,
}

impl ContextFiles {
    pub(super) fn from_projection(mut result: Value, ranges: Vec<FileProjectionRange>, mut poll: impl FnMut() -> Result<()>) -> Result<Self> {
        poll()?;
        let Value::Array(rows) = result["relations"].take() else {
            return Err(super::input_error("reference projection rows missing"));
        };
        // Reference-operation evidence already owns its analysis summary. Do
        // not keep another graphControl tree alive during Type/Definition work.
        drop(result);
        let mut rows = rows.into_iter();
        let mut consumed = 0;
        let mut files = BTreeMap::new();
        for range in ranges {
            poll()?;
            if range.first < consumed || range.end < range.first || files.contains_key(&range.file) {
                return Err(super::input_error("context file projection range differs"));
            }
            while consumed < range.first {
                poll()?;
                rows.next().ok_or_else(|| super::input_error("context file projection start outside rows"))?;
                consumed += 1;
            }
            let mut retained = Vec::with_capacity(range.end - range.first);
            while consumed < range.end {
                poll()?;
                retained.push(rows.next().ok_or_else(|| super::input_error("context file projection end outside rows"))?);
                consumed += 1;
            }
            files.insert(range.file, FileProjection { rows: retained, steps: range.steps, unresolved: range.unresolved });
        }
        // Remaining source rows and XML references/copies are not reusable.
        // Dropping this iterator cannot truncate the later original traversal.
        drop(rows);
        poll()?;
        Ok(Self(files))
    }

    pub(super) fn take(&mut self, file: usize) -> Option<FileProjection> { self.0.remove(&file) }
    pub(super) fn is_empty(&self) -> bool { self.0.is_empty() }
}

#[cfg(test)]
thread_local! { static REUSED_FILES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(super) fn reused_files() -> usize { REUSED_FILES.with(|count| count.get()) }

impl FileProjection {
    pub(super) fn steps(&self) -> usize { self.steps }
    pub(super) fn append(self, rows: &mut Vec<Value>, unresolved: &mut usize, mut poll: impl FnMut() -> Result<()>, mut step: impl FnMut() -> Result<()>) -> Result<()> {
        poll()?;
        // The caller has proved all these mixed original costs fit strictly
        // below the bounded return reserve. Near the edge it reconstructs the
        // file instead, preserving the original step/direct-charge ordering.
        for _ in 0..self.steps { step()?; }
        for row in self.rows { poll()?; rows.push(row); }
        *unresolved += self.unresolved;
        poll()?;
        #[cfg(test)]
        REUSED_FILES.with(|count| count.set(count.get() + 1));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn context_files_move_only_selected_ranges_in_original_order() {
        let row = json!({"path":"界.xml","position":{"line":1,"character":2},"text":"😀\r\n"});
        let pointer = row["text"].as_str().unwrap().as_ptr();
        let expected = serde_json::to_vec(&row).unwrap();
        let mut result = json!({"relations":[{"skip":0},null,{"skip":"xml-tail"}],"analysis":{"graphControl":{"notRetained":true}}});
        result["relations"][1] = row;
        let mut files = ContextFiles::from_projection(result, vec![
            FileProjectionRange {file:2,first:1,end:2,steps:1,unresolved:1},
            FileProjectionRange {file:4,first:2,end:2,steps:0,unresolved:0},
        ], || Ok(())).unwrap();
        assert!(serde_json::to_string(&files).unwrap().find("graphControl").is_none());
        let mut rows = Vec::new(); let mut unresolved = 0; let mut steps = 0;
        files.take(2).unwrap().append(&mut rows, &mut unresolved, || Ok(()), || {steps+=1;Ok(())}).unwrap();
        files.take(4).unwrap().append(&mut rows, &mut unresolved, || Ok(()), || {steps+=1;Ok(())}).unwrap();
        assert!(files.is_empty()); assert_eq!(steps,1); assert_eq!(unresolved,1);
        assert_eq!(rows.len(),1); assert_eq!(serde_json::to_vec(&rows[0]).unwrap(),expected);
        assert_eq!(rows[0]["text"].as_str().unwrap().as_ptr(),pointer);
    }

    #[test]
    fn context_files_replay_stops_at_the_original_step_and_checks_empty_files() {
        for stop in 1..=3 {
            let file=FileProjection{rows:vec![Value::Null],steps:3,unresolved:1};
            let mut rows=Vec::new();let mut unresolved=0;let mut used=0;
            let error=file.append(&mut rows,&mut unresolved,||Ok(()),||{
                used+=1;if used==stop{Err(super::super::input_error("test budget"))}else{Ok(())}
            }).unwrap_err();
            assert_eq!(error.message,"test budget");assert_eq!(used,stop);assert!(rows.is_empty());assert_eq!(unresolved,0);
        }
        for stop in [1,2] {
            let file=FileProjection{rows:vec![],steps:0,unresolved:0};
            let mut seen=0;
            assert!(file.append(&mut Vec::new(),&mut 0,||{
                seen+=1;if seen==stop{Err(super::super::input_error("test cancellation"))}else{Ok(())}
            },||panic!("empty file has no charged query steps")).is_err());
            assert_eq!(seen,stop);
        }
    }
}
