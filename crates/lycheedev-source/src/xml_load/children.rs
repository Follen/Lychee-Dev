//! Ordered child membership for an already extracted XML document.
//!
//! The extractor bounds occurrences and assigns dense ordinals. Keep one flat
//! child array and its offsets rather than scanning all occurrences at every
//! expansion. This index has no ownership of source bytes, catalog, or control;
//! the enclosing manifest Walker drops it together with the parsed document.
#![forbid(unsafe_code)]

use crate::xml_program::{MAX_OCCURRENCES, XmlProgram};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};

pub(super) struct ChildIndex {
    offsets: Vec<usize>,
    ordinals: Vec<usize>,
}

impl ChildIndex {
    pub(super) fn build(program: &XmlProgram, remaining_bytes: usize, control: &OperationControl) -> Result<Option<Self>> {
        control.check()?;
        let count = program.occurrences.len();
        // This is the existing extractor ceiling, not a smaller cache budget.
        // A production parse has already established both this and the dense
        // ordinal invariant. Validate before indexing or allocating regardless.
        if count > MAX_OCCURRENCES {
            return Err(invalid("XML child index exceeds extractor occurrence ceiling"));
        }
        // Reserve both retained arrays and the temporary fill cursors before
        // allocation. Other parsed documents retain their share of this same
        // Walker-local cap. Failure to fit selects the original complete scan.
        let construction_bytes = (3 * count + 3) * std::mem::size_of::<usize>();
        if construction_bytes > remaining_bytes {
            return Ok(None);
        }
        let mut offsets = vec![0usize; count + 2];
        for (ordinal, node) in program.occurrences.iter().enumerate() {
            control.check()?;
            if node.ordinal != ordinal || node.parent.is_some_and(|parent| parent >= count) {
                return Err(invalid("XML child index requires extracted occurrence identities"));
            }
            let key = node.parent.map_or(0, |parent| parent + 1);
            offsets[key + 1] += 1;
        }
        for key in 1..offsets.len() {
            control.check()?;
            offsets[key] += offsets[key - 1];
        }
        // At most (3 * count + 3) usize slots during construction, then
        // (2 * count + 2). Both are bounded by MAX_OCCURRENCES. A single pass
        // preserves the exact extractor order, including synthetic references.
        let mut next = offsets[..count + 1].to_vec();
        let mut ordinals = vec![0; count];
        for node in &program.occurrences {
            control.check()?;
            let key = node.parent.map_or(0, |parent| parent + 1);
            ordinals[next[key]] = node.ordinal;
            next[key] += 1;
        }
        control.check()?;
        Ok(Some(Self { offsets, ordinals }))
    }

    pub(super) fn retained_bytes(&self) -> usize {
        (self.offsets.len() + self.ordinals.len()) * std::mem::size_of::<usize>()
    }

    pub(super) fn get(&self, parent: Option<usize>) -> &[usize] {
        let key = parent.map_or(0, |parent| parent + 1);
        &self.ordinals[self.offsets[key]..self.offsets[key + 1]]
    }
}

fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("source.native_invalid_input", message, 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> XmlProgram {
        crate::xml_program::extract_production("Frames.xml", text.as_bytes()).unwrap()
    }

    #[test]
    fn indexed_children_match_complete_scan_for_every_parent() {
        for text in [
            "<Ui/>",
            "\u{feff}<Ui><Unknown><Scripts><OnLoad method=\"Run\"/><OnShow function=\"Callback\"/></Scripts><Layer/><Layer/></Unknown><Frame name=\"界\"><Scripts><OnLoad>local v=&quot;😀&quot;</OnLoad></Scripts></Frame></Ui>",
            "<Bindings>text<Binding name=\"ACTION\">Target(keystate)</Binding><ModifiedClick name=\"OTHER\"/></Bindings>",
        ] {
            let program = parsed(text);
            let index = ChildIndex::build(&program, usize::MAX, &OperationControl::default()).unwrap().unwrap();
            for parent in std::iter::once(None).chain((0..program.occurrences.len()).map(Some)) {
                let scanned = program.occurrences.iter().filter(|node| node.parent == parent)
                    .map(|node| node.ordinal).collect::<Vec<_>>();
                assert_eq!(index.get(parent), scanned);
            }
            assert_eq!(index.ordinals.len(), program.occurrences.len());
            assert_eq!(index.offsets.len(), program.occurrences.len() + 2);
        }
    }

    #[test]
    fn wide_document_preserves_all_siblings_and_bounded_storage() {
        let program = parsed(&format!("<Ui>{}</Ui>", "<Layer/>".repeat(4096)));
        let index = ChildIndex::build(&program, usize::MAX, &OperationControl::default()).unwrap().unwrap();
        assert_eq!(index.get(None), [0]);
        assert_eq!(index.get(Some(0)), (1..4097).collect::<Vec<_>>());
        assert!(index.get(Some(4096)).is_empty());
        assert_eq!(index.offsets.len() + index.ordinals.len(), 2 * program.occurrences.len() + 2);
    }

    #[test]
    fn cancellation_remains_the_original_typed_control_error() {
        let program = parsed("<Ui><Frame/></Ui>");
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        let expected = control.check().unwrap_err();
        let actual = ChildIndex::build(&program, usize::MAX, &control).err().unwrap();
        assert_eq!(actual.code, expected.code);
        assert_eq!(actual.message, expected.message);
    }

    #[test]
    fn scratch_admission_falls_back_before_allocating_and_checks_control() {
        let program = parsed("<Ui><Frame/><Frame/></Ui>");
        let control = OperationControl::default();
        let needed = (3 * program.occurrences.len() + 3) * std::mem::size_of::<usize>();
        assert!(ChildIndex::build(&program, needed - 1, &control).unwrap().is_none());
        let admitted = ChildIndex::build(&program, needed, &control).unwrap().unwrap();
        assert_eq!(admitted.retained_bytes(), (2 * program.occurrences.len() + 2) * std::mem::size_of::<usize>());
        assert!(admitted.retained_bytes() < needed);
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(ChildIndex::build(&program, 0, &control).err().unwrap().code, control.check().unwrap_err().code);
    }

    #[test]
    fn invalid_internal_identity_is_rejected_before_indexing() {
        let control = OperationControl::default();
        let mut program = parsed("<Ui><Frame/></Ui>");
        program.occurrences[1].ordinal = 0;
        assert_eq!(ChildIndex::build(&program, usize::MAX, &control).err().unwrap().code, "source.native_invalid_input");
        program.occurrences[1].ordinal = 1;
        program.occurrences[1].parent = Some(usize::MAX);
        assert_eq!(ChildIndex::build(&program, usize::MAX, &control).err().unwrap().code, "source.native_invalid_input");
    }
}
