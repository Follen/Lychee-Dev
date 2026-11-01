use super::*;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Active {
    session: String,
}
pub(super) struct Store {
    pub(super) home: PathBuf,
    pub(super) root: PathBuf,
    pub(super) scope: BrokerScope,
}
impl Store {
    pub(super) fn open(home: &Path, control: &OperationControl, create: bool) -> Result<Self> {
        control.check()?;
        if create {
            fs::create_dir_all(home)?;
        }
        let scope = BrokerScope::for_dd_task(home).map_err(platform)?;
        let root = home.join("publish/dd/tasks").join(scope.logon_hash());
        storage::guard_path_with_check(home, &root, || control.check())?;
        if create {
            fs::create_dir_all(&root)?;
        }
        Ok(Self {
            home: home.to_owned(),
            root,
            scope,
        })
    }
    fn state_lock(&self, control: &OperationControl) -> Result<storage::ExclusiveLock> {
        loop {
            match storage::ExclusiveLock::resource_controlled(&self.root, "state", control) {
                Err(error) if error.code == "workspace.busy" && error.details.is_null() => {
                    std::thread::sleep(Duration::from_millis(10).min(control.remaining()?));
                }
                result => return result,
            }
        }
    }
    fn read<T: serde::de::DeserializeOwned>(
        &self,
        path: &Path,
        control: &OperationControl,
    ) -> Result<Option<T>> {
        storage::guard_path_with_check(&self.home, path, || control.check())?;
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let state = self.state_lock(control)?;
        let result = (|| {
            let (mut file, _) = storage::locked_sha256(path, 65536, control)?;
            let result = (|| {
                use std::io::{Seek, SeekFrom};
                file.seek(SeekFrom::Start(0))?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                let value = crate::input::value(&bytes).map_err(|_| invalid())?;
                serde_json::from_value(value)
                    .map(Some)
                    .map_err(|_| invalid())
            })();
            storage::finish_file(file, result, control)
        })();
        finish_owned(result, state.close())
    }
    pub(super) fn active(&self, control: &OperationControl) -> Result<Option<Record>> {
        let active: Option<Active> = self.read(&self.root.join("active.json"), control)?;
        active.map(|a| self.record(&a.session, control)).transpose()
    }
    pub(super) fn record(&self, id: &str, control: &OperationControl) -> Result<Record> {
        if !session_id(id) {
            return Err(invalid());
        }
        let record: Record = self
            .read(&self.root.join(format!("{id}.json")), control)?
            .ok_or_else(|| fault("DD task session does not exist"))?;
        if record.schema != SCHEMA
            || record.id != id
            || !hex(&record.digest, 64)
            || !record.executable.is_absolute()
        {
            return Err(invalid());
        }
        Ok(record)
    }
    pub(super) fn save(&self, record: &Record, control: &OperationControl) -> Result<()> {
        if !session_id(&record.id) {
            return Err(invalid());
        }
        let path = self.root.join(format!("{}.json", record.id));
        storage::guard_path_with_check(&self.home, &path, || control.check())?;
        let state = self.state_lock(control)?;
        let result =
            (|| storage::atomic_write_controlled(&path, &serde_json::to_vec(record)?, control))();
        finish_owned(result, state.close())
    }
    pub(super) fn select(&self, id: &str, control: &OperationControl) -> Result<()> {
        if !session_id(id) {
            return Err(invalid());
        }
        let state = self.state_lock(control)?;
        let result = (|| {
            storage::atomic_write_controlled(
                &self.root.join("active.json"),
                &serde_json::to_vec(&Active { session: id.into() })?,
                control,
            )
        })();
        finish_owned(result, state.close())
    }
    pub(super) fn released(&self, record: &Record) -> Result<bool> {
        let owner = record
            .owner
            .as_ref()
            .map(ended)
            .transpose()?
            .unwrap_or(true);
        let worker = record
            .worker
            .as_ref()
            .map(ended)
            .transpose()?
            .unwrap_or(true);
        Ok(owner && worker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_publication_waits_for_the_original_reader_to_close_before_replacing_its_snapshot() {
        let dir = crate::test_support::TestDir::new();
        let control = OperationControl::default();
        let store = Store::open(&dir.0, &control, true).unwrap();
        let id = format!("DDT-{}", "1".repeat(32));
        let mut record = Record {
            schema: SCHEMA.into(),
            id: id.clone(),
            digest: "2".repeat(64),
            executable: std::env::current_exe().unwrap(),
            parent: current().unwrap(),
            start_deadline_ms: wire_deadline(&control).unwrap(),
            owner: None,
            worker: None,
            phase: Phase::Starting,
            identity: Value::Null,
            last: None,
            error: None,
        };
        store.save(&record, &control).unwrap();
        let path = store.root.join(format!("{id}.json"));
        let state = store.state_lock(&control).unwrap();
        let (mut original, _) = storage::locked_sha256(&path, 65536, &control).unwrap();
        record.phase = Phase::Ready;
        let home = dir.0.clone();
        let caller = control.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            started.send(()).unwrap();
            let result = Store::open(&home, &caller, true)?.save(&record, &caller);
            finished.send(result.clone()).unwrap();
            result
        });
        let reached = ready.recv_timeout(Duration::from_secs(5));
        let pending = done.recv_timeout(Duration::from_millis(80));
        use std::io::{Seek, SeekFrom};
        original.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        original.read_to_end(&mut bytes).unwrap();
        let earlier: Record = serde_json::from_slice(&bytes).unwrap();
        storage::finish_file(original, Ok(()), &control).unwrap();
        state.close().unwrap();
        let result = writer.join().unwrap();
        reached.unwrap();
        assert!(matches!(
            pending,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        result.unwrap();
        assert!(matches!(earlier.phase, Phase::Starting));
        assert!(matches!(
            store.record(&id, &control).unwrap().phase,
            Phase::Ready
        ));
    }
}
