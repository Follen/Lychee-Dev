//! Runtime-only Lua path reader. Runtime code location and Lua ABI admission
//! remain separate capabilities. No executable hash is invented or imported.
#![forbid(unsafe_code)]
use crate::{
    lua_layout::Layout,
    mailbox::lookup::{Memory, Reader, Record, Selector},
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
    recipe: RecipeId,
    profile: &'static str,
    evidence_sha256: &'static str,
}

// Current runtime recipes share the fixed Lua ABI; client metadata and runtime
// instruction evidence remain independently authenticated.
const FORMAL_LAYOUT_SOURCE: &str =
    "ee1c7d473cc54257baed4622a212d29ad7f1f168a48d0a92cd0052e8931289f6";
static FORMAL_PROFILES: &[CatalogProfile] = &[
    CatalogProfile {
        recipe: RecipeId::Retail,
        profile: "go-3.2.0-lua-mailbox-rip-v1",
        evidence_sha256: FORMAL_LAYOUT_SOURCE,
    },
    CatalogProfile {
        recipe: RecipeId::Forever,
        profile: "go-3.2.0-lua-mailbox-runscript-v1",
        evidence_sha256: FORMAL_LAYOUT_SOURCE,
    },
    CatalogProfile {
        recipe: RecipeId::ForeverContext,
        profile: "go-3.2.0-lua-mailbox-context-v1",
        evidence_sha256: FORMAL_LAYOUT_SOURCE,
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
        budget.charge(RuntimeCost::work(FORMAL_PROFILES.len() as u64 * 512 + 1024))?;
        let mut admission = Self::select_formal(&client, recipe)?;
        budget.charge(RuntimeCost::allocation(std::mem::size_of::<
            RuntimeModuleIdentity,
        >()))?;
        admission.scope = Some(RuntimeScope {
            module: module.identity().clone(),
            client,
        });
        Ok(admission)
    }
    fn check_client(client: &ClientInstallation) -> Result<()> {
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
        Ok(())
    }
    fn select_formal(client: &ClientInstallation, recipe: RecipeId) -> Result<Self> {
        Self::check_client(client)?;
        let observed = client::product(client::client_data_product(client))
            .ok_or("reader.runtime_client_identity")?;
        if !client::PRODUCTS
            .iter()
            .any(|product| product.id == observed.id)
        {
            return Err("reader.runtime_layout_unsupported".into());
        }
        let profile = FORMAL_PROFILES
            .iter()
            .find(|p| p.recipe == recipe)
            .ok_or("reader.runtime_layout_unsupported")?;
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
    pub first_read_failure: Option<crate::mailbox::lookup::ReadFailure>,
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
    pub rejected_matches: u64,
    pub root_candidates: u64,
    pub text_bytes: u64,
    pub gaps: crate::runtime_root::RootGaps,
    pub read_failures: u64,
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
            rejected_matches: value.rejected_matches,
            root_candidates: value.root_candidates,
            text_bytes: value.text_bytes,
            gaps: value.gaps.clone(),
            read_failures: value.read_failures,
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
    fn close(self, budget: &RuntimeBudget) -> Result<()>
    where
        Self: Sized,
    {
        budget.check()
    }
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
    fn read_diagnostic(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
        self.read(address, bytes, module, budget)
            .map_err(crate::mailbox::lookup::ReadError::unknown)
    }
}
struct OsSource {
    module: RuntimeModule,
    root: RootBinding,
    latest: RootEvidence,
}
impl Source for OsSource {
    fn close(self, budget: &RuntimeBudget) -> Result<()> {
        self.module.close(budget)
    }
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
        self.read_diagnostic(address, bytes, module, budget)
            .map_err(|failure| failure.cause)
    }
    fn read_diagnostic(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
        runtime_read_diagnostic(&self.module, &self.root, address, bytes, module, budget)
    }
}

fn runtime_read_diagnostic(
    module: &RuntimeModule,
    root: &RootBinding,
    address: u64,
    bytes: &mut [u8],
    is_module: bool,
    budget: &mut RuntimeBudget,
) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
    let result = if is_module {
        if address != root.root_address() || bytes.len() != 8 {
            return Err(crate::mailbox::lookup::ReadError::unknown(
                "reader.runtime_root_range".into(),
            ));
        }
        module.read_exact_observed(
            root.root_rva(),
            bytes,
            RuntimeReadKind::WritableData,
            budget,
        )
    } else {
        module.read_private_observed(address, bytes, budget)
    };
    result.map_err(Into::into)
}
/// A temporary borrowing view probes candidate paths without reopening the
/// process or minting a module/root identity. It uses the same request ledger.
struct CandidateSource<'a> {
    module: &'a RuntimeModule,
    root: &'a mut RootBinding,
    latest: RootEvidence,
}
impl Source for CandidateSource<'_> {
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
        let current = fresh_client(self.module, budget)?;
        check_client_identity(&current, &scope.client, budget)
    }
    fn revalidate(&mut self, budget: &mut RuntimeBudget) -> Result<()> {
        match self.root.revalidate(self.module, budget) {
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
        self.read_diagnostic(address, bytes, module, budget)
            .map_err(|failure| failure.cause)
    }
    fn read_diagnostic(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
        runtime_read_diagnostic(self.module, self.root, address, bytes, module, budget)
    }
}
enum LookupSelector<'a> {
    Named(&'a Selector),
    Formal(&'a crate::mailbox::Selector),
}
fn transient_publication(cause: &str) -> bool {
    matches!(
        cause,
        "mailbox.path_changed" | "mailbox.lua_state_null" | "mailbox.string_layout"
    )
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
            first_read_failure: self.source.evidence().first_read_failure.clone(),
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
            let failure = RuntimeBindingFailure {
                cause,
                stage: "bind",
                root: Box::new(source.evidence().clone()),
                usage: budget.usage(),
                first_read_failure: source.evidence().first_read_failure.clone(),
            };
            return Err(join_close_failure(failure, source.close(budget)));
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
        self.lookup_selected(LookupSelector::Named(selector), None, budget)
    }
    fn lookup_formal(
        &mut self,
        selector: &crate::mailbox::Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        self.lookup_selected(LookupSelector::Formal(selector), accept, budget)
    }
    fn lookup_selected(
        &mut self,
        selector: LookupSelector<'_>,
        mut accept: Option<&mut dyn FnMut(&Record) -> bool>,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        if let Some(cause) = &self.poisoned {
            return Err(self.failure(cause.clone(), "poisoned", budget));
        }
        let mut stage = "before_lookup";
        let mut first_read_failure = None;
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
            let read = {
                let memory = LookupMemory {
                    source: &self.source,
                    budget: RefCell::new(&mut *budget),
                };
                let reader = Reader {
                    memory: &memory,
                    root: self.source.root(),
                    layout: self.admission.layout(),
                    release: self.release.clone(),
                };
                let read = match selector {
                    LookupSelector::Named(selector) => reader
                        .lookup_runtime(selector, &self.admission)
                        .map_err(crate::mailbox::lookup::LookupFailure::from),
                    LookupSelector::Formal(selector) => {
                        reader.lookup_runtime_formal(selector, &self.admission, None)
                    }
                };
                read.map_err(|failure| {
                    first_read_failure = failure.first_read_failure;
                    failure.cause
                })
            };
            if read.is_ok() {
                stage = "after_lookup";
            }
            // Failure is not an exit from qualification: the Lua path can race
            // simultaneously with the outer recipe/metadata. Recheck every
            // original scope under this ledger, retain the original read facts,
            // and join independent faults so a mixed failure is never transient.
            let mut checked = read;
            for guard in [
                self.source.revalidate(budget),
                self.source.verify_scope(&self.admission, budget),
                budget.check(),
            ] {
                if let Err(next) = guard {
                    // Even if the original read failed, this independent outer
                    // failure must abort candidate selection rather than merely
                    // rank this Lua root as malformed.
                    stage = "after_lookup";
                    checked = Err(match checked {
                        Ok(_) => next,
                        Err(mut cause) => {
                            for leaf in next.lines() {
                                if !cause.lines().any(|existing| existing == leaf) {
                                    cause.push('\n');
                                    cause.push_str(leaf);
                                }
                            }
                            cause
                        }
                    });
                }
            }
            let record = checked?;
            // The caller sees bytes only after BOTH the Lua path and outer
            // runtime recipe/metadata guards. Accept cannot suppress control.
            let accepted =
                record.filter(|record| accept.as_mut().is_none_or(|predicate| predicate(record)));
            stage = "after_accept";
            self.source.verify(budget)?;
            budget.check()?;
            Ok(accepted)
        })();
        if budget.request_id() == self.request {
            self.last_usage = budget.usage();
        }
        result.map_err(|cause: String| {
            // A public-path race/null/retired string clears this read, not the
            // held module/root. The driver alone decides whether the current
            // transaction may retry (null/string only after submitted reload).
            // Mixed errors, changed metadata/recipe, OS faults and control stops
            // remain poisoned. No automatic scan or budget replacement occurs.
            if !transient_publication(&cause) {
                self.poisoned = Some(cause.clone());
            }
            let mut failure = self.failure(cause, stage, budget);
            failure.first_read_failure = first_read_failure.or(failure.first_read_failure);
            failure
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
    fn read_diagnostic(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
    ) -> std::result::Result<(), crate::mailbox::lookup::ReadError> {
        self.source
            .read_diagnostic(address, bytes, module, &mut self.budget.borrow_mut())
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
    /// Explicit owned process cleanup. Closing is prepaid and cannot be skipped
    /// because the request's original control expired or its budget was spent.
    pub fn close(self, budget: &RuntimeBudget) -> Result<()> {
        self.bound.source.close(budget)
    }
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
        Self::open_selected(window, release, budget, false)
    }

    /// Formal reader constructor: the controlled, fully hashed EXE/PE layout
    /// is mandatory and stays retained by the selected root capability.
    pub fn open_formal(
        window: lycheedev_windows::WindowIdentity,
        release: &str,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        Self::open_selected(window, release, budget, true)
    }

    fn open_selected(
        window: lycheedev_windows::WindowIdentity,
        release: &str,
        budget: &mut RuntimeBudget,
        trusted: bool,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        let module =
            RuntimeModule::open(window, budget).map_err(|cause| RuntimeBindingFailure {
                cause,
                stage: "module",
                root: Box::default(),
                usage: budget.usage(),
                first_read_failure: None,
            })?;
        let prepared = (|| {
            let resolved = if trusted {
                let layout =
                    module
                        .verify_image_layout(budget)
                        .map_err(|cause| RuntimeBindingFailure {
                            cause,
                            stage: "trusted_image",
                            root: Box::default(),
                            usage: budget.usage(),
                            first_read_failure: None,
                        })?;
                crate::runtime_root::RuntimeRootRecipe::candidates_with_layout(
                    &module, layout, budget,
                )
            } else {
                crate::runtime_root::RuntimeRootRecipe::candidates(&module, budget)
            };
            let mut roots = resolved.map_err(|failure| RuntimeBindingFailure {
                cause: failure.cause,
                stage: "root",
                first_read_failure: failure.evidence.first_read_failure.clone(),
                root: failure.evidence,
                usage: budget.usage(),
            })?;
            let mut selected = 0;
            if roots.len() > 1 {
                budget
                    .charge(RuntimeCost::allocation(
                        roots.len() * std::mem::size_of::<std::result::Result<bool, String>>(),
                    ))
                    .map_err(|cause| RuntimeBindingFailure {
                        cause,
                        stage: "root_selection",
                        root: Box::new(roots[0].evidence().clone()),
                        usage: budget.usage(),
                        first_read_failure: None,
                    })?;
                let mut observations = Vec::with_capacity(roots.len());
                for root in &mut roots {
                    let recipe = root.evidence().recipe.expect("resolved root has a recipe");
                    let admission = RuntimeLayoutAdmission::select(&module, recipe, budget)
                        .map_err(|cause| RuntimeBindingFailure {
                            cause,
                            stage: "layout",
                            root: Box::new(root.evidence().clone()),
                            usage: budget.usage(),
                            first_read_failure: None,
                        })?;
                    let latest = root.evidence().clone();
                    let source = CandidateSource {
                        module: &module,
                        root,
                        latest,
                    };
                    let selector = crate::mailbox::Selector {
                        kind: crate::mailbox::Kind::Identity,
                        nonce: crate::mailbox::ZERO_TOKEN,
                        runtime: crate::mailbox::ZERO_TOKEN,
                        ticket: crate::mailbox::ZERO_TOKEN,
                        body: None,
                    };
                    let result = Bound::bind(source, admission, release, budget)
                        .and_then(|mut bound| bound.lookup_formal(&selector, None, budget));
                    match result {
                        Ok(record) => observations.push(Ok(record.is_some())),
                        Err(failure) => {
                            // Fixed Go aborts a moving path/control immediately;
                            // another candidate cannot turn either into absence.
                            if failure.stage != "lookup"
                                || failure.cause == "mailbox.path_changed"
                                || fatal_lookup_cause(&failure.cause)
                                || budget.check().is_err()
                            {
                                return Err(failure);
                            }
                            observations.push(Err(failure.cause));
                        }
                    }
                }
                selected =
                    choose_observed_root(&observations).map_err(|cause| RuntimeBindingFailure {
                        cause,
                        stage: "root_selection",
                        root: Box::new(roots[0].evidence().clone()),
                        usage: budget.usage(),
                        first_read_failure: None,
                    })?;
            }
            let root = roots.remove(selected);
            let recipe = root.evidence().recipe.expect("resolved root has a recipe");
            let admission =
                RuntimeLayoutAdmission::select(&module, recipe, budget).map_err(|cause| {
                    RuntimeBindingFailure {
                        cause,
                        stage: "layout",
                        root: Box::new(root.evidence().clone()),
                        usage: budget.usage(),
                        first_read_failure: None,
                    }
                })?;
            Ok((root, admission))
        })();
        match prepared {
            Ok((root, admission)) => Self::bind(module, root, admission, release, budget),
            Err(failure) => Err(join_close_failure(failure, module.close(budget))),
        }
    }

    pub fn bind(
        module: RuntimeModule,
        root: RootBinding,
        admission: RuntimeLayoutAdmission,
        release: &str,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        if root.identity() != module.identity() {
            let failure = RuntimeBindingFailure {
                cause: "reader.runtime_module_identity".into(),
                stage: "bind",
                root: Box::new(root.evidence().clone()),
                usage: budget.usage(),
                first_read_failure: None,
            };
            return Err(join_close_failure(failure, module.close(budget)));
        }
        if let Err(cause) = check_scope_identity(
            admission.scope.as_ref().map(|s| &s.module),
            module.identity(),
        ) {
            let failure = RuntimeBindingFailure {
                cause,
                stage: "bind",
                root: Box::new(root.evidence().clone()),
                usage: budget.usage(),
                first_read_failure: None,
            };
            return Err(join_close_failure(failure, module.close(budget)));
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
    pub fn lookup_formal(
        &mut self,
        selector: &crate::mailbox::Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        self.bound.lookup_formal(selector, accept, budget)
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

fn join_close_failure(
    mut failure: RuntimeBindingFailure,
    close: Result<()>,
) -> RuntimeBindingFailure {
    if let Err(cause) = close {
        for leaf in cause.lines() {
            if !failure.cause.lines().any(|existing| existing == leaf) {
                failure.cause.push('\n');
                failure.cause.push_str(leaf);
            }
        }
    }
    failure
}

fn fatal_lookup_cause(cause: &str) -> bool {
    cause.lines().any(|line| {
        line.starts_with("process.")
            || line.starts_with("window.")
            || line.starts_with("reader.runtime_")
            || line.starts_with("root.")
            || line.starts_with("memory.runtime_budget")
            || matches!(
                line,
                "memory.runtime_cancelled"
                    | "memory.runtime_deadline"
                    | "command.cancelled"
                    | "command.deadline"
            )
    })
}
fn choose_observed_root(observations: &[std::result::Result<bool, String>]) -> Result<usize> {
    let mut best = 0;
    let mut selected = 0;
    let mut count = 0;
    let mut failures = Vec::new();
    for (index, observation) in observations.iter().enumerate() {
        let score = match observation {
            Ok(true) => 2,
            Ok(false) => 1,
            Err(cause)
                if cause.starts_with("memory.mailbox_incompatible:") && !cause.contains('\n') =>
            {
                2
            }
            Err(cause) => {
                failures.push(cause.as_str());
                continue;
            }
        };
        if score > best {
            best = score;
            selected = index;
            count = 1;
        } else if score == best {
            count += 1;
        }
    }
    if count != 1 {
        let mut cause = "root.ambiguous_or_unverified_lua_roots".to_string();
        for failure in failures {
            cause.push('\n');
            cause.push_str(failure);
        }
        return Err(cause);
    }
    Ok(selected)
}

#[cfg(test)]
mod tests;
