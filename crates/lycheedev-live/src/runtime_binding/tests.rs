use super::*;
use crate::mailbox;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

static CATALOG: &[CatalogProfile] = &[CatalogProfile {
    product: "forever",
    catalog_product: "wow_classic_beta",
    channel: "asia-beta",
    full_build: "1.60.1.70338",
    recipe: RecipeId::Forever,
    profile: "synthetic-only-fixed-lua",
    evidence_sha256: "1111111111111111111111111111111111111111111111111111111111111111",
}];
fn client() -> ClientInstallation {
    ClientInstallation {
        directory: "synthetic".into(),
        product: "forever".into(),
        product_code: "wow_forever".into(),
        catalog_product: "wow_classic_beta".into(),
        display_name: "Forever Asia Beta".into(),
        channel: "asia-beta".into(),
        metadata_branch: "us".into(),
        full_build: "1.60.1.70338".into(),
        interface: 16001,
        interface_known: true,
        toc: "Lychee Dev.toc".into(),
        identity_source: "synthetic".into(),
    }
}
fn admission() -> RuntimeLayoutAdmission {
    RuntimeLayoutAdmission::select_from(&client(), RecipeId::Forever, CATALOG).unwrap()
}
fn budget() -> RuntimeBudget {
    RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}

struct Fixture {
    evidence: RootEvidence,
    bytes: BTreeMap<u64, Vec<u8>>,
    next: u64,
    validations: usize,
    fail_validation: usize,
    read_calls: Cell<usize>,
    fail_read: Cell<bool>,
    cancel_on_read: Option<Arc<AtomicBool>>,
    close_calls: Option<Arc<AtomicUsize>>,
    close_error: Option<&'static str>,
    tear_root: Cell<bool>,
    scope_checks: Cell<usize>,
    change_scope_at: usize,
    dropped: Option<Arc<AtomicUsize>>,
}
impl Fixture {
    fn new() -> Self {
        let mut this = Self {
            evidence: RootEvidence {
                recipe: Some(RecipeId::Forever),
                root_rva: Some(0x1000),
                full_text_pass: true,
                stable_guards: true,
                ..RootEvidence::default()
            },
            bytes: BTreeMap::new(),
            next: 0x2000,
            validations: 0,
            fail_validation: usize::MAX,
            read_calls: Cell::new(0),
            fail_read: Cell::new(false),
            cancel_on_read: None,
            close_calls: None,
            close_error: None,
            tear_root: Cell::new(false),
            scope_checks: Cell::new(0),
            change_scope_at: usize::MAX,
            dropped: None,
        };
        let globals = this.table(&[]);
        this.state(globals);
        this
    }
    fn alloc(&mut self, bytes: Vec<u8>) -> u64 {
        let address = self.next;
        self.next += (bytes.len() as u64 + 15) & !15;
        self.bytes.insert(address, bytes);
        address
    }
    fn string(&mut self, value: &[u8]) -> u64 {
        let mut bytes = vec![0; 32];
        bytes[16] = 4;
        bytes[24..32].copy_from_slice(&(value.len() as u64).to_le_bytes());
        bytes.extend_from_slice(value);
        self.alloc(bytes)
    }
    fn table(&mut self, fields: &[(&str, u64, u8)]) -> u64 {
        let mut nodes = vec![0u8; 256 * 56];
        let hash = |key: &str| {
            let mut hash = key.len() as u32;
            let step = (key.len() >> 5) + 1;
            let mut n = key.len();
            while n >= step {
                hash ^= hash
                    .wrapping_shl(5)
                    .wrapping_add(hash >> 2)
                    .wrapping_add(key.as_bytes()[n - 1] as u32);
                n -= step;
            }
            (hash & 255) as usize
        };
        let primary: BTreeSet<_> = fields.iter().map(|(key, _, _)| hash(key)).collect();
        let mut used = BTreeSet::new();
        let mut tails = BTreeMap::new();
        let node_address = self.next;
        self.next += nodes.len() as u64;
        for &(key, value, tag) in fields {
            let bucket = hash(key);
            let index = if used.contains(&bucket) {
                (0..256)
                    .find(|i| !primary.contains(i) && !used.contains(i))
                    .unwrap()
            } else {
                bucket
            };
            used.insert(index);
            let at = index * 56;
            if let Some(previous) = tails.insert(bucket, index) {
                let link = previous * 56 + 48;
                nodes[link..link + 8].copy_from_slice(&(node_address + at as u64).to_le_bytes());
            }
            let name = self.string(key.as_bytes());
            nodes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            nodes[at + 8] = tag;
            nodes[at + 24..at + 32].copy_from_slice(&name.to_le_bytes());
            nodes[at + 32] = 4;
        }
        self.bytes.insert(node_address, nodes);
        let mut table = vec![0; 72];
        table[16] = 5;
        table[19] = 8;
        table[40..48].copy_from_slice(&node_address.to_le_bytes());
        self.alloc(table)
    }
    fn state(&mut self, globals: u64) {
        let mut state = vec![0; 0x90 + 24];
        state[16] = 8;
        state[0x90..0x98].copy_from_slice(&globals.to_le_bytes());
        state[0x98] = 5;
        let address = self.alloc(state);
        self.bytes.insert(0x1000, address.to_le_bytes().to_vec());
    }
    fn publication(&mut self) {
        self.publication_payload(b"identity", "3.1.2");
    }
    fn publication_payload(&mut self, payload: &[u8], expected_release: &str) {
        let r = Record {
            address: 0,
            nonce: "0".repeat(32),
            runtime: "2".repeat(32),
            ticket: "0".repeat(32),
            kind: 1,
            state: 1,
            sequence: 1,
            checksum: 0,
            payload: payload.to_vec(),
        };
        let record = self.string(&mailbox::encode(&r).unwrap());
        let schema = self.string(crate::go_mailbox_v1::SCHEMA.as_bytes());
        let release = self.string(expected_release.as_bytes());
        let runtime = self.string(r.runtime.as_bytes());
        let mailbox = self.table(&[
            ("schema", schema, 4),
            ("release", release, 4),
            ("runtime", runtime, 4),
            ("identity", record, 4),
        ]);
        let namespace = self.table(&[("Mailbox", mailbox, 5)]);
        let globals = self.table(&[("LycheeDevInternal", namespace, 5)]);
        self.state(globals);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(dropped) = &self.dropped {
            dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
}
impl Source for Fixture {
    fn close(self, budget: &RuntimeBudget) -> Result<()> {
        if let Some(calls) = &self.close_calls {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        match self.close_error {
            Some(cause) => Err(cause.into()),
            None => budget.check(),
        }
    }
    fn root(&self) -> u64 {
        0x1000
    }
    fn evidence(&self) -> &RootEvidence {
        &self.evidence
    }
    fn verify_scope(&self, _: &RuntimeLayoutAdmission, budget: &mut RuntimeBudget) -> Result<()> {
        self.scope_checks.set(self.scope_checks.get() + 1);
        let issued = client();
        let mut current = client();
        if self.scope_checks.get() == self.change_scope_at {
            current.full_build.push('1');
        }
        check_client_identity(&current, &issued, budget)
    }
    fn revalidate(&mut self, budget: &mut RuntimeBudget) -> Result<()> {
        budget.charge(RuntimeCost::work(10))?;
        self.validations += 1;
        if self.validations == self.fail_validation {
            self.evidence.stable_guards = false;
            return Err("fixture.anchor_changed".into());
        }
        Ok(())
    }
    fn verify(&self, budget: &mut RuntimeBudget) -> Result<()> {
        budget.charge(RuntimeCost::work(1))
    }
    fn read(
        &self,
        address: u64,
        out: &mut [u8],
        module: bool,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        budget.charge(RuntimeCost {
            read_calls: 1,
            requested_bytes: out.len() as u64,
            ..RuntimeCost::default()
        })?;
        self.read_calls.set(self.read_calls.get() + 1);
        if let Some(cancel) = &self.cancel_on_read {
            cancel.store(true, Ordering::Release);
        }
        if self.fail_read.get() {
            return Err("fixture.os_partial_copy:299".into());
        }
        if module && (address != 0x1000 || out.len() != 8) {
            return Err("fixture.module_range".into());
        }
        let (base, bytes) = self
            .bytes
            .range(..=address)
            .next_back()
            .ok_or("fixture.unmapped")?;
        let at = (address - base) as usize;
        out.copy_from_slice(bytes.get(at..at + out.len()).ok_or("fixture.short")?);
        if module && self.tear_root.get() && self.read_calls.get() > 1 {
            out[0] ^= 16;
        }
        Ok(())
    }
}
fn bound(source: Fixture, budget: &mut RuntimeBudget) -> Bound<Fixture> {
    Bound::bind(source, admission(), "3.1.2", budget).unwrap()
}

mod observation {
    use super::*;
    use crate::runtime_observation::{
        self, ObservationReader, RuntimeObservationFailure, RuntimeTargetObservation,
    };
    use lycheedev_core::control::OperationControl;
    use serde_json::{Value, json};

    fn identity() -> Value {
        json!({"schema":crate::go_mailbox_v1::SCHEMA,"runtime":"2".repeat(32),"nextSlot":1,"inputState":crate::go_mailbox_v1::SCHEMA,
            "guid":"Player-1-ABC","character":"测试角色","realm":"测试 realm", "product":"forever",
            "build":"1.60.1.70338","release":lycheedev_core::VERSION,"slots":500,"inventory":500})
    }
    fn observation_fixture(payload: Option<&Value>) -> (Fixture, Arc<AtomicUsize>) {
        let mut fixture = Fixture::new();
        let dropped = Arc::new(AtomicUsize::new(0));
        fixture.dropped = Some(dropped.clone());
        fixture.evidence.pattern_hits = 1;
        fixture.evidence.header_bytes = 512;
        fixture.evidence.scanned_bytes = 4096;
        fixture.evidence.module_read_calls = 3;
        fixture.evidence.module_read_bytes = 4608;
        if let Some(payload) = payload {
            fixture.publication_payload(
                &serde_json::to_vec(payload).unwrap(),
                lycheedev_core::VERSION,
            );
        }
        (fixture, dropped)
    }
    fn description(
        source: &Fixture,
        admission: &RuntimeLayoutAdmission,
        release: &str,
        budget: &mut RuntimeBudget,
    ) -> Result<RuntimeBindingDescription> {
        let client = client();
        budget.charge(description_cost(&client, release))?;
        Ok(RuntimeBindingDescription {
            window: RuntimeWindowDescription {
                handle: "18446744073709551000".into(),
                process_id: 42,
                process_started_at: "134362582180138573".into(),
            },
            module: RuntimeModuleDescription {
                base: "5368709120".into(),
                size: 65536,
                architecture: 0x8664,
            },
            client,
            expected_release: release.into(),
            layout_profile: admission.profile_id(),
            initial_coverage: source.evidence().into(),
            latest_coverage: source.evidence().into(),
        })
    }
    impl ObservationReader for Bound<Fixture> {
        fn identity(
            &mut self,
            budget: &mut RuntimeBudget,
        ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
            self.lookup(&Selector::Identity, budget)
        }
        fn description(
            &mut self,
            budget: &mut RuntimeBudget,
        ) -> std::result::Result<RuntimeBindingDescription, RuntimeBindingFailure> {
            self.describe(budget, description)
        }
        fn coverage(&self) -> RuntimeCoverage {
            self.source.evidence().into()
        }
    }
    fn observe(
        fixture: Fixture,
        limits: RuntimeLimits,
    ) -> std::result::Result<RuntimeTargetObservation, RuntimeObservationFailure> {
        runtime_observation::observe_with(&OperationControl::default(), limits, |budget| {
            // Model the open/close reservation in the same ledger, then use the
            // existing actual named-Lua-path fixture reader for all observations.
            budget
                .charge(RuntimeCost {
                    os_call_units: 3,
                    ..RuntimeCost::default()
                })
                .map_err(|cause| RuntimeBindingFailure {
                    cause,
                    stage: "module",
                    root: Box::new(fixture.evidence.clone()),
                    usage: budget.usage(),
                    first_read_failure: None,
                })?;
            Bound::bind(fixture, admission(), lycheedev_core::VERSION, budget)
        })
    }

    #[test]
    fn one_ledger_observes_identity_and_serializes_only_descriptive_facts() {
        let (fixture, dropped) = observation_fixture(Some(&identity()));
        let observed = observe(fixture, RuntimeLimits::defaults()).unwrap();
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(observed.identity.runtime, "2".repeat(32));
        assert_eq!(observed.identity.guid, "Player-1-ABC");
        assert_eq!(observed.identity.character, "测试角色");
        assert!(!observed.identity.activation_only);
        assert_eq!(observed.binding.client.product_code, "wow_forever");
        assert_eq!(observed.budget.used.os_call_units, 3);
        assert!(observed.budget.used.read_calls > 10);
        assert!(observed.budget.used.work_units > 4 * 131072);
        assert!(observed.budget.used.allocation_bytes > 0);
        let value = serde_json::to_value(observed).unwrap();
        assert_eq!(value["authority"], "observation_only");
        assert_eq!(value["binding"]["window"]["handle"], "18446744073709551000");
        assert_eq!(
            value["binding"]["window"]["processStartedAt"],
            "134362582180138573"
        );
        assert_eq!(
            value["binding"]["module"],
            json!({"base":"5368709120","size":65536,"architecture":0x8664})
        );
        assert_eq!(value["binding"]["initialCoverage"]["fullTextPass"], true);
        assert_eq!(value["binding"]["latestCoverage"]["atomic"], false);
        fn no_capabilities(value: &Value) {
            match value {
                Value::Object(fields) => {
                    for (name, value) in fields {
                        assert!(
                            ![
                                "serial",
                                "request",
                                "requestId",
                                "root",
                                "rootRva",
                                "rootAddress",
                                "anchorRva",
                                "address",
                                "state",
                                "heap",
                                "moduleSha256",
                                "nonce",
                                "ticket",
                                "owner",
                                "fence"
                            ]
                            .contains(&name.as_str()),
                            "leaked {name}"
                        );
                        no_capabilities(value);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        no_capabilities(value);
                    }
                }
                _ => {}
            }
        }
        no_capabilities(&value);
    }
    #[test]
    fn activation_only_identity_is_observation_without_input_authority() {
        let mut payload = identity();
        payload["schema"] = crate::go_mailbox_v1::LEGACY_IDENTITY_SCHEMA.into();
        payload["inputState"] = crate::go_mailbox_v1::LEGACY_HYBRID_SCHEMA.into();
        payload["slots"] = 200.into();
        payload["inventory"] = 200.into();
        let (fixture, dropped) = observation_fixture(Some(&payload));
        let observed = observe(fixture, RuntimeLimits::defaults()).unwrap();
        assert!(observed.identity.activation_only);
        assert_eq!(observed.identity.inventory, Some(200));
        assert_eq!(
            observed.identity.compatibility,
            "manual_activation_required"
        );
        assert_eq!(
            observed.identity.diagnostic_codes,
            [
                "memory.mailbox_incompatible",
                "live.channel_legacy_identity_requires_activation"
            ]
        );
        assert_eq!(
            serde_json::to_value(&observed).unwrap()["authority"],
            "observation_only"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn current_capacity_does_not_invent_loaded_inventory_or_input_authority() {
        for inventory in [None, Some(0), Some(200), Some(500)] {
            let mut payload = identity();
            if let Some(count) = inventory {
                payload["inventory"] = json!(count);
            } else {
                payload.as_object_mut().unwrap().remove("inventory");
            }
            let (fixture, dropped) = observation_fixture(Some(&payload));
            let observed = observe(fixture, RuntimeLimits::defaults()).unwrap();
            assert_eq!(observed.identity.slots, 500);
            assert_eq!(observed.identity.inventory, inventory);
            assert_eq!(observed.identity.compatibility, "current");
            assert!(!observed.identity.activation_only);
            assert_eq!(
                serde_json::to_value(&observed).unwrap()["authority"],
                "observation_only"
            );
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
    }
    #[test]
    fn identity_disagreements_fail_after_real_named_lookup_and_release_reader() {
        for (field, value, cause) in [
            (
                "runtime",
                json!("3".repeat(32)),
                "live.channel_identity_record_invalid",
            ),
            ("product", json!("cn-beta"), "live.runtime_identity_product"),
            (
                "build",
                json!("1.60.1.70124"),
                "live.runtime_identity_build",
            ),
            (
                "release",
                json!("wrong"),
                "live.channel_identity_record_invalid",
            ),
            (
                "schema",
                json!("identity.v4"),
                "live.channel_identity_record_invalid",
            ),
            ("guid", json!(""), "live.channel_identity_record_invalid"),
            (
                "inventory",
                json!(501),
                "live.channel_identity_record_invalid",
            ),
            ("slots", json!(64), "live.channel_identity_record_invalid"),
        ] {
            let mut payload = identity();
            payload[field] = value;
            let (fixture, dropped) = observation_fixture(Some(&payload));
            let failure = observe(fixture, RuntimeLimits::defaults()).unwrap_err();
            assert_eq!(failure.cause, cause, "{field}");
            assert_eq!(failure.stage, "identity");
            assert!(failure.budget.unwrap().used.read_calls > 10);
            assert_eq!(dropped.load(Ordering::SeqCst), 1);
        }
        let mut payload = identity();
        payload["inventory"] = json!("500");
        let (fixture, dropped) = observation_fixture(Some(&payload));
        assert!(
            observe(fixture, RuntimeLimits::defaults())
                .unwrap_err()
                .cause
                .starts_with("live.channel_identity_record_invalid")
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn missing_identity_and_metadata_change_remain_failures_with_coverage() {
        let (fixture, dropped) = observation_fixture(None);
        let failure = observe(fixture, RuntimeLimits::defaults()).unwrap_err();
        assert_eq!(failure.cause, "live.runtime_identity_unavailable");
        assert!(failure.coverage.full_text_pass);
        assert!(failure.budget.unwrap().used.read_calls > 0);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let (mut fixture, dropped) = observation_fixture(Some(&identity()));
        fixture.change_scope_at = 4; // bind + before/after lookup + description
        let failure = observe(fixture, RuntimeLimits::defaults()).unwrap_err();
        assert_eq!(failure.cause, "reader.runtime_metadata_changed");
        assert_eq!(failure.stage, "description");
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn shared_budget_exhaustion_and_os_error_do_not_return_a_target_or_retain_reader() {
        let (fixture, dropped) = observation_fixture(Some(&identity()));
        let mut limits = RuntimeLimits::defaults();
        limits.read_calls = 1;
        let failure = observe(fixture, limits).unwrap_err();
        assert_eq!(failure.cause, "memory.runtime_budget:reads");
        assert_eq!(failure.budget.unwrap().used.read_calls, 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let (fixture, dropped) = observation_fixture(Some(&identity()));
        fixture.fail_read.set(true);
        let failure = observe(fixture, RuntimeLimits::defaults()).unwrap_err();
        assert_eq!(failure.cause, "fixture.os_partial_copy:299");
        assert_eq!(failure.stage, "lookup");
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn bind_failure_and_outer_publication_release_mismatch_drop_owned_source() {
        let (mut fixture, dropped) = observation_fixture(Some(&identity()));
        fixture.fail_validation = 1;
        let failure = observe(fixture, RuntimeLimits::defaults()).unwrap_err();
        assert_eq!(failure.cause, "fixture.anchor_changed");
        assert_eq!(failure.stage, "bind");
        assert!(!failure.coverage.stable_guards);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        let (mut fixture, dropped) = observation_fixture(None);
        fixture.publication_payload(&serde_json::to_vec(&identity()).unwrap(), "other-release");
        let failure = observe(fixture, RuntimeLimits::defaults()).unwrap_err();
        assert_eq!(
            failure.cause,
            "memory.mailbox_incompatible: mailbox_release_mismatch"
        );
        assert_eq!(failure.stage, "lookup");
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn cancellation_before_open_does_not_issue_a_reader() {
        let control = OperationControl::default();
        control.cancel_handle().store(true, Ordering::Release);
        let opened = Cell::new(false);
        let failure = runtime_observation::observe_with::<Bound<Fixture>>(
            &control,
            RuntimeLimits::defaults(),
            |_| {
                opened.set(true);
                panic!("cancelled request opened reader")
            },
        )
        .unwrap_err();
        assert_eq!(failure.cause, "memory.runtime_cancelled");
        assert_eq!(failure.stage, "budget");
        assert!(failure.budget.is_none());
        assert!(!opened.get());
    }
    #[test]
    fn description_rejects_a_replacement_ledger_and_retains_original_usage() {
        let mut budget = super::budget();
        let mut reader = bound(Fixture::new(), &mut budget);
        let used = budget.usage();
        let failure = reader
            .describe(&mut super::budget(), description)
            .unwrap_err();
        assert_eq!(failure.cause, "reader.runtime_budget_identity");
        assert_eq!(failure.usage, used);
        assert_eq!(reader.source.read_calls.get(), 0);
        assert_eq!(
            reader.describe(&mut budget, description).unwrap_err().stage,
            "poisoned"
        );
    }
}

#[test]
fn unrecorded_build_does_not_inherit_historical_or_synthetic_abi() {
    assert_eq!(
        RuntimeLayoutAdmission::select_from(&client(), RecipeId::Forever, PROFILES)
            .err()
            .unwrap(),
        "reader.runtime_layout_unsupported"
    );
}
#[test]
fn historical_admission_matches_archived_lua_layout_and_exact_client_scope() {
    let profiles: [&[u8]; 4] = [
        include_bytes!("../../../../docs/toolkit/research/lua-mailbox/root.profile.json"),
        include_bytes!(
            "../../../../docs/toolkit/research/lua-mailbox-classic-70032/root.profile.json"
        ),
        include_bytes!(
            "../../../../docs/toolkit/research/lua-mailbox-titan-69874/root.profile.json"
        ),
        include_bytes!(
            "../../../../docs/toolkit/research/lua-mailbox-forever-70124/root-runscript.profile.json"
        ),
    ];
    for (profile, raw) in PROFILES.iter().zip(profiles) {
        let recorded: serde_json::Value = serde_json::from_slice(raw).unwrap();
        assert_eq!(
            lycheedev_core::storage::sha256(raw),
            profile.evidence_sha256
        );
        assert_eq!(recorded["id"], profile.profile);
        assert_eq!(
            recorded["publicPath"],
            serde_json::json!(["_G", "LycheeDevInternal", "Mailbox"])
        );
        let known = client::catalog_product(profile.catalog_product).unwrap();
        let mut c = client();
        c.product = profile.product.into();
        c.product_code = if known.family == "forever" {
            "wow_forever".into()
        } else {
            profile.catalog_product.into()
        };
        c.catalog_product = if known.id != known.family {
            profile.catalog_product.into()
        } else {
            String::new()
        };
        c.channel = profile.channel.into();
        c.full_build = profile.full_build.into();
        c.interface = known.interface.unwrap();
        let admitted = RuntimeLayoutAdmission::select_from(&c, profile.recipe, PROFILES).unwrap();
        let layout = admitted.layout();
        let offset = |value: &serde_json::Value| {
            u64::from_str_radix(value.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
        };
        assert_eq!(
            offset(&recorded["layoutAssumptions"]["globalsTValueOffset"]),
            layout.state_globals
        );
        assert_eq!(
            offset(&recorded["types"]["Table"]["fields"]["tag"]["offset"]),
            layout.tag_offset
        );
        assert_eq!(
            offset(&recorded["types"]["TString"]["fields"]["length"]["offset"]),
            layout.string_length
        );
        assert_eq!(
            offset(&recorded["layoutAssumptions"]["stringDataOffset"]),
            layout.string_data
        );
        assert_eq!(
            offset(&recorded["types"]["Table"]["fields"]["nodes"]["offset"]),
            layout.table_nodes
        );
        assert_eq!(
            offset(&recorded["types"]["Table"]["fields"]["logNodes"]["offset"]),
            layout.table_log2
        );
        assert_eq!(
            recorded["types"]["Node"]["size"].as_u64().unwrap(),
            layout.node_bytes
        );
        assert_eq!(recorded["types"]["TValue"]["size"], 24);
        for (path, expected) in [
            ("/types/TValue/fields/tag/offset", 8),
            ("/types/TValue/fields/secret/offset", 9),
            ("/types/Node/fields/value/offset", 0),
            ("/types/Node/fields/valueTag/offset", 8),
            ("/types/Node/fields/valueSecret/offset", 9),
            ("/types/Node/fields/key/offset", 24),
            ("/types/Node/fields/keyTag/offset", 32),
            ("/types/Node/fields/keySecret/offset", 33),
            ("/types/Node/fields/next/offset", 48),
            ("/types/TString/fields/hash/offset", 20),
        ] {
            assert_eq!(offset(recorded.pointer(path).unwrap()), expected);
        }
        c.full_build.push('1');
        assert!(RuntimeLayoutAdmission::select_from(&c, profile.recipe, PROFILES).is_err());
        c.full_build = profile.full_build.into();
        c.product = "forever".into();
        c.catalog_product = "wow_cn_beta".into();
        c.product_code = "wow_forever".into();
        c.channel = "cn-beta".into();
        assert!(RuntimeLayoutAdmission::select_from(&c, profile.recipe, PROFILES).is_err());
    }
}
#[test]
fn admission_requires_exact_product_channel_build_and_recipe() {
    let a = admission();
    assert_eq!(a.profile_id(), "synthetic-only-fixed-lua");
    assert_eq!(a.evidence_sha256(), CATALOG[0].evidence_sha256);
    assert!(RuntimeLayoutAdmission::select_from(&client(), RecipeId::Retail, CATALOG).is_err());
    for which in 0..5 {
        let mut c = client();
        match which {
            0 => c.product = "cn-beta".into(),
            1 => c.catalog_product = "wow_cn_beta".into(),
            2 => c.channel = "cn-beta".into(),
            3 => c.full_build = "1.60.1.70339".into(),
            _ => c.product_code = "wow_cn_beta".into(),
        }
        assert!(RuntimeLayoutAdmission::select_from(&c, RecipeId::Forever, CATALOG).is_err());
    }
    let mut cn = client();
    cn.product = "forever".into();
    cn.product_code = "wow_forever".into();
    cn.catalog_product = "wow_cn_beta".into();
    cn.channel = "cn-beta".into();
    assert_eq!(
        RuntimeLayoutAdmission::select_from(&cn, RecipeId::Forever, CATALOG)
            .err()
            .unwrap(),
        "reader.runtime_layout_unsupported"
    );
}
#[test]
fn layout_admission_rejects_every_altered_field_and_fake_hash() {
    let a = admission();
    for field in 0..9 {
        let mut l = a.layout();
        match field {
            0 => l.id.push('x'),
            1 => l.applicable_hashes.push("fake".into()),
            2 => l.state_globals += 1,
            3 => l.tag_offset += 1,
            4 => l.string_length += 1,
            5 => l.string_data += 1,
            6 => l.table_nodes += 1,
            7 => l.table_log2 += 1,
            _ => l.node_bytes += 1,
        }
        assert!(a.admits_layout(&l).is_err());
    }
    assert!(a.layout().admit("").is_err());
    assert!(a.admits_layout(&a.layout()).is_ok());
}
#[test]
fn bind_recipe_mismatch_never_reads_or_revalidates() {
    let mut f = Fixture::new();
    f.evidence.recipe = Some(RecipeId::Retail);
    let error = Bound::bind(f, admission(), "3.1.2", &mut budget())
        .err()
        .unwrap();
    assert_eq!(error.cause, "reader.runtime_recipe_mismatch");
    assert_eq!(error.usage, RuntimeCost::default());
}
#[test]
fn stable_absence_keeps_request_usage_and_rechecks_each_lookup() {
    let mut b = budget();
    let mut reader = bound(Fixture::new(), &mut b);
    assert!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap()
            .is_none()
    );
    let first = b.usage();
    assert!(first.read_calls > 0 && first.allocation_bytes > 0);
    assert_eq!(reader.source.validations, 3);
    assert!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap()
            .is_none()
    );
    assert_eq!(reader.source.validations, 5);
    assert!(b.usage().read_calls > first.read_calls);
    assert!(b.usage().allocation_bytes > first.allocation_bytes);
}
#[test]
fn bounded_publication_uses_named_path_and_returns_record() {
    let mut f = Fixture::new();
    f.publication();
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let r = reader.lookup(&Selector::Identity, &mut b).unwrap().unwrap();
    assert_eq!(r.payload, b"identity");
    assert!(r.address > 0 && b.usage().read_calls > 10);
    assert_eq!(reader.source.validations, 3);
}
#[test]
fn replacement_budget_poison_cannot_reset_ledger() {
    let mut b = budget();
    let mut reader = bound(Fixture::new(), &mut b);
    let original = b.usage();
    let e = reader
        .lookup(&Selector::Identity, &mut budget())
        .unwrap_err();
    assert_eq!(e.cause, "reader.runtime_budget_identity");
    assert_eq!(e.usage, original);
    assert_eq!(reader.source.read_calls.get(), 0);
    assert_eq!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap_err()
            .stage,
        "poisoned"
    );
    assert_eq!(b.usage(), original);
}
#[test]
fn cancellation_before_lookup_reads_nothing() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut b = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(30),
        cancel.clone(),
    )
    .unwrap();
    let mut reader = bound(Fixture::new(), &mut b);
    cancel.store(true, Ordering::Release);
    assert_eq!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap_err()
            .cause,
        "memory.runtime_cancelled"
    );
    assert_eq!(reader.source.read_calls.get(), 0);
}
#[test]
fn root_before_lookup_failure_retains_evidence() {
    let mut f = Fixture::new();
    f.fail_validation = 2;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup(&Selector::Identity, &mut b).unwrap_err();
    assert_eq!(e.cause, "fixture.anchor_changed");
    assert_eq!(e.stage, "before_lookup");
    assert!(!e.root.stable_guards);
    assert_eq!(e.usage, b.usage());
    assert_eq!(reader.source.read_calls.get(), 0);
}
#[test]
fn post_lookup_root_change_discards_successful_record() {
    let mut f = Fixture::new();
    f.publication();
    f.fail_validation = 3;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup(&Selector::Identity, &mut b).unwrap_err();
    assert_eq!(e.cause, "fixture.anchor_changed");
    assert_eq!(e.stage, "after_lookup");
    assert!(e.usage.read_calls > 0);
    let calls = reader.source.read_calls.get();
    assert_eq!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap_err()
            .stage,
        "poisoned"
    );
    assert_eq!(reader.source.read_calls.get(), calls);
}
#[test]
fn os_read_failure_keeps_exact_cause_and_never_retries() {
    let f = Fixture::new();
    f.fail_read.set(true);
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup(&Selector::Identity, &mut b).unwrap_err();
    assert_eq!(e.cause, "fixture.os_partial_copy:299");
    assert_eq!(e.stage, "lookup");
    assert_eq!(e.usage.read_calls, 1);
    assert_eq!(reader.source.validations, 3);
    reader.source.fail_read.set(false);
    assert!(reader.lookup(&Selector::Identity, &mut b).is_err());
    assert_eq!(reader.source.read_calls.get(), 1);
}
#[test]
fn reader_root_path_tearing_discards_observation() {
    let f = Fixture::new();
    f.tear_root.set(true);
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    assert_eq!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap_err()
            .cause,
        "mailbox.path_changed"
    );
}
#[test]
fn exhausted_shared_allocation_budget_fails_before_path_read() {
    let mut limits = RuntimeCost::defaults();
    limits.allocation_bytes = 5;
    let mut b = RuntimeBudget::new(
        limits,
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let mut reader = bound(Fixture::new(), &mut b);
    assert_eq!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap_err()
            .cause,
        "memory.runtime_budget:allocations"
    );
    assert_eq!(reader.source.read_calls.get(), 0);
}
#[test]
fn invalid_selector_cannot_reach_heap() {
    let mut b = budget();
    let mut reader = bound(Fixture::new(), &mut b);
    assert_eq!(
        reader
            .lookup(
                &Selector::Receipt {
                    nonce: "0".repeat(32)
                },
                &mut b
            )
            .unwrap_err()
            .cause,
        "mailbox.receipt_nonce"
    );
    assert_eq!(reader.source.read_calls.get(), 0);
}

#[test]
fn expired_deadline_and_invalid_release_refuse_binding() {
    let mut expired = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let e = Bound::bind(Fixture::new(), admission(), "3.1.2", &mut expired)
        .err()
        .unwrap();
    assert_eq!(e.cause, "memory.runtime_deadline");
    assert_eq!(e.usage, RuntimeCost::default());
    for release in ["", "版本"] {
        let e = Bound::bind(Fixture::new(), admission(), release, &mut budget())
            .err()
            .unwrap();
        assert_eq!(e.cause, "reader.runtime_release");
        assert_eq!(e.usage, RuntimeCost::default());
    }
}

#[test]
fn bind_guard_failure_preserves_partial_root_evidence_and_cause() {
    let mut f = Fixture::new();
    f.fail_validation = 1;
    let mut b = budget();
    let e = Bound::bind(f, admission(), "3.1.2", &mut b).err().unwrap();
    assert_eq!(e.cause, "fixture.anchor_changed");
    assert_eq!(e.stage, "bind");
    assert!(!e.root.stable_guards);
    assert_eq!(e.usage, b.usage());
}

#[test]
fn malformed_build_does_not_enter_catalog_matching() {
    for build in ["1.60.1", "1.60.1.70338.0", "1.60..70338", "1.60.1.70338x"] {
        let mut c = client();
        c.full_build = build.into();
        assert_eq!(
            RuntimeLayoutAdmission::select_from(&c, RecipeId::Forever, CATALOG)
                .err()
                .unwrap(),
            "reader.runtime_client_identity"
        );
    }
}

#[test]
fn scope_identity_rejects_missing_or_foreign_module() {
    assert_eq!(
        check_scope_identity::<u64>(None, &1).unwrap_err(),
        "reader.runtime_scope_missing"
    );
    assert_eq!(
        check_scope_identity(Some(&1), &2).unwrap_err(),
        "reader.runtime_scope_module_changed"
    );
    assert!(check_scope_identity(Some(&1), &1).is_ok());
}

#[test]
fn metadata_change_before_lookup_prevents_heap_read() {
    let mut f = Fixture::new();
    f.change_scope_at = 2;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup(&Selector::Identity, &mut b).unwrap_err();
    assert_eq!(e.cause, "reader.runtime_metadata_changed");
    assert_eq!(e.stage, "before_lookup");
    assert_eq!(reader.source.read_calls.get(), 0);
}

#[test]
fn metadata_change_after_lookup_discards_record_and_poisons() {
    let mut f = Fixture::new();
    f.publication();
    f.change_scope_at = 3;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let e = reader.lookup(&Selector::Identity, &mut b).unwrap_err();
    assert_eq!(e.cause, "reader.runtime_metadata_changed");
    assert_eq!(e.stage, "after_lookup");
    assert!(e.usage.read_calls > 0);
    assert_eq!(
        reader
            .lookup(&Selector::Identity, &mut b)
            .unwrap_err()
            .stage,
        "poisoned"
    );
}

#[test]
fn runtime_budget_observes_parent_cancel_after_issuance() {
    use lycheedev_core::control::OperationControl;
    let parent_token = Arc::new(AtomicBool::new(false));
    let parent = OperationControl::new(
        Instant::now() + Duration::from_secs(30),
        parent_token.clone(),
    );
    let child = parent.child(parent.deadline(), Arc::new(AtomicBool::new(false)));
    let mut b = runtime_budget(&child, RuntimeLimits::defaults()).unwrap();
    parent_token.store(true, Ordering::Release);
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
    let before = b.usage();
    assert_eq!(
        b.charge(RuntimeCost::work(1)).unwrap_err(),
        "memory.runtime_cancelled"
    );
    assert_eq!(b.usage(), before);
}

#[test]
fn runtime_budget_keeps_nested_ancestry_and_independent_child_cancellation() {
    use lycheedev_core::control::OperationControl;
    let top_token = Arc::new(AtomicBool::new(false));
    let top = OperationControl::new(Instant::now() + Duration::from_secs(30), top_token.clone());
    let middle = top.child(top.deadline(), Arc::new(AtomicBool::new(false)));
    let leaf_token = Arc::new(AtomicBool::new(false));
    let leaf = middle.child(middle.deadline(), leaf_token.clone());
    let b = runtime_budget(&leaf, RuntimeLimits::defaults()).unwrap();
    assert!(b.usage().allocation_bytes >= leaf.clone_allocation_bytes() as u64);
    top_token.store(true, Ordering::Release);
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
    let independent_top = OperationControl::default();
    let independent_child =
        independent_top.child(independent_top.deadline(), Arc::new(AtomicBool::new(false)));
    let b = runtime_budget(&independent_child, RuntimeLimits::defaults()).unwrap();
    independent_child
        .cancel_handle()
        .store(true, Ordering::Release);
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
    assert!(independent_top.check().is_ok());
}

#[test]
fn runtime_budget_inherits_expired_parent_deadline() {
    use lycheedev_core::control::OperationControl;
    let parent = OperationControl::new(Instant::now(), Arc::new(AtomicBool::new(false)));
    let child = parent.child(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    );
    assert_eq!(
        runtime_budget(&child, RuntimeLimits::defaults())
            .err()
            .unwrap(),
        "memory.runtime_deadline"
    );
}

#[test]
fn api_family_never_substitutes_for_exact_regional_abi_evidence() {
    let asia = client();
    assert_eq!(client::client_data_product(&asia), "forever-asia-beta");
    RuntimeLayoutAdmission::select_from(&asia, RecipeId::Forever, CATALOG).unwrap();
    let mut cn = asia.clone();
    cn.catalog_product = "wow_cn_beta".into();
    cn.channel = "cn-beta".into();
    assert_eq!(client::client_data_product(&cn), "forever-cn-beta");
    assert_eq!(
        RuntimeLayoutAdmission::select_from(&cn, RecipeId::Forever, CATALOG)
            .err()
            .unwrap(),
        "reader.runtime_layout_unsupported"
    );
    let mut legacy = asia.clone();
    legacy.catalog_product.clear();
    legacy.channel.clear();
    assert_eq!(client::client_data_product(&legacy), "forever");
    assert_eq!(
        RuntimeLayoutAdmission::select_from(&legacy, RecipeId::Forever, CATALOG)
            .err()
            .unwrap(),
        "reader.runtime_layout_unsupported"
    );
    for mut conflicting in [asia.clone(), cn] {
        conflicting.product_code = "wow_classic_beta".into();
        assert_eq!(
            RuntimeLayoutAdmission::select_from(&conflicting, RecipeId::Forever, CATALOG)
                .err()
                .unwrap(),
            "reader.runtime_client_identity"
        );
    }
}

fn formal_identity_selector() -> crate::go_mailbox_v1::Selector {
    use crate::go_mailbox_v1::{Kind, Selector, ZERO_TOKEN};
    Selector {
        kind: Kind::Identity,
        nonce: ZERO_TOKEN,
        runtime: ZERO_TOKEN,
        ticket: ZERO_TOKEN,
        body: None,
    }
}
#[test]
fn go320_formal_abi_follows_all_seven_exact_product_slots_without_old_build_allowlist() {
    for product in &client::PRODUCTS {
        let mut c = client();
        c.product = product.family.into();
        c.product_code = if product.family == "forever" {
            "wow_forever"
        } else {
            product.catalog_product
        }
        .into();
        c.catalog_product = if product.id != product.family {
            product.catalog_product
        } else {
            ""
        }
        .into();
        c.channel = product.channel.into();
        c.full_build = format!("{}.99999", product.series);
        for recipe in [
            RecipeId::Retail,
            RecipeId::Forever,
            RecipeId::ForeverContext,
        ] {
            let admitted = RuntimeLayoutAdmission::select_formal(&c, recipe).unwrap();
            assert_eq!(admitted.evidence_sha256(), FORMAL_LAYOUT_SOURCE);
            admitted.admits_layout(&admitted.layout()).unwrap();
        }
        c.product_code = "unrelated".into();
        assert!(RuntimeLayoutAdmission::select_formal(&c, RecipeId::Retail).is_err());
    }
}
#[test]
fn go320_root_selection_prefers_actual_or_upgrade_mailbox_and_rejects_ties() {
    assert_eq!(choose_observed_root(&[Ok(false), Ok(true)]).unwrap(), 1);
    assert_eq!(
        choose_observed_root(&[Err("mailbox.thread_tag".into()), Ok(false)]).unwrap(),
        1
    );
    assert_eq!(
        choose_observed_root(&[
            Ok(false),
            Err("memory.mailbox_incompatible: mailbox_schema_mismatch".into())
        ])
        .unwrap(),
        1
    );
    assert!(choose_observed_root(&[Ok(true), Ok(true)]).is_err());
    assert!(choose_observed_root(&[Ok(false), Ok(false)]).is_err());
    assert!(
        choose_observed_root(&[
            Err("mailbox.thread_tag".into()),
            Err("mailbox.globals".into())
        ])
        .unwrap_err()
        .contains("mailbox.globals")
    );
    assert!(fatal_lookup_cause("mailbox.path_changed\nwindow.reused"));
    assert!(fatal_lookup_cause(
        "memory.mailbox_incompatible: mailbox_schema_mismatch\nmemory.runtime_cancelled"
    ));
}
#[test]
fn go320_formal_accept_runs_after_outer_root_guard_and_cannot_hide_cancellation() {
    let mut f = Fixture::new();
    f.publication();
    f.fail_validation = 3;
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    let calls = Cell::new(0);
    let mut accept = |_: &Record| {
        calls.set(calls.get() + 1);
        true
    };
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), Some(&mut accept), &mut b)
            .unwrap_err()
            .cause,
        "fixture.anchor_changed"
    );
    assert_eq!(calls.get(), 0);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut b = RuntimeBudget::new(
        RuntimeLimits::defaults(),
        Instant::now() + Duration::from_secs(10),
        cancel.clone(),
    )
    .unwrap();
    let mut f = Fixture::new();
    f.publication();
    let mut reader = bound(f, &mut b);
    let mut accept = |_: &Record| {
        cancel.store(true, Ordering::Release);
        false
    };
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), Some(&mut accept), &mut b)
            .unwrap_err()
            .cause,
        "memory.runtime_cancelled"
    );
}
#[test]
fn go320_transient_named_path_can_recover_without_new_binding_or_budget() {
    let f = Fixture::new();
    f.tear_root.set(true);
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "mailbox.path_changed"
    );
    let used = b.usage();
    reader.source.tear_root.set(false);
    assert!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap()
            .is_none()
    );
    assert!(b.usage().read_calls > used.read_calls);
    assert!(reader.poisoned.is_none());
}
#[test]
fn go320_state_null_is_observation_and_does_not_decide_driver_retry_policy() {
    let mut f = Fixture::new();
    f.bytes.insert(0x1000, 0u64.to_le_bytes().to_vec());
    let mut b = budget();
    let mut reader = bound(f, &mut b);
    assert_eq!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err()
            .cause,
        "mailbox.lua_state_null"
    );
    reader.source.publication();
    assert!(
        reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap()
            .is_some()
    );
}

#[test]
fn go320_generic_forever_identity_cannot_replace_a_selected_regional_slot() {
    let mut legacy = client();
    legacy.catalog_product.clear();
    legacy.channel.clear();
    assert_eq!(
        RuntimeLayoutAdmission::select_formal(&legacy, RecipeId::ForeverContext)
            .err()
            .unwrap(),
        "reader.runtime_layout_unsupported"
    );
}
#[test]
fn go320_candidate_selection_uses_actual_named_lua_observations() {
    let mut b = budget();
    let absent = Fixture::new();
    let mut present = Fixture::new();
    present.publication();
    let mut malformed = Fixture::new();
    malformed.bytes.insert(0x1000, 0u64.to_le_bytes().to_vec());
    let observations: Vec<_> = [absent, present, malformed]
        .into_iter()
        .map(|fixture| {
            let mut reader = bound(fixture, &mut b);
            reader
                .lookup_formal(&formal_identity_selector(), None, &mut b)
                .map(|record| record.is_some())
                .map_err(|failure| failure.cause)
        })
        .collect();
    assert_eq!(choose_observed_root(&observations).unwrap(), 1);
    assert!(b.usage().read_calls > 10);
}

#[test]
fn failed_lookup_joins_independent_outer_recipe_and_metadata_changes_without_retry() {
    for fail_read in [false, true] {
        let mut f = Fixture::new();
        f.tear_root.set(!fail_read);
        f.fail_read.set(fail_read);
        f.fail_validation = 3; // bind, before lookup, mandatory error-return recheck
        f.change_scope_at = 3;
        let mut b = budget();
        let mut reader = bound(f, &mut b);
        let request = b.request_id();
        let failure = reader
            .lookup_formal(&formal_identity_selector(), None, &mut b)
            .unwrap_err();
        let original = if fail_read {
            "fixture.os_partial_copy:299"
        } else {
            "mailbox.path_changed"
        };
        assert_eq!(failure.cause.lines().next(), Some(original));
        assert!(failure.cause.contains("fixture.anchor_changed"));
        assert!(failure.cause.contains("reader.runtime_metadata_changed"));
        assert_eq!(failure.stage, "after_lookup");
        assert_eq!(reader.source.validations, 3);
        assert_eq!(reader.source.scope_checks.get(), 3);
        assert_eq!(b.request_id(), request);
        assert!(!transient_publication(&failure.cause));
        if fail_read {
            let first = failure.first_read_failure.unwrap();
            assert_eq!(first.path, "root.state");
            assert_eq!(first.source, "module");
        }
        let reads = reader.source.read_calls.get();
        assert_eq!(
            reader
                .lookup_formal(&formal_identity_selector(), None, &mut b)
                .unwrap_err()
                .stage,
            "poisoned"
        );
        assert_eq!(reader.source.read_calls.get(), reads);
    }
}

#[test]
fn failed_lookup_retains_original_read_detail_and_cancellation_at_outer_guard_boundary() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut b = RuntimeBudget::new(
        RuntimeLimits::defaults(),
        Instant::now() + Duration::from_secs(10),
        cancel.clone(),
    )
    .unwrap();
    let mut f = Fixture::new();
    f.fail_read.set(true);
    f.cancel_on_read = Some(cancel);
    let mut reader = bound(f, &mut b);
    let failure = reader
        .lookup_formal(&formal_identity_selector(), None, &mut b)
        .unwrap_err();
    assert_eq!(
        failure.cause.lines().next(),
        Some("fixture.os_partial_copy:299")
    );
    assert!(failure.cause.contains("memory.runtime_cancelled"));
    assert_eq!(failure.first_read_failure.unwrap().path, "root.state");
    assert!(reader.poisoned.is_some());
}

#[test]
fn binding_cleanup_retains_original_diagnostics_and_all_close_failures() {
    let detail = mailbox::ReadFailure {
        path: "record.wire_header".into(),
        requested_bytes: 80,
        returned_bytes: Some(7),
        source: "private",
        reason: "native_read_failed",
        guard_recheck: "verified",
        io_attempted: Some(true),
        native_error_code: Some(299),
        region: None,
    };
    let failure = RuntimeBindingFailure {
        cause: "fixture.read_failed".into(),
        stage: "lookup",
        root: Box::default(),
        usage: RuntimeCost::default(),
        first_read_failure: Some(detail.clone()),
    };
    let failure = join_close_failure(
        failure,
        Err("memory.runtime_process_close: fixture\nmemory.runtime_cancelled".into()),
    );
    assert_eq!(
        failure.cause.lines().collect::<Vec<_>>(),
        vec![
            "fixture.read_failed",
            "memory.runtime_process_close: fixture",
            "memory.runtime_cancelled"
        ]
    );
    assert_eq!(failure.first_read_failure, Some(detail));
    assert_eq!(failure.stage, "lookup");
}

#[test]
fn failed_binding_closes_its_owned_source_once_and_preserves_both_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut source = Fixture::new();
    source.fail_validation = 1;
    source.close_calls = Some(calls.clone());
    source.close_error = Some("fixture.checked_close_failed");
    source.dropped = Some(drops.clone());
    let mut b = budget();
    let failure = Bound::bind(source, admission(), "3.1.2", &mut b)
        .err()
        .unwrap();
    assert_eq!(
        failure.cause,
        "fixture.anchor_changed\nfixture.checked_close_failed"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
