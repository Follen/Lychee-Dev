use super::*;
use installation::Resource;
use std::io::{Read, Seek, SeekFrom};
pub(super) const OWNER: &str = ".lycheedev-preparation.json";
const RETAINED_ROOTS: usize = 4;
/// Exclusive fixed slots bound only this target's unconfirmed preparations.
/// Successful cleanup releases its slot; existing/foreign entries are never
/// adopted by age, PID liveness or an untrusted ownership document.
pub(super) fn create_private(
    parent: &Path,
    prefix: &str,
    owner: &Path,
    control: &OperationControl,
) -> Result<PathBuf> {
    let key = storage::sha256(owner.to_string_lossy().to_lowercase().as_bytes());
    for slot in 0..RETAINED_ROOTS {
        control.check()?;
        let path = parent.join(format!("{prefix}{key}-{slot}"));
        storage::guard_path(parent, &path)?;
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
    }
    Err(ToolkitError::new("delivery.preparation_capacity", "retained or occupied preparations for this target require explicit review before another attempt", 6)
        .with_details(json!({"target":owner,"parent":parent,"namespace":format!("{prefix}{key}-"),"maximumRetainedRoots":RETAINED_ROOTS})))
}
pub(super) fn ownership(target: &Path, version: &str, commit: &str) -> Value {
    json!({"schema":"lycheedev.repair-preparation-owner.v1","target":target,"version":version,"commit":commit,"processId":std::process::id(),"createdAt":storage::utc_now(),"maximumPayloadBytes":256u64<<20,"maximumEntries":65536,"state":"preparation"})
}
pub(super) struct Snapshot {
    root: PathBuf,
    files: Vec<Resource>,
}
impl Snapshot {
    pub(super) fn root(&self) -> &Path {
        &self.root
    }
    pub(super) fn prepare(
        source: &Path,
        parent: &Path,
        manifest: &release::Release,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let mut snapshot = Self {
            root: create_private(
                parent,
                ".lycheedev-release-",
                &parent.join("Lychee Dev"),
                control,
            )?,
            files: vec![],
        };
        let result = (|| {
            snapshot.write(
                OWNER,
                &storage::bounded_json(
                    &ownership(
                        &parent.join("Lychee Dev"),
                        &manifest.version,
                        &manifest.commit,
                    ),
                    4096,
                )?,
                control,
            )?;
            for resource in &manifest.resources {
                control.check()?;
                let input = storage::safe_relative(&source.join("payload"), &resource.path)?;
                storage::guard_path(&source.join("payload"), &input)?;
                let bytes = read_owned(&input, resource, control)?;
                snapshot.write(&format!("payload/{}", resource.path), &bytes, control)?;
            }
            snapshot.write(
                "release.json",
                &storage::bounded_json(manifest, 1 << 20)?,
                control,
            )?;
            snapshot.verify(manifest, control)
        })();
        if let Err(mut error) = result {
            error.details = json!({"schema":"lycheedev.formal-repair-preparation.v1","retainedPreparation":snapshot.root,"cause":error.clone()});
            return Err(error);
        }
        Ok(snapshot)
    }
    fn write(&mut self, relative: &str, bytes: &[u8], control: &OperationControl) -> Result<()> {
        let path = storage::safe_relative(&self.root, relative)?;
        storage::guard_path(&self.root, &path)?;
        storage::atomic_write_controlled(&path, bytes, control)?;
        self.files.push(Resource {
            path: relative.into(),
            sha256: storage::sha256(bytes),
            bytes: bytes.len() as u64,
        });
        Ok(())
    }
    pub(super) fn verify(
        &self,
        original: &release::Release,
        control: &OperationControl,
    ) -> Result<()> {
        let observed = release::inspect_with_control(&self.root, &original.version, control)?;
        if serde_json::to_value(&observed)? != serde_json::to_value(original)? {
            return Err(conflict("independent release snapshot changed"));
        }
        Ok(())
    }
    pub(super) fn prepare_addon(
        &mut self,
        wanted: &Receipt,
        control: &OperationControl,
    ) -> Result<PathBuf> {
        self.write(
            &format!("payload/addon/{}", installation::MARKER),
            &storage::bounded_json(wanted, 1 << 20)?,
            control,
        )?;
        let path = self.root.join("payload/addon");
        require_receipt(
            &installation::inspect_main_controlled(&path, "addon", control)?,
            wanted,
            "prepared addon is not clean",
        )?;
        Ok(path)
    }
    pub(super) fn moved_addon(&mut self) {
        self.files.retain(|r| !r.path.starts_with("payload/addon/"));
    }
    pub(super) fn cleanup(self, control: &OperationControl) -> Result<()> {
        // Admit the entire owned tree before deleting the first file. A caller
        // cancellation or foreign file retains all remaining preparation.
        let result = super::cleanup::remove_exact(&self.root, &self.files, control);
        result.map_err(|mut error| {
            error.details=json!({"schema":"lycheedev.repair-preparation-cleanup.v1","retainedPreparation":self.root,"cause":error.clone()});error
        })
    }
}
fn read_owned(path: &Path, want: &Resource, control: &OperationControl) -> Result<Vec<u8>> {
    let (mut file, digest) = storage::locked_sha256(path, want.bytes, control)?;
    let result = (|| {
        if file.metadata()?.len() != want.bytes || digest != want.sha256 {
            return Err(conflict("release changed during snapshot preparation"));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        let mut block = [0u8; 65536];
        loop {
            control.check()?;
            let count = file.read(&mut block)?;
            if count == 0 {
                break;
            }
            if count as u64 > want.bytes.saturating_sub(bytes.len() as u64) {
                return Err(conflict("release resource grew"));
            }
            bytes.extend_from_slice(&block[..count]);
        }
        if bytes.len() as u64 != want.bytes || storage::sha256(&bytes) != want.sha256 {
            return Err(conflict("release resource changed"));
        }
        Ok(bytes)
    })();
    storage::finish_file(file, result, control)
}

#[cfg(test)]
impl Snapshot {
    pub(super) fn cleanup_with(
        self,
        control: &OperationControl,
        admitted: &mut impl FnMut(&Path, usize) -> Result<()>,
        before_delete: &mut impl FnMut() -> Result<()>,
    ) -> Result<()> {
        super::cleanup::remove_exact_with(&self.root, &self.files, control, admitted, before_delete)
    }
}
