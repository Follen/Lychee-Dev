//! Ordered local TOC/XML load graph. Every analyzed byte is archived before
//! references are expanded; repeated edges and cycles remain visible.
use crate::{
    facts::{self, Facts},
    git,
};
use lycheedev_core::{Result, ToolkitError, evidence, storage};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
pub struct Document {
    pub path: String,
    pub bytes: Vec<u8>,
    pub facts: Facts,
    pub blob: Value,
}
pub struct Closure {
    pub root: PathBuf,
    pub manifest: String,
    pub documents: Vec<Document>,
    pub steps: Vec<Value>,
    pub issues: Vec<Value>,
    pub valid: bool,
}
struct Walker<'a> {
    home: &'a Path,
    control: &'a lycheedev_core::control::OperationControl,
    result: Closure,
    seen: BTreeSet<String>,
    active: BTreeSet<String>,
    directories: BTreeMap<String, Vec<String>>,
    entries: usize,
    total: usize,
}
impl Closure {
    pub fn inspect(home: &Path, root: &Path, manifest: &str) -> Result<Self> {
        Self::inspect_with_control(home, root, manifest, &Default::default())
    }
    pub fn inspect_with_control(
        home: &Path,
        root: &Path,
        manifest: &str,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Self> {
        control.check()?;
        storage::guard_path(root, root)?;
        let root = fs::canonicalize(root)?;
        let mut w = Walker {
            home,
            control,
            result: Self {
                root,
                manifest: String::new(),
                documents: vec![],
                steps: vec![],
                issues: vec![],
                valid: true,
            },
            seen: BTreeSet::new(),
            active: BTreeSet::new(),
            directories: BTreeMap::new(),
            entries: 0,
            total: 0,
        };
        let name = match w.resolve("", manifest) {
            Ok(n) => n,
            Err((code, message)) => {
                w.issue("error", code, "", 0, manifest, &message);
                return Ok(w.result);
            }
        };
        if !name.to_ascii_lowercase().ends_with(".toc") {
            return Err(ToolkitError::new(
                "codebase.manifest_must_be_toc",
                "Manifest must be TOC",
                2,
            ));
        }
        w.result.manifest = name.clone();
        w.load(&name, "", 0, 0, "toc", true)?;
        Ok(w.result)
    }
    pub fn lua_files(&self) -> BTreeMap<String, Vec<u8>> {
        self.documents
            .iter()
            .filter(|d| d.path.to_ascii_lowercase().ends_with(".lua"))
            .map(|d| (d.path.clone(), d.bytes.clone()))
            .collect()
    }
    pub fn value(&self) -> Value {
        json!({"root":self.root,"manifest":self.manifest,"documents":self.documents.iter().map(|d|json!({"path":d.path,"blob":d.blob,"archived":true,"facts":d.facts})).collect::<Vec<_>>(),"steps":self.steps,"issues":self.issues,"loadValid":self.valid,"compatibilityComplete":false})
    }
}
impl Walker<'_> {
    fn issue(
        &mut self,
        severity: &str,
        code: &str,
        path: &str,
        line: usize,
        reference: &str,
        message: &str,
    ) {
        if severity == "error" {
            self.result.valid = false;
        }
        self.result.issues.push(json!({"severity":severity,"code":code,"path":path,"line":line,"reference":reference,"message":message}));
    }
    fn resolve(
        &mut self,
        from: &str,
        reference: &str,
    ) -> std::result::Result<String, (&'static str, String)> {
        let reference = reference.trim().replace('\\', "/");
        if reference.is_empty() {
            return Err(("load_file_missing", "Empty load reference".into()));
        }
        if reference.starts_with('/') || reference.contains([':', '\0', '\r', '\n']) {
            return Err(("load_path_escape", "Unsafe load reference".into()));
        }
        let mut parts: Vec<&str> = from
            .rsplit_once('/')
            .map(|(p, _)| p.split('/').collect())
            .unwrap_or_default();
        for part in reference.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err((
                            "load_path_escape",
                            "Load reference escapes addon root".into(),
                        ));
                    }
                }
                _ => parts.push(part),
            }
        }
        if parts.is_empty() {
            return Err(("load_file_missing", "Load reference is a directory".into()));
        }
        let mut current = String::new();
        for part in parts {
            if !self.directories.contains_key(&current) {
                let path = self.result.root.join(&current);
                storage::guard_path(&self.result.root, &path)
                    .map_err(|e| ("load_path_escape", e.message))?;
                let mut names = vec![];
                for entry in fs::read_dir(&path).map_err(|e| ("load_reference", e.to_string()))? {
                    let entry = entry.map_err(|e| ("load_reference", e.to_string()))?;
                    let name = entry
                        .file_name()
                        .into_string()
                        .map_err(|_| ("load_reference", "Non-UTF8 load directory".into()))?;
                    names.push(name);
                    self.entries += 1;
                    if names.len() > 50000 || self.entries > 100000 {
                        return Err(("load_reference", "Directory entry budget exceeded".into()));
                    }
                }
                self.directories.insert(current.clone(), names);
            }
            let matches: Vec<_> = self.directories[&current]
                .iter()
                .filter(|s| s.eq_ignore_ascii_case(part))
                .collect();
            if matches.len() != 1 {
                return Err((
                    if matches.is_empty() {
                        "load_file_missing"
                    } else {
                        "load_reference"
                    },
                    format!("Missing or ambiguous case-insensitive load: {reference}"),
                ));
            }
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(matches[0]);
        }
        if !git::source_path(&current) {
            return Err(("load_path_escape", "Invalid resolved load path".into()));
        }
        storage::guard_path(&self.result.root, &self.result.root.join(&current))
            .map_err(|e| ("load_path_escape", e.message))?;
        Ok(current)
    }
    fn load(
        &mut self,
        name: &str,
        from: &str,
        line: usize,
        depth: usize,
        kind: &str,
        manifest: bool,
    ) -> Result<()> {
        self.control.check()?;
        if depth > 64 || self.result.steps.len() >= 20000 || self.result.documents.len() >= 10000 {
            return Err(ToolkitError::new(
                "codebase.closure_budget",
                "Addon load closure budget exceeded",
                3,
            ));
        }
        let key = name.to_ascii_lowercase();
        let repeated = self.seen.contains(&key);
        let cycle = self.active.contains(&key);
        self.result.steps.push(json!({"from":from,"line":line,"path":name,"kind":kind,"repeated":repeated,"cycle":cycle}));
        if cycle {
            self.issue(
                "warning",
                "load_cycle",
                from,
                line,
                name,
                "Recursive XML load reference",
            );
            return Ok(());
        }
        if repeated {
            self.issue(
                "warning",
                "duplicate_load",
                from,
                line,
                name,
                "Document already analyzed; repeated edge retained",
            );
            return Ok(());
        }
        let ext = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !manifest && !["lua", "xml"].contains(&ext.as_str()) {
            self.issue(
                "error",
                "unsupported_load",
                from,
                line,
                name,
                "Only Lua/XML may be loaded",
            );
            return Ok(());
        }
        let path = self.result.root.join(name);
        if storage::guard_path(&self.result.root, &path).is_err()
            || !fs::metadata(&path).is_ok_and(|m| m.is_file())
        {
            self.issue(
                "error",
                "load_unreadable",
                from,
                line,
                name,
                "Target is not a regular file",
            );
            return Ok(());
        }
        let mut data = vec![];
        match fs::File::open(&path).and_then(|f| {
            f.take(git::MAX_SOURCE_BYTES as u64 + 1)
                .read_to_end(&mut data)
        }) {
            Ok(_) => {}
            Err(e) => {
                self.issue("error", "load_unreadable", from, line, name, &e.to_string());
                return Ok(());
            }
        }
        storage::guard_path(&self.result.root, &path)?;
        self.total += data.len();
        if data.len() > git::MAX_SOURCE_BYTES || self.total > 64 << 20 {
            return Err(ToolkitError::new(
                "codebase.closure_byte_budget",
                "Addon closure byte budget exceeded",
                3,
            ));
        }
        if data.contains(&0) {
            self.issue(
                "error",
                "source_invalid",
                name,
                0,
                "",
                "Source must not contain NUL",
            );
            return Ok(());
        }
        // Invalid UTF-8 in a comment or string is a display observation, never a
        // reason to drop the document or invalidate the addon load.
        if let Some(warning) = crate::encoding::warning(&data) {
            self.issue(
                "warning",
                "source_encoding",
                name,
                warning.line,
                "",
                &warning.message(),
            );
        }
        let facts = match ext.as_str() {
            "lua" => facts::lua(&data, false),
            "toc" => facts::toc(&data),
            _ => facts::xml(&data),
        };
        let blob = evidence::publish_blob(self.home, &data, git::MAX_SOURCE_BYTES)?;
        for note in &facts.diagnostics {
            self.issue("error", "syntax", name, note.line, "", &note.message);
        }
        let loads = facts.loads.clone();
        self.result.documents.push(Document {
            path: name.into(),
            bytes: data,
            facts,
            blob,
        });
        self.seen.insert(key.clone());
        self.active.insert(key.clone());
        for load in loads {
            match self.resolve(name, &load.path) {
                Ok(child) => self.load(&child, name, load.line, depth + 1, &load.kind, false)?,
                Err((code, message)) => {
                    self.issue("error", code, name, load.line, &load.path, &message)
                }
            }
        }
        self.active.remove(&key);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_duplicate_unicode_cycle_and_escape() {
        let home = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../.tmp/rust-source-closure-tests/{}",
            std::process::id()
        ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/source-native/addon-closure");
        let good = Closure::inspect(&home, &root, "main.TOC").unwrap();
        assert!(good.valid, "{:?}", good.issues);
        assert_eq!(
            good.documents
                .iter()
                .map(|d| d.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Main.toc",
                "Code/Legal.lua",
                "UI.xml",
                "Code/Unicode 名称.lua"
            ]
        );
        assert_eq!(
            good.steps.iter().filter(|s| s["repeated"] == true).count(),
            1
        );
        assert!(
            good.documents
                .iter()
                .all(|d| d.blob["sha256"] == storage::sha256(&d.bytes))
        );
        let bad = Closure::inspect(&home, &root, "Cycle.toc").unwrap();
        assert!(!bad.valid);
        assert!(bad.issues.iter().any(|i| i["code"] == "load_cycle"));
        assert!(bad.issues.iter().any(|i| i["code"] == "load_path_escape"));
        fs::remove_dir_all(&home).unwrap();
    }
    // N11: a non-UTF-8 comment stays a load warning, never an error, and the
    // archived bytes keep their identity. A non-UTF-8 token keeps the real
    // syntax error route and invalidates the load.
    #[test]
    fn non_utf8_lua_comment_keeps_load_valid_and_a_token_keeps_its_syntax_error() {
        let home = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../.tmp/rust-source-closure-encoding/{}",
            std::process::id()
        ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let root = home.join("addon");
        fs::create_dir_all(root.join("Code")).unwrap();
        fs::write(root.join("Main.toc"), "Code/Tutorial.lua\n").unwrap();
        let mut lua = b"function Tutorial() end\n-- Night Web".to_vec();
        lua.push(0x92);
        lua.extend_from_slice(b"s Hollow\n");
        fs::write(root.join("Code/Tutorial.lua"), &lua).unwrap();
        let good = Closure::inspect(&home, &root, "Main.toc").unwrap();
        assert!(good.valid, "{:?}", good.issues);
        let warning = good
            .issues
            .iter()
            .find(|issue| issue["code"] == "source_encoding")
            .expect("encoding warning");
        assert_eq!(warning["severity"], "warning");
        assert_eq!(warning["path"], "Code/Tutorial.lua");
        assert_eq!(warning["line"], 2);
        assert!(warning["message"].as_str().unwrap().contains("0x92"));
        assert_eq!(good.documents.len(), 2);
        let document = good
            .documents
            .iter()
            .find(|document| document.path == "Code/Tutorial.lua")
            .unwrap();
        assert_eq!(document.bytes, lua);
        assert_eq!(document.blob["sha256"], storage::sha256(&lua));
        fs::write(
            root.join("Code/Tutorial.lua"),
            b"function Br\x92oken() end\n",
        )
        .unwrap();
        let broken = Closure::inspect(&home, &root, "Main.toc").unwrap();
        assert!(!broken.valid);
        assert!(
            broken
                .issues
                .iter()
                .any(|issue| issue["severity"] == "error" && issue["code"] == "syntax")
        );
        fs::remove_dir_all(&home).unwrap();
    }
}
