//! Reusable authenticated whole-source inputs, independent of the requested query.
use crate::{environment, index, luac, native, native_research};
use lycheedev_core::{
    Invocation, Result, ToolkitError, cache, metadata::Metadata, selection::SourcePin, storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Instant;

const SCHEMA: &str = "lycheedev.source-research-material.v3";
const MAX_BYTES: usize = 768 << 20;

/// Opt-in diagnostics only: timings never participate in input or query identity.
pub(crate) fn measured<T>(phase: &str, run: impl FnOnce() -> Result<T>) -> Result<T> {
    let enabled = std::env::var_os("LYCHEEDEV_SOURCE_PHASE_PROFILE").is_some_and(|v| v == "1");
    let started = Instant::now();
    let result = run();
    if enabled {
        let mut event = json!({"schema":"lycheedev.source-phase.v1","phase":phase,"elapsedMs":started.elapsed().as_millis(),"status":if result.is_ok(){"ok"}else{"error"}});
        if let Err(error) = &result {
            event["errorCode"] = json!(error.code);
        }
        eprintln!("{event}");
    }
    result
}

#[derive(Serialize, Deserialize)]
struct StoredMaterial {
    schema: String,
    identity: Value,
    snapshot: native_research::SnapshotRequest,
    environment: Option<environment::Environment>,
    load_facts: Vec<index::LoadFact>,
}

pub(crate) struct Material {
    pub snapshot: native_research::SnapshotRequest,
    pub environment: Option<environment::Environment>,
    pub load_facts: Vec<index::LoadFact>,
}

fn integrity(message: &str) -> ToolkitError {
    ToolkitError::new("codebase.source_integrity", message, 4)
}

fn serialize(
    material: &StoredMaterial,
    inv: &Invocation,
    maximum: usize,
) -> Result<Option<Vec<u8>>> {
    struct Writer<'a> {
        inv: &'a Invocation,
        bytes: Vec<u8>,
        maximum: usize,
        full: bool,
    }
    impl std::io::Write for Writer<'_> {
        fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
            self.inv.control.check().map_err(std::io::Error::other)?;
            if raw.len() > self.maximum.saturating_sub(self.bytes.len()) {
                self.full = true;
                return Err(std::io::Error::other(
                    "research material cache byte budget exceeded",
                ));
            }
            for chunk in raw.chunks(65536) {
                self.inv.control.check().map_err(std::io::Error::other)?;
                self.bytes.extend_from_slice(chunk);
            }
            Ok(raw.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        inv,
        bytes: Vec::new(),
        maximum,
        full: false,
    };
    let result = serde_json::to_writer(&mut writer, material);
    inv.control.check()?;
    // The cache stores additional environment data beside the worker request.
    // Exceeding its own bound leaves the valid original research path available.
    if writer.full {
        return Ok(None);
    }
    result?;
    Ok(Some(writer.bytes))
}

fn identity(
    inv: &Invocation,
    source: &SourcePin,
    environment: Option<&SourcePin>,
    index_digest: &str,
    runtime: &luac::Runtime,
) -> Result<Value> {
    let executable = std::env::current_exe()?;
    let bytes = std::fs::metadata(&executable)?.len();
    let (_file, digest) = storage::locked_sha256(&executable, bytes, &inv.control)?;
    Ok(
        json!({"schema":SCHEMA,"source":source,"environment":environment,
        "index":{"sha256":index_digest,"fingerprint":index::fingerprint(source)},
        "implementation":{"sha256":digest,"bytes":bytes,"engine":native::ENGINE_COMMIT,"engineArchive":native::ENGINE_ARCHIVE_SHA256,"enginePatch":option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256"),"lock":storage::sha256(native::DEPENDENCY_LOCK)},
        "rules":{"native":native::RULES_VERSION,"research":native_research::RESEARCH_RULES,"entry":native::ENTRY_RULES,"sourceMap":native::SOURCE_MAP_PROFILE,"xmlSourceMap":crate::xml_inline::SOURCE_MAP_RULES,"xmlProgram":crate::xml_load::RULES,"environment":environment::GENERATOR,"declaration":environment::DECLARATION_RULES},
        "luac":runtime.manifest(),"limits":native::SourceLimits::research(),"wireBytes":MAX_BYTES}),
    )
}

/// Source spans are deliberately excluded from the public ApiRecord JSON. The
/// Environment owns their exact parallel representation, so restore both views.
fn restore_spans(material: &mut StoredMaterial) -> Result<()> {
    if let Some(environment) = &mut material.environment {
        if environment.facts.len() != environment.source_spans.len()
            || serde_json::to_value(&material.snapshot.api)?
                != serde_json::to_value(&environment.facts)?
            || serde_json::to_value(&material.snapshot.environment_manifest)?
                != serde_json::to_value(Some(&environment.manifest))?
        {
            return Err(integrity("Cached research API material differs"));
        }
        for ((fact, api), span) in environment
            .facts
            .iter_mut()
            .zip(&mut material.snapshot.api)
            .zip(&environment.source_spans)
        {
            fact.observed_span = span.clone();
            api.observed_span = span.clone();
        }
    } else if !material.snapshot.api.is_empty() || material.snapshot.environment_manifest.is_some()
    {
        return Err(integrity("Cached research environment is absent"));
    }
    Ok(())
}

fn admit(
    material: &mut StoredMaterial,
    expected: &Value,
    index: &index::Index,
    inv: &Invocation,
) -> Result<()> {
    inv.control.check()?;
    let luac = &material.snapshot.luac;
    if material.schema != SCHEMA
        || material.identity != *expected
        || material.snapshot.source != expected["source"]
        || Some(luac.version.as_str()) != expected["luac"]["version"].as_str()
        || Some(luac.platform.as_str()) != expected["luac"]["platform"].as_str()
        || Some(luac.executable_sha256.as_str()) != expected["luac"]["sha256"].as_str()
        || Some(luac.executable_bytes) != expected["luac"]["bytes"].as_u64()
    {
        return Err(integrity("Cached research material identity differs"));
    }
    let env_limits = environment::Limits::default();
    let env_docs = &material.snapshot.environment_documents;
    if env_docs.len() > env_limits.documents || env_docs.values().any(|text| text.len() > env_limits.file_bytes)
        || env_docs.values().try_fold(0usize, |sum,text| sum.checked_add(text.len())).is_none_or(|bytes| bytes > env_limits.total_bytes)
        || material.snapshot.environment_manifest.is_none() && !env_docs.is_empty() {
        return Err(integrity("Cached independent environment originals exceed admission"));
    }
    for path in env_docs.keys() {
        inv.control.check()?;
        let path_lower=path.to_ascii_lowercase();
        if !crate::git::source_path(path) || !path_lower.contains("/blizzard_apidocumentationgenerated/") || !path_lower.ends_with(".lua") {
            return Err(integrity("Cached independent environment document path differs"));
        }
    }
    let limits = native::SourceLimits::research();
    let files = &material.snapshot.files;
    if files.len()+material.snapshot.load_documents.len() > limits.files
        || files.values().chain(material.snapshot.load_documents.values()).map(String::len).sum::<usize>() > limits.input_bytes
    {
        return Err(integrity(
            "Cached research material exceeds source admission",
        ));
    }
    for path in files.keys().chain(material.snapshot.load_documents.keys()) {
        inv.control.check()?;
        if !crate::git::source_path(path) || ![".lua",".xml",".toc"].iter().any(|extension|path.to_ascii_lowercase().ends_with(extension)) {
            return Err(integrity("Cached research source path differs"));
        }
    }
    // The index has already been authenticated. Recheck its available Lua, XML and TOC bytes;
    // skipped source files and their explicit gaps retain the cold-path meaning.
    for document in &index.documents {
        inv.control.check()?;
        if [".lua",".xml",".toc"].iter().any(|extension|document.path.to_ascii_lowercase().ends_with(extension)) {
            let raw = files
                .get(&document.path).or_else(||material.snapshot.load_documents.get(&document.path))
                .ok_or_else(|| integrity("Cached research source is missing"))?;
            if raw.len() != document.bytes || storage::sha256(raw.as_bytes()) != document.sha256 {
                return Err(integrity("Cached research source differs from fixed index"));
            }
        }
    }
    restore_spans(material)?;
    inv.control.check()
}

fn read(inv: &Invocation, key: &str, identity: &Value) -> Result<Option<StoredMaterial>> {
    let metadata = Metadata::open(&inv.home, false)?;
    let manifest = match metadata.read::<Value>(key) {
        Ok(value) => value,
        Err(error) if error.code == "vault.record_missing" => return Ok(None),
        Err(error) => return Err(error),
    };
    if manifest["schema"] != SCHEMA || manifest["identity"] != *identity {
        return Err(integrity("Cached research manifest identity differs"));
    }
    let digest = manifest["blob"]["sha256"]
        .as_str()
        .filter(|value| lycheedev_core::selection::valid_digest(value, 64))
        .ok_or_else(|| integrity("Cached research digest is invalid"))?;
    let path = inv
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    storage::guard_path(&inv.home, &path)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = cache::read_object(&inv.home, &manifest["blob"], MAX_BYTES, &inv.control)?;
    let material = serde_json::from_slice(&raw)?;
    inv.control.check()?;
    Ok(Some(material))
}

pub(crate) fn load_or_build(
    inv: &Invocation,
    source: &SourcePin,
    environment_pin: Option<&SourcePin>,
    index: &index::Index,
    index_digest: &str,
    runtime: &luac::Runtime,
) -> Result<Material> {
    let identity = measured("source.material.identity", || {
        identity(inv, source, environment_pin, index_digest, runtime)
    })?;
    let key = format!(
        "source/research-material/{}",
        storage::sha256(&serde_json::to_vec(&identity)?)
    );
    let cached = measured("source.material.read", || read(inv, &key, &identity))?;
    if let Some(mut material) = cached {
        measured("source.material.admit", || {
            admit(&mut material, &identity, index, inv)
        })?;
        return Ok(Material {
            snapshot: material.snapshot,
            environment: material.environment,
            load_facts: material.load_facts,
        });
    }
    let environment = measured("source.environment", || {
        environment_pin
            .map(|pin| environment::from_pin(&inv.home, pin, &inv.control))
            .transpose()
    })?;
    let (snapshot, load_facts) = measured("source.snapshot", || {
        native_research::snapshot_material(inv, source, index, environment.as_ref(), runtime)
    })?;
    let material = StoredMaterial {
        schema: SCHEMA.into(),
        identity: identity.clone(),
        snapshot,
        environment,
        load_facts,
    };
    let stored = measured("source.material.write", || -> Result<()> {
        let Some(raw) = serialize(&material, inv, MAX_BYTES)? else {
            return Ok(());
        };
        let blob = cache::publish(&inv.home, &raw, MAX_BYTES, None)?;
        let mut metadata = Metadata::open(&inv.home, true)?;
        let generation = metadata.generation(&key)?;
        metadata.put(
            &key,
            &json!({"schema":SCHEMA,"identity":identity,"blob":blob}),
            generation,
        )?;
        inv.control.check()
    });
    // A full cache must not turn valid source research into a failed operation.
    // Integrity failures still surface, and cancellation/deadline always wins.
    if let Err(error) = stored {
        inv.control.check()?;
        if error.code != "vault.cache_budget_exceeded" {
            return Err(error);
        }
    }
    Ok(Material {
        snapshot: material.snapshot,
        environment: material.environment,
        load_facts: material.load_facts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fixture() -> (StoredMaterial, index::Index) {
        let text = "function Entry() return FixtureAPI() end\n";
        let span = json!({"path":"FrameDocumentation.lua","firstLine":3,"lastLine":7,"startByte":20,"endByte":95,"contentHash":"1".repeat(64),"coordinate":"utf8-byte"});
        let api: environment::ApiRecord = serde_json::from_value(json!({"kind":"function","name":"GetName","owner":{"kind":"script-object","name":"Frame","source":span},"declarationRole":"method","displayGroup":"Frame","namespace":"","path":"FrameDocumentation.lua","line":3,"endLine":7})).unwrap();
        let manifest = environment::Manifest {
            schema: "lycheedev.source-environment.v1".into(),
            identity: environment::Identity {
                repository: "wow-ui-source".into(),
                commit: "1".repeat(40),
                client: "wow".into(),
                dependency_commits: BTreeMap::new(),
                generator_version: environment::GENERATOR.into(),
            },
            input_sha256: "2".repeat(64),
            definitions_sha256: "3".repeat(64),
            facts_sha256: "4".repeat(64),
            documents: 1,
        };
        let mut environment = environment::Environment {
            manifest: manifest.clone(),
            definitions: "---@class Frame\n".into(),
            facts: vec![api],
            mappings: vec![],
            coverage: environment::Coverage {
                gaps: vec![environment::Gap {
                    path: "FrameDocumentation.lua".into(),
                    line: 10,
                    code: "dynamic_literal".into(),
                    detail: "fixture".into(),
                }],
                ..Default::default()
            },
            source_spans: vec![span.clone()],
        };
        environment.facts[0].observed_span = span;
        let source =
            json!({"repository":"wow-ui-source","product":"wow","exactCommit":"1".repeat(40)});
        let luac = native::LuacIdentity {
            version: "5.1.5".into(),
            platform: "windows-amd64".into(),
            executable_sha256: "5".repeat(64),
            executable_bytes: 123,
        };
        let identity = json!({"source":source,"luac":{"version":luac.version,"platform":luac.platform,"sha256":luac.executable_sha256,"bytes":luac.executable_bytes}});
        let snapshot = native_research::SnapshotRequest {
            environment_documents: BTreeMap::new(),
                load_documents:BTreeMap::from([("Main.toc".into(),"Entry.lua\nOther.xml\n".into()),("Other.xml".into(),"<Ui><Script>local x = 1 &lt; 2</Script></Ui>".into())]),
            files: BTreeMap::from([("Entry.lua".into(), text.into())]),
            source,
            luac,
            api: environment.facts.clone(),
            environment_manifest: Some(manifest),
            load_plans: vec![native_research::LoadEvidence {
                manifest: "Main.toc".into(),
                ordered: vec!["Entry.lua".into(), "Entry.lua".into()],
                issues: vec![json!({"code":"load_cycle","path":"Loop.xml"})],
            }],
            input_gaps: vec![json!({"code":"source_file_unavailable","path":"Oversized.lua"})],
        };
        let mut index = index::Index {
            documents: vec![index::Document {
                path: "Entry.lua".into(),
                text: vec![],
                sha256: storage::sha256(text.as_bytes()),
                bytes: text.len(),
                facts: Default::default(),
            }],
            assets: vec![],
            symbols: vec![],
            summary: Value::Null,
            home: Default::default(),
        };
        for(path,text)in &snapshot.load_documents{index.documents.push(index::Document{path:path.clone(),text:vec![],sha256:storage::sha256(text.as_bytes()),bytes:text.len(),facts:Default::default()});}
        (
            StoredMaterial {
                schema: SCHEMA.into(),
                identity,
                snapshot,
                environment: Some(environment),
                load_facts: vec![
                    index::LoadFact {
                        path: "Main.toc".into(),
                        line: 2,
                        kind: "file".into(),
                        target: "Entry.lua".into(),
                        source_blob: json!({"sha256":"6".repeat(64),"bytes":42}),
                    },
                    index::LoadFact {
                        path: "Other.xml".into(),
                        line: 4,
                        kind: "script".into(),
                        target: "Other.lua".into(),
                        source_blob: json!({"sha256":"7".repeat(64),"bytes":84}),
                    },
                ],
            },
            index,
        )
    }

    fn invocation() -> Invocation {
        let home = std::env::temp_dir().join(format!(
            "lycheedev-research-material-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        Invocation {
            command: "source refs".into(),
            positional: None,
            flags: Default::default(),
            cwd: home.clone(),
            home,
            control: Default::default(),
        }
    }

    #[test]
    fn research_material_restores_full_inputs_and_admits_actual_source() {
        let (material, index) = fixture();
        let inv = invocation();
        let raw = serde_json::to_vec(&material).unwrap();
        assert!(serialize(&material, &inv, raw.len() - 1).unwrap().is_none());
        assert_eq!(serialize(&material, &inv, raw.len()).unwrap().unwrap(), raw);
        let mut restored: StoredMaterial = serde_json::from_slice(&raw).unwrap();
        assert!(restored.snapshot.api[0].observed_span.is_null());
        admit(&mut restored, &material.identity, &index, &inv).unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&material).unwrap()
        );
        assert_eq!(
            restored.snapshot.api[0].observed_span,
            material.snapshot.api[0].observed_span
        );
        assert_eq!(
            restored.environment.as_ref().unwrap().facts[0].observed_span,
            material.environment.as_ref().unwrap().facts[0].observed_span
        );
        assert!(!restored.environment.as_ref().unwrap().coverage.complete);
        assert_eq!(restored.load_facts.len(), 2);
        assert_eq!(restored.load_facts[1].target, "Other.lua");
        for path in ["Main.toc","Other.xml"]{
            let mut changed:StoredMaterial=serde_json::from_slice(&raw).unwrap();changed.snapshot.load_documents.get_mut(path).unwrap().push(' ');
            assert_eq!(admit(&mut changed,&material.identity,&index,&inv).unwrap_err().code,"codebase.source_integrity");
        }
        restored
            .snapshot
            .files
            .insert("Entry.lua".into(), "function Entry() end\n".into());
        assert_eq!(
            admit(&mut restored, &material.identity, &index, &inv)
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        std::fs::remove_dir_all(inv.home).unwrap();
    }

    #[test]
    fn research_material_cas_reads_full_bytes_and_allows_pruned_miss() {
        let (material, _) = fixture();
        let inv = invocation();
        let key = "source/research-material/test";
        let raw = serde_json::to_vec(&material).unwrap();
        let blob = cache::publish(&inv.home, &raw, MAX_BYTES, None).unwrap();
        let mut metadata = Metadata::open(&inv.home, true).unwrap();
        metadata
            .put(
                key,
                &json!({"schema":SCHEMA,"identity":material.identity,"blob":blob}),
                None,
            )
            .unwrap();
        drop(metadata);
        assert!(read(&inv, key, &material.identity).unwrap().is_some());
        let digest = blob["sha256"].as_str().unwrap();
        let path = inv
            .home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]);
        let mut changed = raw;
        let last = changed.len() - 1;
        changed[last] = b'!';
        std::fs::write(&path, changed).unwrap();
        assert!(
            matches!(read(&inv,key,&material.identity),Err(error) if error.code=="vault.cache_integrity")
        );
        std::fs::remove_file(path).unwrap();
        assert!(read(&inv, key, &material.identity).unwrap().is_none());
        std::fs::remove_dir_all(inv.home).unwrap();
    }
}
