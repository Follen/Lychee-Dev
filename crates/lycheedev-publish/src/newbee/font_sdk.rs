//! Narrow official SDK boundary: folder content-index upload only. All backup
//! choices, bytes, manifests, author requests and recovery remain Rust-owned.
use super::backup_bundle::validation::LockedFont;
use super::*;
use crate::{desktop_runtime, local_backup, mutation::Mutation, provider::Provider};
use lycheedev_core::{process, storage};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::process::CommandExt,
    process::Command,
};

pub(super) struct FontSdk {
    binding: PathBuf,
    client_version: String,
    module_sha256: String,
}
pub(super) struct FontUpload {
    pub(super) index: String,
    pub(super) cleanup_pending: bool,
}
fn dependency() -> ToolkitError {
    ToolkitError::new(
        "publish.font_sdk",
        "font backups require a compatible publisher-verified NewBeeBox desktop SDK and Windows x64 Node.js",
        3,
    )
}
impl FontSdk {
    pub(super) fn cached(
        home: &Path,
        fonts: &[LockedFont],
        control: &OperationControl,
    ) -> Result<Option<FontUpload>> {
        let identity = font_identity(fonts);
        let hash = storage::sha256(&storage::identity_json(&identity)?);
        let mutation = Mutation::open(
            home,
            Provider::Newbee,
            "backup-font",
            u64::from_str_radix(&hash[..16], 16).unwrap().max(1),
            "upload",
            &identity,
            control,
        )?;
        if let Some(index) = mutation
            .evidence()
            .and_then(|value| value["font_hash"].as_str())
        {
            return Ok(Some(FontUpload {
                index: index.to_owned(),
                cleanup_pending: !cleanup(home, mutation.id().unwrap()),
            }));
        }
        if mutation.pending() {
            return Err(mutation.pending_error("the original font folder upload is uncertain; inspect its original content index before another upload"));
        }
        Ok(None)
    }
    pub(super) fn prepare(home: &Path, control: &OperationControl) -> Result<Self> {
        let installation = desktop_runtime::lookup(
            &desktop_runtime::Lookup {
                name_prefix: "NewBeeBox",
                program_subdirectory: None,
                executable: "NewBeeBox.exe",
                resource: "resources/app.asar",
                publishers: &["南宁新手村科技有限公司"],
            },
            control,
        )?;
        let mut source = local_backup::open_source(&installation.resource)?;
        let result = extract(&mut source, control);
        let bytes = storage::finish_file(source, result, control)?;
        let sha256 = storage::sha256(&bytes);
        let directory = home.join("publish/newbee/sdk").join(&sha256);
        fs::create_dir_all(&directory)?;
        let binding = directory.join("nbb-core.win32-x64-msvc.node");
        storage::guard_path_with_check(&directory, &binding, || control.check())?;
        let reuse = if binding.is_file() {
            let (file, hash) = storage::locked_sha256(&binding, 128 * 1024 * 1024, control)?;
            let matches = hash == sha256;
            storage::finish_file(file, Ok(matches), control)?
        } else {
            false
        };
        if !reuse {
            storage::atomic_write(&binding, &bytes)?;
        }
        Ok(Self {
            binding,
            client_version: installation.version,
            module_sha256: sha256,
        })
    }
    pub(super) fn upload(
        &self,
        home: &Path,
        fonts: &[LockedFont],
        control: &OperationControl,
    ) -> Result<FontUpload> {
        let identity = font_identity(fonts);
        let hash = storage::sha256(&storage::identity_json(&identity)?);
        let target = u64::from_str_radix(&hash[..16], 16).unwrap().max(1);
        let mut mutation = Mutation::open(
            home,
            Provider::Newbee,
            "backup-font",
            target,
            "upload",
            &identity,
            control,
        )?;
        if let Some(value) = mutation
            .evidence()
            .and_then(|value| value["font_hash"].as_str())
        {
            let index = value.to_owned();
            let cleanup_pending = !cleanup(home, mutation.id().unwrap());
            return Ok(FontUpload {
                index,
                cleanup_pending,
            });
        }
        if mutation.pending() {
            return Err(mutation.pending_error("the original font folder upload is uncertain; inspect its original content index before another upload"));
        }
        let (binding, module_hash) =
            storage::locked_sha256(&self.binding, 128 * 1024 * 1024, control)?;
        if module_hash != self.module_sha256 {
            return storage::finish_file(binding, Err(dependency()), control);
        }
        let result = (|| {
            mutation.prepare(control)?;
            let directory = home
                .join("publish/newbee/sdk/work")
                .join(mutation.id().unwrap());
            // The SDK uploads a whole directory. Copy only the admitted locked
            // files into this task's input, without collecting later additions.
            let staged = (|| {
                fs::create_dir_all(&directory)?;
                storage::guard_path_with_check(&directory, &directory, || control.check())?;
                for font in fonts {
                    let mut source = font.file.try_clone()?;
                    source.seek(SeekFrom::Start(0))?;
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(directory.join(&font.name))?;
                    let result = (|| {
                        let mut buffer = [0u8; 65536];
                        loop {
                            control.check()?;
                            let count = source.read(&mut buffer)?;
                            if count == 0 {
                                break;
                            }
                            output.write_all(&buffer[..count])?;
                        }
                        output.sync_all()?;
                        Ok(())
                    })();
                    let result = storage::finish_file(output, result, control);
                    storage::finish_file(source, result, control)?;
                }
                Ok(())
            })();
            staged.map_err(|error: ToolkitError| mutation.failure(not_sent(error)))?;
            let result =
                invoke(&self.binding, &directory, control).map_err(|error| mutation.failure(error));
            let index = match result {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
            mutation.verified_with(json!({"font_hash":index,"verification":"official_sdk_content_index","client_version":self.client_version,"module_sha256":self.module_sha256}))
                .map_err(|_|mutation.pending_error("font upload was accepted; finish saving its original content index"))?;
            // The only files here were created above for this operation. Keep
            // inputs on uncertain failure; completed snapshots can be removed.
            let cleanup_pending = !cleanup(home, mutation.id().unwrap());
            Ok(FontUpload {
                index,
                cleanup_pending,
            })
        })();
        storage::finish_file(binding, result, control)
    }
}
fn font_identity(fonts: &[LockedFont]) -> Value {
    let files = fonts
        .iter()
        .map(|font| (font.name.clone(), json!({"sha256":font.sha256})))
        .collect::<std::collections::BTreeMap<_, _>>();
    json!({"files":files,"api":"https://api.next.newbeebox.com/uploadserver"})
}
fn invoke(binding: &Path, directory: &Path, control: &OperationControl) -> Result<String> {
    let mut command = Command::new("node.exe");
    command
        .args(["-e", include_str!("font_sdk.js")])
        .current_dir(binding.parent().unwrap())
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .creation_flags(0x08000000);
    let input = serde_json::to_vec(&json!({"binding":binding,"path":directory}))?;
    control.check().map_err(not_sent)?;
    let output=process::run(command,Some(input),65536,65536,control).map_err(|mut error|{
        let never_created=error.details.get("startup").is_some_and(|value|value["cleanup"]["identity"].is_null());
        let process=std::mem::take(&mut error.details);error.details=json!({"submission":if never_created{"not_sent"}else{"uncertain"},"replay_allowed":false,"process":process});error
    })?;
    let value = std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|text| {
            text.lines()
                .filter_map(|line| line.strip_prefix("LYCHEE_FONT_RESULT:"))
                .last()
        })
        .and_then(|line| crate::input::value(line.as_bytes()).ok());
    let Some(value) = value.filter(|value| value["schema"] == "lycheedev.publish.font-sdk.v1")
    else {
        return Err(ToolkitError::new(
            "publish.font_upload_pending",
            "official font uploader did not return a complete terminal result",
            6,
        )
        .with_details(json!({"submission":"uncertain","replay_allowed":false})));
    };
    if value["ok"] == true && output.status == 0 {
        if let Some(index) = value["index"].as_str().filter(|value| {
            !value.is_empty()
                && value.len() <= 256
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        }) {
            return Ok(index.to_owned());
        }
    }
    if value["started"] == false {
        return Err(
            dependency().with_details(json!({"submission":"not_sent","replay_allowed":false}))
        );
    }
    Err(ToolkitError::new(
        "publish.font_upload_pending",
        "official font upload outcome is uncertain; retain its original request",
        6,
    )
    .with_details(json!({"submission":"uncertain","replay_allowed":false})))
}
fn not_sent(mut error: ToolkitError) -> ToolkitError {
    let cause = std::mem::take(&mut error.details);
    error.details = json!({"submission":"not_sent","replay_allowed":false,"cause":cause});
    error
}
fn cleanup(home: &Path, id: &str) -> bool {
    let root = home.join("publish/newbee/sdk/work");
    let directory = root.join(id);
    if !directory.exists() {
        return true;
    }
    storage::guard_path(&root, &directory)
        .and_then(|_| fs::remove_dir_all(directory).map_err(Into::into))
        .is_ok()
}
fn extract(source: &mut File, control: &OperationControl) -> Result<Vec<u8>> {
    control.check()?;
    source.seek(SeekFrom::Start(0))?;
    let mut header = [0; 16];
    source.read_exact(&mut header).map_err(|_| dependency())?;
    let header_size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64;
    let json_size = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    if !(2..=16 * 1024 * 1024).contains(&json_size) {
        return Err(dependency());
    }
    let mut bytes = vec![0; json_size];
    source.read_exact(&mut bytes).map_err(|_| dependency())?;
    let value = crate::input::value(&bytes).map_err(|_| dependency())?;
    let mut base = &value["files"]["node_modules"]["files"]["nbb-core"]["files"];
    if base.get("dist").is_some() {
        base = &base["dist"]["files"];
    }
    let entry = &base["nbb-core.win32-x64-msvc.node"];
    let size = entry["size"]
        .as_u64()
        .filter(|size| *size > 0 && *size < 128 * 1024 * 1024)
        .ok_or_else(dependency)?;
    let offset = entry["offset"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| entry["offset"].as_u64())
        .ok_or_else(dependency)?;
    let start = 8u64
        .checked_add(header_size)
        .and_then(|value| value.checked_add(offset))
        .ok_or_else(dependency)?;
    if entry["unpacked"] == true
        || start < 16 + json_size as u64
        || start
            .checked_add(size)
            .is_none_or(|end| end > source.metadata().map(|value| value.len()).unwrap_or(0))
    {
        return Err(dependency());
    }
    control.check()?;
    source.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; size as usize];
    source.read_exact(&mut bytes).map_err(|_| dependency())?;
    control.check()?;
    Ok(bytes)
}
#[cfg(test)]
mod tests;
