//! Complete default project commands. The original shared directory remains
//! held through validation, project lease, same-directory stage and checked close.
use super::*;
use lycheedev_windows::filesystem::destination::{StagedFile, StagedPublication};

#[derive(Serialize)]
struct Project {
    schema: &'static str,
    product: String,
}
#[derive(Serialize)]
pub(super) struct ProjectLock {
    schema: &'static str,
    pub(super) selection: PinnedSet,
}
#[derive(Serialize)]
pub(super) struct Status {
    directory: PathBuf,
    project: Project,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lock: Option<ProjectLock>,
}
fn project(product: String) -> Project {
    Project {
        schema: "lycheedev.project.v1",
        product,
    }
}
fn locked(pin: PinnedSet) -> ProjectLock {
    ProjectLock {
        schema: "lycheedev.project-lock.v1",
        selection: pin,
    }
}
fn directory<C: FnMut() -> Result<()>>(path: &Path, scope: &mut Scope<'_, C>) -> Result<PathBuf> {
    scope.check()?;
    let path = absolute(path)?;
    if !fs::metadata(&path)?.is_dir() {
        return Err(format());
    }
    scope.check()?;
    Ok(path)
}
fn status<C: FnMut() -> Result<()>>(
    directory: &storage::Directory,
    path: &Path,
    scope: &mut Scope<'_, C>,
) -> Result<Status> {
    let product = read_file(directory, path, "lycheedev.json", scope, wire::project)?
        .ok_or_else(|| error("selection.project_missing"))?;
    let pin = read_file(directory, path, "lycheedev.lock.json", scope, wire::lock)?;
    if pin
        .as_ref()
        .is_some_and(|pin| pin_product(&pin.spec) != Some(product.as_str()))
    {
        return Err(error("selection.project_conflict"));
    }
    scope.check()?;
    Ok(Status {
        directory: path.into(),
        project: project(product),
        state: if pin.is_some() { "locked" } else { "unlocked" },
        lock: pin.map(locked),
    })
}
fn finish_directory<T, C: FnMut() -> Result<()>>(
    directory: storage::Directory,
    result: Result<T>,
    scope: &mut Scope<'_, C>,
) -> Result<T> {
    scope.finish(
        result,
        directory
            .close()
            .err()
            .into_iter()
            .flatten()
            .map(ToolkitError::from),
    )
}
pub(super) fn inspect<C: FnMut() -> Result<()>>(
    path: &Path,
    scope: &mut Scope<'_, C>,
) -> Result<Status> {
    let path = directory(path, scope)?;
    let directory = storage::Directory::open_shared_controlled(&path, scope)?;
    let result = status(&directory, &path, scope);
    finish_directory(directory, result, scope)
}
pub(in crate::selection) fn inspect_value(
    path: &Path,
    control: &OperationControl,
) -> Result<Value> {
    let mut scope = Scope {
        control,
        supplemental: || Ok(()),
    };
    let result =
        inspect(path, &mut scope).and_then(|value| serde_json::to_value(value).map_err(Into::into));
    scope.finish(result, [])
}
fn exists<C: FnMut() -> Result<()>>(path: &Path, scope: &mut Scope<'_, C>) -> Result<bool> {
    scope.check()?;
    let result = match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    };
    scope.finish(result, [])
}
#[derive(Default)]
struct Publication {
    path: Option<PathBuf>,
    state: Option<StagedPublication>,
    stage_name: Option<String>,
}
impl Publication {
    fn finish<T>(self, result: Result<T>) -> Result<T> {
        result.map_err(|mut error| {
            if let Some(state) = self.state {
                let state = match state {
                    StagedPublication::NotPublished => "not_published",
                    StagedPublication::Published => "published",
                    StagedPublication::Unknown => "unknown",
                };
                let primary = error.clone();
                if state != "not_published" {
                    error.message = format!(
                        "{}\nproject file publication {}: {}",
                        error.message,
                        state,
                        self.path
                            .as_ref()
                            .expect("stage has original path")
                            .display()
                    );
                }
                error.details = json!({
                    "schema":"lycheedev.project-file-error.v1", "primary":primary,
                    "publication":{"state":state,"path":self.path,"stageName":self.stage_name}
                });
            }
            error
        })
    }
}
fn write<C: FnMut() -> Result<()>>(
    directory: &storage::Directory,
    path: &Path,
    name: &str,
    value: &impl Serialize,
    scope: &mut Scope<'_, C>,
    publication: &mut Publication,
) -> Result<()> {
    scope.check()?;
    // Match Go's immediate Lstat regular-file check; no target body is opened
    // or truncated. The following native operation changes only this entry.
    match fs::symlink_metadata(path.join(name)) {
        Ok(metadata) if !regular(&metadata) => return Err(format()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut raw = serde_json::to_string_pretty(value)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes();
    if raw.len() > 65535 {
        return Err(format());
    }
    raw.push(b'\n');
    scope.check()?;
    let mut stage = StagedFile::create(directory, scope)?;
    publication.path = Some(path.join(name));
    publication.stage_name = Some(stage.name().to_owned());
    publication.state = Some(stage.publication());
    let result = (|| {
        for chunk in raw.chunks(8192) {
            scope.check()?;
            stage.write_all(chunk)?;
            scope.check()?;
        }
        stage.seal()?;
        scope.check()?;
        let state = if name == "lycheedev.json" {
            stage.publish_name(OsStr::new(name), scope)
        } else {
            stage.replace_name(OsStr::new(name), scope)
        }?;
        if state == StagedPublication::NotPublished {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "project declaration appeared before publication",
            )
            .into());
        }
        scope.check()
    })();
    publication.state = Some(stage.publication());
    scope.finish(result, stage.close().into_iter().map(ToolkitError::from))
}
fn lease<C: FnMut() -> Result<()>>(path: &Path, scope: &mut Scope<'_, C>) -> Result<ExclusiveLock> {
    ExclusiveLock::resource_wait_checked(
        &path.join(".lycheedev-locks"),
        "project",
        Some(scope.control.deadline()),
        || scope.check(),
    )
}
fn initialize<C: FnMut() -> Result<()>>(
    path: &Path,
    product_name: &str,
    scope: &mut Scope<'_, C>,
    publication: &mut Publication,
) -> Result<Status> {
    // Fixed Go validates product before touching the project directory.
    if !valid_product(product_name) {
        return Err(format());
    }
    if matches!(product_name, "forever-asia-beta" | "forever-cn-beta") {
        return Err(ToolkitError::new(
            "selection.unsupported_data_product",
            "Retired Forever beta products cannot initialize a new project",
            2,
        ));
    }
    let path = directory(path, scope)?;
    let directory = storage::Directory::open_shared_controlled(&path, scope)?;
    let result = (|| {
        let lease = lease(&path, scope)?;
        let result = (|| {
            if exists(&path.join("lycheedev.json"), scope)? {
                let status = status(&directory, &path, scope)?;
                if status.project.product != product_name {
                    return Err(error("selection.project_conflict"));
                }
                return Ok(status);
            }
            if exists(&path.join("lycheedev.lock.json"), scope)? {
                return Err(error("selection.project_conflict"));
            }
            let project = project(product_name.into());
            write(
                &directory,
                &path,
                "lycheedev.json",
                &project,
                scope,
                publication,
            )?;
            Ok(Status {
                directory: path.clone(),
                project,
                state: "unlocked",
                lock: None,
            })
        })();
        scope.finish(result, lease.close().err())
    })();
    finish_directory(directory, result, scope)
}
fn lock<C: FnMut() -> Result<()>>(
    home: &Path,
    path: &Path,
    id: &str,
    scope: &mut Scope<'_, C>,
    publication: &mut Publication,
) -> Result<Status> {
    // LockProject authenticates the snapshot before validating/opening the
    // explicit project directory or attempting its independent lease.
    let raw = Metadata::read_document_raw_controlled_with_check(
        home,
        &format!("pin/{id}"),
        scope.control,
        || (scope.supplemental)(),
    )?;
    let pin = super::super::file::stored_pin(&raw.bytes, id, scope.control)?;
    scope.check()?;
    let path = directory(path, scope)?;
    let directory = storage::Directory::open_shared_controlled(&path, scope)?;
    let result = (|| {
        let lease = lease(&path, scope)?;
        let result = (|| {
            let product_name =
                read_file(&directory, &path, "lycheedev.json", scope, wire::project)?
                    .ok_or_else(|| error("selection.project_missing"))?;
            if pin_product(&pin.spec) != Some(product_name.as_str()) {
                return Err(error("selection.project_conflict"));
            }
            // A previous known-format lock may belong to another product. Go
            // validates its pin identity, then deliberately replaces it.
            let _previous = read_file(&directory, &path, "lycheedev.lock.json", scope, wire::lock)?;
            let lock = locked(pin);
            write(
                &directory,
                &path,
                "lycheedev.lock.json",
                &lock,
                scope,
                publication,
            )?;
            Ok(Status {
                directory: path.clone(),
                project: project(product_name),
                state: "locked",
                lock: Some(lock),
            })
        })();
        scope.finish(result, lease.close().err())
    })();
    finish_directory(directory, result, scope)
}
pub(in crate::selection) fn execute(
    ctx: &RequestContext,
    request: &ProjectRequest,
) -> Result<Outcome> {
    let mut scope = Scope {
        control: &ctx.control,
        supplemental: || Ok(()),
    };
    let mut publication = Publication::default();
    let result = (|| {
        let value = match request {
            ProjectRequest::Init { path, product } => {
                initialize(&ctx.cwd.join(path), product, &mut scope, &mut publication)?
            }
            ProjectRequest::Status { path } | ProjectRequest::Lock { path, .. } => {
                // Find the nearest declaration before authenticating the pin;
                // an explicitly selected directory is not stat'ed yet.
                let path = match path {
                    Some(path) => ctx.cwd.join(path),
                    None => find(&ctx.cwd, &mut scope)?,
                };
                match request {
                    ProjectRequest::Lock { snapshot, .. } => {
                        lock(&ctx.home, &path, snapshot, &mut scope, &mut publication)?
                    }
                    _ => inspect(&path, &mut scope)?,
                }
            }
        };
        let mut out = Outcome::new(serde_json::to_value(&value)?);
        if matches!(request, ProjectRequest::Lock { .. }) {
            out.selection =
                json!({"snapshot":value.lock.as_ref().expect("successful lock").selection.id});
        }
        Ok(out)
    })();
    publication.finish(scope.finish(result, []))
}

pub(in crate::selection) fn optional(
    request: PinSelectionRequest<'_>,
    control: &OperationControl,
) -> Result<Option<(PathBuf, PinnedSet)>> {
    let mut scope = Scope {
        control,
        supplemental: || Ok(()),
    };
    let result = (|| {
        scope.check()?;
        if request.snapshot.is_some_and(|id| !id.is_empty()) {
            return Ok(None);
        }
        let path = match request.project.filter(|p| !p.is_empty()) {
            Some(path) => absolute(&request.cwd.join(path))?,
            None => match find(request.cwd, &mut scope) {
                Ok(path) => path,
                Err(e) if pure(&e, "selection.project_missing") => return Ok(None),
                Err(e) => return Err(e),
            },
        };
        let project = path.to_string_lossy();
        let pin = resolve_pin_request_controlled_with_check(
            PinSelectionRequest {
                project: Some(&project),
                ..request
            },
            control,
            || Ok(()),
        )?;
        Ok(Some((path, pin)))
    })();
    scope.finish(result, [])
}

#[cfg(test)]
mod tests;
