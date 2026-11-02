//! Success-only private snapshot cleanup, bound to authenticated open objects.
use super::*;
use installation::Resource;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Component,
};
const MAX_NODES: usize = 65536;
struct Entry {
    path: PathBuf,
    directory: bool,
    held: storage::AuthenticatedCleanup,
}
#[derive(Default)]
struct Plan {
    ancestors: Vec<File>,
    entries: Vec<Entry>,
}
impl Plan {
    fn finish(
        mut self,
        result: Result<()>,
        mut delete: bool,
        root: &Path,
        control: &OperationControl,
    ) -> Result<()> {
        let mut failures = vec![];
        // The owner record survives ordinary partial cleanup: files, child
        // directories, owner receipt, then root. All identities remain held.
        self.entries.sort_by_key(|entry| {
            let priority = if entry.path == root {
                3
            } else if entry.path == root.join(snapshot::OWNER) {
                2
            } else if entry.directory {
                1
            } else {
                0
            };
            (priority, std::cmp::Reverse(entry.path.components().count()))
        });
        for entry in self.entries {
            if delete {
                if let Err(e) = control.check() {
                    append(&mut failures, e);
                    delete = false;
                }
            }
            let (file, operation) = if delete {
                let (file, result) = entry.held.discard();
                (file, result.map_err(ToolkitError::io))
            } else {
                (entry.held.into_file(), Ok(()))
            };
            if let Err(e) = storage::finish_file(file, operation, control) {
                append(&mut failures, e);
                delete = false;
            }
        }
        for file in self.ancestors.into_iter().rev() {
            if let Err(e) = storage::finish_file(file, Ok(()), control) {
                append(&mut failures, e);
            }
        }
        finish(result, failures.into_iter().map(Err), control)
    }
}
fn append(errors: &mut Vec<ToolkitError>, error: ToolkitError) {
    // A cancelled 60,000-entry tree still reports one original control cause,
    // while every distinct/nested I/O or close failure remains represented.
    if error.details.is_null()
        && matches!(
            error.code.as_str(),
            "command.cancelled" | "command.deadline"
        )
        && errors.iter().any(|e| {
            e.code == error.code && e.message == error.message && e.details == error.details
        })
    {
        return;
    }
    errors.push(error);
}
pub(super) fn remove_exact(
    root: &Path,
    files: &[Resource],
    control: &OperationControl,
) -> Result<()> {
    remove_exact_with(root, files, control, &mut |_, _| Ok(()), &mut || Ok(()))
}
pub(super) fn remove_exact_with(
    root: &Path,
    files: &[Resource],
    control: &OperationControl,
    admitted: &mut impl FnMut(&Path, usize) -> Result<()>,
    before_delete: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut plan = Plan::default();
    let result = (|| {
        control.check()?;
        let parent = root
            .parent()
            .ok_or_else(|| invalid("preparation parent missing"))?;
        if !root.is_absolute() || parent.components().count() > 1024 {
            return Err(invalid("preparation ancestry bound"));
        }
        let expected = files
            .iter()
            .map(|r| (r.path.as_str(), r))
            .collect::<BTreeMap<_, _>>();
        if expected.len() != files.len() {
            return Err(invalid("duplicate preparation cleanup path"));
        }
        let mut directories = BTreeSet::from([root.to_path_buf()]);
        for resource in files {
            control.check()?;
            let mut path = storage::safe_relative(root, &resource.path)?;
            while let Some(parent) = path.parent() {
                if parent == root {
                    break;
                }
                directories.insert(parent.to_path_buf());
                path = parent.to_path_buf();
            }
        }
        let handle_bound = expected
            .len()
            .checked_add(directories.len())
            .and_then(|n| n.checked_add(parent.components().count()))
            .ok_or_else(|| invalid("cleanup count overflow"))?;
        if handle_bound > MAX_NODES {
            return Err(invalid("cleanup handle/node admission exceeds 65536"));
        }
        // Reserve only the real inventory's maximum, not the global ceiling.
        plan.entries.reserve(expected.len() + directories.len());
        plan.ancestors.reserve(parent.components().count());
        let mut current = PathBuf::new();
        for component in parent.components() {
            control.check()?;
            current.push(component.as_os_str());
            match component {
                Component::Prefix(_) => continue,
                Component::RootDir | Component::Normal(_) => (),
                _ => return Err(invalid("noncanonical cleanup ancestor")),
            }
            // Establish each original ancestor before opening its descendant.
            // Deny-write/delete prevents both renaming and reparse mutation.
            plan.ancestors
                .push(storage::hold_cleanup_ancestor(&current, control)?);
        }
        let held = storage::authenticate_cleanup_directory(root, control)?;
        plan.entries.push(Entry {
            path: root.into(),
            directory: true,
            held,
        });
        let mut stack = vec![root.to_path_buf()];
        let mut seen = 0;
        let mut nodes = 1;
        while let Some(directory) = stack.pop() {
            control.check()?;
            for child in fs::read_dir(&directory)? {
                control.check()?;
                let child = child?;
                nodes += 1;
                if nodes > MAX_NODES {
                    return Err(invalid("preparation cleanup node budget"));
                }
                let path = child.path();
                storage::guard_path(root, &path)?;
                if child.file_type()?.is_dir() {
                    if !directories.contains(&path) {
                        return Err(conflict("foreign preparation directory retained"));
                    }
                    let held = storage::authenticate_cleanup_directory(&path, control)?;
                    plan.entries.push(Entry {
                        path: path.clone(),
                        directory: true,
                        held,
                    });
                    stack.push(path.clone());
                } else {
                    let relative = path
                        .strip_prefix(root)
                        .map_err(|_| invalid("cleanup escapes root"))?
                        .to_string_lossy()
                        .replace('\\', "/");
                    let wanted = expected
                        .get(relative.as_str())
                        .ok_or_else(|| conflict("foreign preparation file retained"))?;
                    let held = storage::authenticate_cleanup_file(
                        &path,
                        wanted.bytes,
                        &wanted.sha256,
                        control,
                    )?;
                    plan.entries.push(Entry {
                        path: path.clone(),
                        directory: false,
                        held,
                    });
                    seen += 1;
                }
                admitted(&path, plan.entries.len())?;
            }
        }
        if seen != expected.len() {
            return Err(conflict("preparation inventory changed before cleanup"));
        }
        before_delete()?;
        control.check()
    })();
    let delete = result.is_ok();
    plan.finish(result, delete, root, control)
}
