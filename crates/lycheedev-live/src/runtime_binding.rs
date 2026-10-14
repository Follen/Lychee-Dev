//! Runtime-only Lua path reader. Runtime code location and Lua ABI admission
//! remain separate capabilities. No executable hash is invented or imported.
#![forbid(unsafe_code)]
use crate::{
    locator::Layout,
    mailbox::{Memory, Reader, Record, Selector},
    runtime_root::{RecipeId, RootBinding, RootEvidence},
};
use lycheedev_core::client::{self, ClientInstallation};
use lycheedev_windows::process::{
    RuntimeBudget, RuntimeControl, RuntimeCost, RuntimeLimits, RuntimeModule,
    RuntimeModuleIdentity, RuntimeReadKind, RuntimeUsage,
};
use serde::Serialize;
use std::cell::RefCell;

type Result<T> = std::result::Result<T, String>;

struct ParentControl(lycheedev_core::control::OperationControl);
fn control_error(e: lycheedev_core::ToolkitError) -> String {
    match e.code.as_str() {
        "command.cancelled" => "memory.runtime_cancelled".into(),
        "command.deadline" => "memory.runtime_deadline".into(),
        _ => format!("{}: {}", e.code, e.message),
    }
}
impl RuntimeControl for ParentControl {
    fn check(&self) -> Result<()> {
        self.0.check().map_err(control_error)
    }
}
/// Keep the complete caller control alive; taking only cancel_handle() would
/// lose future parent cancellation because Core propagates it on each check.
pub fn runtime_budget(
    control: &lycheedev_core::control::OperationControl,
    limits: RuntimeLimits,
) -> Result<RuntimeBudget> {
    control.check().map_err(control_error)?;
    RuntimeBudget::with_control_factory(
        limits,
        control.deadline(),
        control.clone_allocation_bytes(),
        || ParentControl(control.clone()),
    )
}

struct CatalogProfile {
    product: &'static str,
    catalog_product: &'static str,
    channel: &'static str,
    full_build: &'static str,
    recipe: RecipeId,
    profile: &'static str,
    evidence_sha256: &'static str,
}

// These exact historical clients have complete named-path read confirmations.
// The profile digests bind the recorded ABI, not an executable or numeric RVA.
// Read/acceptance evidence lives beside each profile under docs/toolkit/research:
// Retail current-final-candidate/pid60944/summary.json; Classic
// current-candidate/core/final-summary.json; Titan current-candidate/summary.json;
// Forever current-live-acceptance-summary.json. Their candidate profile labels
// remain historical; none establishes new protocol or current-build acceptance.
static PROFILES: &[CatalogProfile] = &[
    CatalogProfile {
        product: "retail",
        catalog_product: "wow",
        channel: "",
        full_build: "12.1.0.69933",
        recipe: RecipeId::Retail,
        profile: "retail-lua-mailbox-root-v1-69933",
        evidence_sha256: "1afe07213bb3979384c5939062df458ea9f26ef5d1b3e98a8cff5fa382f56406",
    },
    CatalogProfile {
        product: "classic",
        catalog_product: "wow_classic",
        channel: "",
        full_build: "5.5.4.70032",
        recipe: RecipeId::Retail,
        profile: "classic-lua-mailbox-root-v1-70032",
        evidence_sha256: "3c7cf9920b59e62c8ab62ab296c47df50a73c8666010d38d2712650e0269d6db",
    },
    CatalogProfile {
        product: "titan",
        catalog_product: "wow_classic_titan",
        channel: "",
        full_build: "3.80.2.69874",
        recipe: RecipeId::Retail,
        profile: "titan-lua-mailbox-root-v1-69874",
        evidence_sha256: "98e5bc40a701845af2552e13545d0800d7fd2bc86ae07b8d80e03b5e00a9ef7d",
    },
    CatalogProfile {
        product: "forever",
        catalog_product: "wow_classic_beta",
        channel: "asia-beta",
        full_build: "1.60.1.70124",
        recipe: RecipeId::Forever,
        profile: "forever-lua-mailbox-root-runscript-v1-70124",
        evidence_sha256: "2c85b30a93ab8b4f0da884f928a02e4bf8cc051c7b67071d8c41ce15432a4db9",
    },
];

/// Opaque, compile-catalog-issued ABI permission. It cannot be deserialized,
/// default-constructed or issued by importing flags, layouts or EXE digests.
pub struct RuntimeLayoutAdmission {
    profile: &'static CatalogProfile,
    scope: Option<RuntimeScope>,
}
struct RuntimeScope {
    module: RuntimeModuleIdentity,
    client: ClientInstallation,
}
struct MetadataBudget<'a>(&'a mut RuntimeBudget);
impl client::MetadataControl for MetadataBudget<'_> {
    fn charge(&mut self, cost: client::MetadataCost) -> lycheedev_core::Result<()> {
        // std filesystem APIs may use multiple Win32 calls. Reserve sixteen
        // OS units per bounded filesystem operation, including a close reserved
        // before open. These are conservative reservations, not syscall counts.
        self.0
            .charge(RuntimeCost {
                os_call_units: cost.io_operations.checked_mul(16).ok_or_else(|| {
                    lycheedev_core::ToolkitError::invalid("metadata operation overflow")
                })?,
                read_calls: cost.read_calls,
                requested_bytes: cost.requested_bytes,
                allocation_bytes: cost.allocation_bytes,
                work_units: cost.work_units,
                ..RuntimeCost::default()
            })
            .map_err(|cause| lycheedev_core::ToolkitError::new("reader.runtime_metadata", cause, 3))
    }
    fn check(&mut self) -> lycheedev_core::Result<()> {
        self.0
            .check()
            .map_err(|cause| lycheedev_core::ToolkitError::new("reader.runtime_metadata", cause, 3))
    }
}
fn fresh_client(module: &RuntimeModule, budget: &mut RuntimeBudget) -> Result<ClientInstallation> {
    let directory = module.image_directory(budget)?;
    let client =
        client::inspect_with(&directory, &mut MetadataBudget(budget)).map_err(|e| e.message)?;
    module.verify(budget)?;
    Ok(client)
}
fn check_scope_identity<I: Eq>(issued: Option<&I>, current: &I) -> Result<()> {
    match issued {
        None => Err("reader.runtime_scope_missing".into()),
        Some(issued) if issued != current => Err("reader.runtime_scope_module_changed".into()),
        Some(_) => Ok(()),
    }
}
fn check_client_identity(
    current: &ClientInstallation,
    issued: &ClientInstallation,
    budget: &mut RuntimeBudget,
) -> Result<()> {
    budget.charge(RuntimeCost::work(131072))?;
    if current != issued {
        return Err("reader.runtime_metadata_changed".into());
    }
    Ok(())
}
impl RuntimeLayoutAdmission {
    pub fn select(
        module: &RuntimeModule,
        recipe: RecipeId,
        budget: &mut RuntimeBudget,
    ) -> Result<Self> {
        let client = fresh_client(module, budget)?;
        if PROFILES.len() > 1024 {
            return Err("reader.runtime_catalog_budget".into());
        }
        budget.charge(RuntimeCost::work(PROFILES.len() as u64 * 512 + 1024))?;
        let mut admission = Self::select_from(&client, recipe, PROFILES)?;
        budget.charge(RuntimeCost::allocation(std::mem::size_of::<
            RuntimeModuleIdentity,
        >()))?;
        admission.scope = Some(RuntimeScope {
            module: module.identity().clone(),
            client,
        });
        Ok(admission)
    }
    fn select_from(
        client: &ClientInstallation,
        recipe: RecipeId,
        profiles: &'static [CatalogProfile],
    ) -> Result<Self> {
        // The API family may be shared, but ABI admission remains tied to the
        // exact observed delivery slot. No regional catalog evidence is borrowed.
        let known = client::product(client::client_data_product(client))
            .ok_or("reader.runtime_client_identity")?;
        let expected_code = if known.family == "forever" {
            "wow_forever"
        } else {
            known.catalog_product
        };
        let expected_catalog = if known.id != known.family {
            known.catalog_product
        } else {
            ""
        };
        if known.family != client.product
            || client.product_code != expected_code
            || client.catalog_product != expected_catalog
            || known.channel != client.channel
            || client.full_build.len() > 64
        {
            return Err("reader.runtime_client_identity".into());
        }
        let observed_slot = if client.catalog_product.is_empty() {
            client.product_code.as_str()
        } else {
            client.catalog_product.as_str()
        };
        let mut components = client.full_build.split('.');
        for _ in 0..4 {
            let part = components.next().ok_or("reader.runtime_client_identity")?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err("reader.runtime_client_identity".into());
            }
        }
        if components.next().is_some() {
            return Err("reader.runtime_client_identity".into());
        }
        let mut matches = profiles.iter().filter(|p| {
            p.product == client.product
                && p.catalog_product == observed_slot
                && p.channel == client.channel
                && p.full_build == client.full_build
                && p.recipe == recipe
        });
        let profile = matches.next().ok_or("reader.runtime_layout_unsupported")?;
        if matches.next().is_some()
            || profile.profile.is_empty()
            || profile.profile.len() > 128
            || profile.evidence_sha256.len() != 64
            || !profile
                .evidence_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !profile.evidence_sha256.bytes().any(|b| b != b'0')
        {
            return Err("reader.runtime_catalog_invalid".into());
        }
        Ok(Self {
            profile,
            scope: None,
        })
    }
    pub fn profile_id(&self) -> &'static str {
        self.profile.profile
    }
    pub fn evidence_sha256(&self) -> &'static str {
        self.profile.evidence_sha256
    }
    fn layout(&self) -> Layout {
        // This reader's remaining tag/node ABI is fixed too. A future ABI must
        // gain a separate implementation and independently admitted profile.
        Layout {
            id: self.profile.profile.into(),
            applicable_hashes: Vec::new(),
            state_globals: 0x90,
            tag_offset: 16,
            string_length: 24,
            string_data: 32,
            table_nodes: 40,
            table_log2: 19,
            node_bytes: 56,
        }
    }
    pub(crate) fn admits_layout(&self, layout: &Layout) -> Result<()> {
        if layout.id != self.profile.profile
            || !layout.applicable_hashes.is_empty()
            || layout.state_globals != 0x90
            || layout.tag_offset != 16
            || layout.string_length != 24
            || layout.string_data != 32
            || layout.table_nodes != 40
            || layout.table_log2 != 19
            || layout.node_bytes != 56
        {
            return Err("reader.runtime_layout_mismatch".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeBindingFailure {
    pub cause: String,
    pub stage: &'static str,
    pub root: Box<RootEvidence>,
    /// Original ledger's last observed usage. A foreign replacement ledger
    /// never replaces the binding's retained counters.
    pub usage: RuntimeUsage,
}
impl std::fmt::Display for RuntimeBindingFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cause)
    }
}
impl std::error::Error for RuntimeBindingFailure {}

/// Plain observation data, deliberately excluding the opaque module's issuance
/// serial/request and all root, Lua state and heap addresses.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeWindowDescription {
    pub handle: String,
    pub process_id: u32,
    pub process_started_at: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModuleDescription {
    pub base: String,
    pub size: u64,
    pub architecture: u16,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoverage {
    pub recipe: Option<RecipeId>,
    pub pattern_hits: u64,
    pub header_bytes: u64,
    pub scanned_bytes: u64,
    pub module_read_calls: u64,
    pub module_read_bytes: u64,
    pub full_text_pass: bool,
    pub stable_guards: bool,
    pub atomic: bool,
}
impl From<&RootEvidence> for RuntimeCoverage {
    fn from(value: &RootEvidence) -> Self {
        Self {
            recipe: value.recipe,
            pattern_hits: value.pattern_hits,
            header_bytes: value.header_bytes,
            scanned_bytes: value.scanned_bytes,
            module_read_calls: value.module_read_calls,
            module_read_bytes: value.module_read_bytes,
            full_text_pass: value.full_text_pass,
            stable_guards: value.stable_guards,
            atomic: value.atomic,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBindingDescription {
    pub window: RuntimeWindowDescription,
    pub module: RuntimeModuleDescription,
    pub client: ClientInstallation,
    pub expected_release: String,
    pub layout_profile: &'static str,
    pub initial_coverage: RuntimeCoverage,
    pub latest_coverage: RuntimeCoverage,
}

fn description_cost(client: &ClientInstallation, release: &str) -> RuntimeCost {
    let client_bytes: usize = [
        &client.directory,
        &client.product,
        &client.product_code,
        &client.catalog_product,
        &client.display_name,
        &client.channel,
        &client.metadata_branch,
        &client.full_build,
        &client.toc,
        &client.identity_source,
    ]
    .iter()
    .map(|value| value.len())
    .sum();
    RuntimeCost {
        allocation_bytes: (std::mem::size_of::<RuntimeBindingDescription>()
            + client_bytes
            + release.len()
            + 3 * 32) as u64,
        work_units: (client_bytes + release.len() + 256) as u64,
        ..RuntimeCost::default()
    }
}

// Private seam exercises orchestration without allowing external Memory
// implementations to mint an OS-bound reader or an ABI admission.
trait Source {
    fn root(&self) -> u64;
    fn evidence(&self) -> &RootEvidence;
    fn verify_scope(
        &self,
        admission: &RuntimeLayoutAdmission,
        budget: &mut RuntimeBudget,
    ) -> Result<()>;
    fn revalidate(&mut self, budget: &mut RuntimeBudget) -> Result<()>;
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()>;
    fn read(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> Result<()>;
}
struct OsSource {
    module: RuntimeModule,
    root: RootBinding,
    latest: RootEvidence,
}
impl Source for OsSource {
    fn root(&self) -> u64 {
        self.root.root_address()
    }
    fn evidence(&self) -> &RootEvidence {
        &self.latest
    }
    fn verify_scope(
        &self,
        admission: &RuntimeLayoutAdmission,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        let scope = admission
            .scope
            .as_ref()
            .ok_or("reader.runtime_scope_missing")?;
        check_scope_identity(Some(&scope.module), self.module.identity())?;
        let current = fresh_client(&self.module, budget)?;
        check_client_identity(&current, &scope.client, budget)
    }
    fn revalidate(&mut self, budget: &mut RuntimeBudget) -> Result<()> {
        match self.root.revalidate(&self.module, budget) {
            Ok(evidence) => {
                self.latest = evidence;
                Ok(())
            }
            Err(failure) => {
                self.latest = *failure.evidence;
                Err(failure.cause)
            }
        }
    }
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()> {
        self.module.verify(budget)
    }
    fn read(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        if module {
            if address != self.root.root_address() || bytes.len() != 8 {
                return Err("reader.runtime_root_range".into());
            }
            self.module.read_exact(
                self.root.root_rva(),
                bytes,
                RuntimeReadKind::WritableData,
                budget,
            )
        } else {
            self.module.read_private_exact(address, bytes, budget)
        }
    }
}

struct Bound<S> {
    source: S,
    admission: RuntimeLayoutAdmission,
    request: u64,
    last_usage: RuntimeUsage,
    release: String,
    poisoned: Option<String>,
}
impl<S: Source> Bound<S> {
    fn describe(
        &mut self,
        budget: &mut RuntimeBudget,
        project: impl FnOnce(
            &S,
            &RuntimeLayoutAdmission,
            &str,
            &mut RuntimeBudget,
        ) -> Result<RuntimeBindingDescription>,
    ) -> std::result::Result<RuntimeBindingDescription, RuntimeBindingFailure> {
        if let Some(cause) = &self.poisoned {
            return Err(self.failure(cause.clone(), "poisoned", budget));
        }
        let result = (|| {
            if budget.request_id() != self.request {
                return Err("reader.runtime_budget_identity".into());
            }
            budget.check()?;
            self.source.verify_scope(&self.admission, budget)?;
            let description = project(&self.source, &self.admission, &self.release, budget)?;
            budget.check()?;
            Ok(description)
        })();
        if budget.request_id() == self.request {
            self.last_usage = budget.usage();
        }
        result.map_err(|cause: String| {
            self.poisoned = Some(cause.clone());
            self.failure(cause, "description", budget)
        })
    }
    fn failure(
        &self,
        cause: String,
        stage: &'static str,
        budget: &RuntimeBudget,
    ) -> RuntimeBindingFailure {
        RuntimeBindingFailure {
            cause,
            stage,
            root: Box::new(self.source.evidence().clone()),
            usage: if budget.request_id() == self.request {
                budget.usage()
            } else {
                self.last_usage
            },
        }
    }
    fn bind(
        mut source: S,
        admission: RuntimeLayoutAdmission,
        release: &str,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        let checked = (|| {
            budget.check()?;
            if release.is_empty() || release.len() > 64 || !release.is_ascii() {
                return Err("reader.runtime_release".into());
            }
            if source.evidence().recipe != Some(admission.profile.recipe) {
                return Err("reader.runtime_recipe_mismatch".into());
            }
            budget.charge(RuntimeCost {
                allocation_bytes: release.len() as u64,
                work_units: 1,
                ..RuntimeCost::default()
            })?;
            source.verify_scope(&admission, budget)?;
            source.revalidate(budget)?;
            budget.check()
        })();
        if let Err(cause) = checked {
            return Err(RuntimeBindingFailure {
                cause,
                stage: "bind",
                root: Box::new(source.evidence().clone()),
                usage: budget.usage(),
            });
        }
        Ok(Self {
            source,
            admission,
            request: budget.request_id(),
            last_usage: budget.usage(),
            release: release.into(),
            poisoned: None,
        })
    }
    fn lookup(
        &mut self,
        selector: &Selector,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        if let Some(cause) = &self.poisoned {
            return Err(self.failure(cause.clone(), "poisoned", budget));
        }
        let mut stage = "before_lookup";
        let result = (|| {
            if budget.request_id() != self.request {
                return Err("reader.runtime_budget_identity".into());
            }
            budget.check()?;
            self.source.verify_scope(&self.admission, budget)?;
            self.source.revalidate(budget)?;
            budget.charge(RuntimeCost {
                allocation_bytes: self.admission.profile.profile.len() as u64
                    + self.release.len() as u64,
                work_units: 1,
                ..RuntimeCost::default()
            })?;
            stage = "lookup";
            let record = {
                let memory = LookupMemory {
                    source: &self.source,
                    budget: RefCell::new(&mut *budget),
                };
                Reader {
                    memory: &memory,
                    root: self.source.root(),
                    layout: self.admission.layout(),
                    module_hash: String::new(),
                    release: self.release.clone(),
                }
                .lookup_runtime(selector, &self.admission)?
            };
            stage = "after_lookup";
            self.source.revalidate(budget)?;
            self.source.verify_scope(&self.admission, budget)?;
            budget.check()?;
            Ok(record)
        })();
        if budget.request_id() == self.request {
            self.last_usage = budget.usage();
        }
        result.map_err(|cause: String| {
            self.poisoned = Some(cause.clone());
            self.failure(cause, stage, budget)
        })
    }
}
struct LookupMemory<'a, S> {
    source: &'a S,
    budget: RefCell<&'a mut RuntimeBudget>,
}
impl<S: Source> Memory for LookupMemory<'_, S> {
    fn verify(&self) -> Result<()> {
        self.source.verify(&mut self.budget.borrow_mut())
    }
    fn read(&self, address: u64, bytes: &mut [u8], module: bool) -> Result<()> {
        self.source
            .read(address, bytes, module, &mut self.budget.borrow_mut())
    }
    fn charge(&self, allocation_bytes: u64, work_units: u64) -> Result<()> {
        self.budget.borrow_mut().charge(RuntimeCost {
            allocation_bytes,
            work_units,
            ..RuntimeCost::default()
        })
    }
}

/// Held process/module/root plus an independently admitted Lua ABI. This
/// returns observations only: records do not authorize slots or game input.
/// Every call uses the original cumulative request ledger. Failure poisons the
/// binding; no implicit retry, cached heap pointer or fallback scan exists.
pub struct RuntimeBoundMemory {
    bound: Bound<OsSource>,
}
impl RuntimeBoundMemory {
    /// Recheck this held scope and copy descriptive facts under its original
    /// ledger. This DTO cannot reconstruct a module, root or input capability.
    pub fn description(
        &mut self,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RuntimeBindingDescription, RuntimeBindingFailure> {
        self.bound
            .describe(budget, |source, admission, release, budget| {
                let scope = admission
                    .scope
                    .as_ref()
                    .ok_or("reader.runtime_scope_missing")?;
                budget.charge(description_cost(&scope.client, release))?;
                let module = source.module.identity();
                Ok(RuntimeBindingDescription {
                    window: RuntimeWindowDescription {
                        handle: module.window().hwnd.to_string(),
                        process_id: module.process().pid,
                        process_started_at: module.process().created.to_string(),
                    },
                    module: RuntimeModuleDescription {
                        base: module.base().to_string(),
                        size: module.size(),
                        architecture: module.architecture(),
                    },
                    client: scope.client.clone(),
                    expected_release: release.into(),
                    layout_profile: admission.profile_id(),
                    initial_coverage: source.root.evidence().into(),
                    latest_coverage: source.evidence().into(),
                })
            })
    }
    /// Open the held runtime module, resolve its instruction recipe and bind the
    /// Lua reader under one cumulative request budget. No disk-backed root rule
    /// or caller-supplied module/root identity enters this path.
    pub fn open(
        window: lycheedev_windows::WindowIdentity,
        release: &str,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        let module =
            RuntimeModule::open(window, budget).map_err(|cause| RuntimeBindingFailure {
                cause,
                stage: "module",
                root: Box::default(),
                usage: budget.usage(),
            })?;
        let root = crate::runtime_root::RuntimeRootRecipe::resolve(&module, budget).map_err(
            |failure| RuntimeBindingFailure {
                cause: failure.cause,
                stage: "root",
                root: failure.evidence,
                usage: budget.usage(),
            },
        )?;
        let recipe = root.evidence().recipe.expect("resolved root has a recipe");
        let admission =
            RuntimeLayoutAdmission::select(&module, recipe, budget).map_err(|cause| {
                RuntimeBindingFailure {
                    cause,
                    stage: "layout",
                    root: Box::new(root.evidence().clone()),
                    usage: budget.usage(),
                }
            })?;
        Self::bind(module, root, admission, release, budget)
    }

    pub fn bind(
        module: RuntimeModule,
        root: RootBinding,
        admission: RuntimeLayoutAdmission,
        release: &str,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        if root.identity() != module.identity() {
            return Err(RuntimeBindingFailure {
                cause: "reader.runtime_module_identity".into(),
                stage: "bind",
                root: Box::new(root.evidence().clone()),
                usage: budget.usage(),
            });
        }
        if let Err(cause) = check_scope_identity(
            admission.scope.as_ref().map(|s| &s.module),
            module.identity(),
        ) {
            return Err(RuntimeBindingFailure {
                cause,
                stage: "bind",
                root: Box::new(root.evidence().clone()),
                usage: budget.usage(),
            });
        }
        let latest = root.evidence().clone();
        Bound::bind(
            OsSource {
                module,
                root,
                latest,
            },
            admission,
            release,
            budget,
        )
        .map(|bound| Self { bound })
    }
    pub fn lookup(
        &mut self,
        selector: &Selector,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        self.bound.lookup(selector, budget)
    }
    pub fn root_evidence(&self) -> &RootEvidence {
        self.bound.source.root.evidence()
    }
    /// Latest bounded anchor recheck, including failed/partial coverage. It
    /// does not claim that the full text domain was scanned again.
    pub fn latest_root_evidence(&self) -> &RootEvidence {
        self.bound.source.evidence()
    }
    pub fn profile_id(&self) -> &'static str {
        self.bound.admission.profile_id()
    }
}

#[cfg(test)]
mod tests;
