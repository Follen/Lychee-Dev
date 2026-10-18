# Stage 2 platform-test reachability inventory

Baseline: `05649a0bf3ec923f6aec984b9a5f29fa3b792e7c` (the first reviewed Stage 2 repair batch). Inventory completed 2026-10-18 UTC. Repository read-only; no Cargo, Node tests, production audit, user-computer work, or repository writes were performed. No runtime success is implied.

## Result and convergence ledger

Three supported-Windows coverage gaps remain in the fixed baseline. Two are newly identified beyond the known Delivery integration gap. One additional non-Windows test is graph-dead but does not represent missing supported-product behavior.

| ID / disposition | Exact baseline site | Reachability / consumer evidence | Required convergence |
|---|---|---|---|
| P1 replace; second writer already working | `crates/lycheedev-delivery/tests/slot_maintenance.rs:454`, `linked_slot_directory_or_static_member_is_rejected_without_following_it` | Whole test is `cfg(unix)`. It contains separate managed-directory redirection and valid static-file redirection negatives. Delivery → Core → Windows is unconditional; therefore Windows excludes this test and Unix cannot build the normal graph. This is dead coverage, not merely a machine unavailable. | Active Windows directory-junction test with valid original slot bytes and specific rejection; independent real file-symlink case with explicit privilege prerequisite. Do not replace the file case with a directory at the leaf. Candidate now exists in the shared worktree, separately owned and not part of this baseline or an executed result. |
| P2 replace silent pass | `crates/lycheedev-core/src/installation_selection.rs:718–733`, `root_discovery_does_not_follow_foreign_directory_symlink` | `symlink_dir` failure 1314 returns `()` before either `discover(...).candidates.is_empty()` or explicit-alias `select(...).product == titan`. Ordinary Windows test can report success with neither consumer assertion executed. | Use a verified Windows directory junction to a valid foreign client, preserving both discovery rejection and explicit-alias selection assertions. If a symbolic-link-specific tag distinction matters, retain that as a separate explicit privileged case. Setup failures must fail or be explicitly ignored, never return success. |
| P3 split omitted assertion | `tests/rust-performance-paired/source-candidates.test.mjs:155–176`, `historical witness collector hashes actual bounded files and rejects changed bytes sizes missing files and symlinks` | Only the last file-link assertion is inside `if (process.platform !== 'win32')`. Windows passes the surrounding test while never testing the claimed link rejection. The preceding directory replacement tests type rejection, not file-link rejection. Pure Node harness is runnable on Unix, so it is not graph-dead, but the supported host branch is omitted. | Keep normal bounded-file/size/missing/type assertions in their own test; make the valid-target file-link rejection a distinct Windows-capable test with narrow creation-only EPERM/EACCES explicit skip. Authenticate fixture before mutation and require the intended link rejection reason. |
| P4 explicit unsupported-host gap; no product repair established | `crates/lycheedev-data/src/resource/adaptive/tests.rs:473–488`, `development_platform_preserves_fixed_defaults_after_real_admission` | `cfg(not(windows))`, same unconditional Core → Windows graph; no independent harness found for this file. The body asserts the unsupported-host `fixed` budget fallback. Windows counterpart `native_query_activates_after_real_admission` at 457 executes the supported branch. | Do not count this as covered, runnable pending hardware, or a Windows defect. Label dormant unsupported-host coverage. Any removal/relocation is separate scope; no production fallback audit was performed. |

Baseline byte identities (SHA-256):
- Core installation selection: `3cb63d5de67df1f47dafb8216ff874597845c2c11e6520ea5851940ba637f0b9`
- Delivery slot maintenance: `8d854ad00aaa2035b478ac0316e151d6aec483ee969fe039ea11264d84a1e488`
- Data adaptive tests: `82726b108341e86007c1604e4d0c3b3c16499f5b925d3705f87f81554ee61cea`
- Node source candidates: `f6f590de7c467d5c2db9e29f0eba2b8f01153e54f3730387b612ee0f6e5cbbf2`

## Scan coverage and dependency evidence

The stable tree has 1,069 tracked `.rs` files and 342 tracked `.mjs/.js/.cjs/.ts/.tsx` files. Full tracked-file sweeps covered `cfg`, `cfg!`, `cfg_attr`, multiline target predicates, Unix/Windows/target OS/family/architecture/pointer width, Node platform and architecture predicates, `t.skip`/`test.skip`/skip options, diagnostics and test-adjacent early returns. Locations include all owned crates, `packages/npm`, `tools`, `tests` (including historical/reference/pure harness trees), `protocol`, `third_party`, and `vendor`. Matching test code was read in context; ordinary production returns were not treated as test defects. This is selector/omission inventory, not an assertion-quality or line-coverage audit of every test.

Graph evidence: `crates/lycheedev-core/Cargo.toml:17` unconditionally depends on `lycheedev-windows`; Delivery/Data depend unconditionally on Core. Windows `src/lib.rs` unconditionally exports its modules; `src/filesystem.rs:1` and `src/process.rs:16` import `std::os::windows` without a host gate. No Unix product shim or alternate harness for P1/P4 was found. The pure standalone harness below is a different, explicitly bounded graph.

The only owned Rust whole-test non-Windows target omissions found are P1 and P4. `slot_publication_tests.rs:1596/1605` and `release/checked_close_tests.rs:67` contain non-Windows helper branches, but the real deny-write/delete assertions execute on Windows; they are not Windows coverage gaps. `metadata_format/prepared/tests.rs:241` is 64-bit only and therefore included on the supported Windows amd64 target. `cfg!(windows)` expectation branches in Data adaptive consumers choose Windows expectations, without skipping the test. The default Source feature is `emmy-prototype`; its feature gates are not default Windows omissions.

## Already repaired in the first batch: keep, verify later

- Core `coordination/tests.rs:111`, `rejected_registry_redirect_explicitly_closes_acquired_maintenance`: active Windows junction, actual maintenance conflict/checked close. The earlier Unix-only problem is no longer present in the fixed baseline.
- Live `go_mailbox_v1/admission_tests.rs:812` and `:842`: active directory-junction redirection cases. `go_mailbox_v1_admission_redirected_claim_file_fails` at `:875` is a distinct, explicitly ignored real file-symlink test. On explicit execution, missing privilege panics with a clear prerequisite rather than reporting success.
- Node `read-fact-delta`, `source-pack-scenarios`, and Skill `runner` use real directory junctions or separately reported file-link skips. Their already repaired predicates should not be rewritten again.

## Explicit Node environment gaps: keep separate from pass counts

These are visible skips, not silent passes. Missing evidence remains missing until the relevant test runs on the exact candidate.

| Path / exact test | Gate / proposed disposition |
|---|---|
| `tests/rust-parity/oracle.test.mjs:114`, `filesystem inventory and bounded read reject actual file symlinks` | Creation-only Windows EPERM/EACCES → `t.skip`; keep. Separate active Windows junction-ancestor test at 130 preserves ordinary Windows coverage. |
| `tests/rust-performance-paired/read-fact-delta.test.mjs:349`, `shared Rust reader rejects a file symlink to an authenticated cache file` | Helper at 318 permits creation-only Windows EPERM/EACCES; dedicated helper negative tests reject other errors and fake nonlinks. Keep. |
| `tests/rust-performance-paired/source-pack-scenarios.test.mjs:552`, `Rust custody refuses a pack file symlink before copying` | Same narrow helper at 23; keep explicit file privilege gap. |
| Same file `:624`, `Rust custody refuses a preexisting dangling output file symlink before admission` | Same narrow helper; retain distinct dangling-file boundary. Directory-junction output test is separate and not equivalent. |
| `tests/rust-source-engine/fixed-emmy-oracle/current-contract.test.mjs:196`, `symlink evidence cannot bypass a seal or redirect reads` | Creation EPERM → explicit `t.skip`; genuine target bytes and intended `/symlink/` rejection. Keep as visible environment gap. Optional consistency cleanup can restrict EPERM to Windows and include EACCES; not a newly found silent pass. |
| `tests/rust-parity/source-project-selection.test.mjs:28`, `optional exact Go323 source review confirms project and numeric-error ordering` | Explicit missing `LYCHEEDEV_GO323_CHECKOUT` skip; fixed commit and source hashes required when supplied. Keep static-review scope, not Rust execution evidence. |
| `tests/rust-performance-paired/data-runner.test.mjs:20`, `Data phase contract matches the actual recorder in the source checkout` | Explicit skip if the source file is absent in a staged runner-only package. File exists in full baseline. Keep; full-checkout runs must not count an absent-source skip as passing production contract evidence. |
| `tools/release.test.mjs:51`, pinned LuaLS staging test | Explicit missing `LYCHEEDEV_LUALS_ARCHIVE` skip; preserve historical bundled-runtime prerequisite rather than silently claiming coverage. |

No diagnostic-plus-return silent skip remains in the inspected Node test matches beyond P3's conditional omission. Diagnostics for retaining failed fixture/evidence are teardown reporting, not skipped assertion bodies.

## Legitimate platform-specific and cross-platform harnesses

- `tests/rust-source-native-acceptance/session-ipc.test.mjs:85`, `non-Windows entry fails before any mock could be reported as acceptance`: explicitly skipped on Windows because it tests the unsupported-host rejection of `runSessionGroup`. Valid negative readiness test, not Windows IPC coverage and not a missing Windows branch. Windows native entry assertions in the real runners fail closed instead of simulating acceptance.
- `tools/rust-local-owned-process.mjs:22` explicitly requires Linux. Its seven tests in `tools/rust-local-owned-process.test.mjs:18,23,26,30,34,38,43` exercise real bubblewrap/PID namespaces, not Windows. The two tests in `tools/rust-local-pure.test.mjs:36,81` invoke that same Linux runner for cache-custody checks. Keep in the documented Linux-only harness; do not include them blindly in a Windows test glob or count them as Windows-native proof.
- `tests/rust-local-pure/Cargo.toml` has its own `[workspace]` and selected unchanged source-module paths plus the actual binding-contracts integration target. Its controlled copied-module checks are legitimate bounded cross-platform/pure evidence; they do not make Core/Data/Delivery's normal graph cross-platform and do not cover P1/P4.
- npm launcher tests explicitly inject unsupported platform/architecture combinations and assert rejection; these run on the host and are valid cross-platform contract tests. Platform-dependent `.exe`, null-device, Python-command or junction selection is fixture adaptation, not skipped coverage.
- Keep Windows-positive Rust gates in Core storage/durability/data admission, Data TLS/native budget, Delivery close/combined-addon, Live admission, Source RPC/runtime locks, CLI native-session/XML and Windows fixtures. The supported host includes them. Source `luals.rs:2002–2012` gates its RPC budget/document lifecycle/native quota/runtime-lock modules to Windows, correctly matching their native dependencies.
- Keep Node Windows-positive gates in `tools/release.test.mjs`, channel readmission/recovery tests, owned-memory tests, owned-process/atomic-job tests, Skill launcher and isolated update tests, plus the pinned three-way-copy owned-process tests. They use actual Windows filesystem/process/PowerShell facilities. Linux skips are not missing Windows coverage. Pure model tests alongside them do not replace the native tests.
- Vendored native-tls Apple/OpenSSL backend gates and schannel's Windows crate gate are upstream platform partitioning, not owned Windows omissions. Vendor `invalid_protocol` is explicitly ignored for unstable external-server behavior; record separately and do not edit pinned third-party tests to inflate coverage.

## Early returns that are legitimate subprocess plumbing

Readiness/terminal assertions in parent tests make these child bodies meaningful even though ordinary discovery invokes the helper with no role environment. Keep; do not count the helper's default no-op as independent behavior coverage, and do not turn it into a failure that breaks ordinary test discovery.

- Core `cache/source_facts.rs:1295` `streamed_publication_process_helper` is spawned by `ProcessOwner::start`; the parent checks ready markers, exact exit, publication bytes and old open reader.
- Core `cache/source_facts/atomic_publication_tests.rs:499` `facts_publication_process_helper` is spawned by `ChildOwner`; parent verifies readiness, exit and concurrent publication.
- Core `workspace/data_resources/tests.rs:295` `data_resource_helper_process` is spawned by `cross_process_measurement_and_killed_owner_release_actual_kernel_locks`, which checks readiness, actual conflict and post-kill lock acquisition.
- Core `process.rs:169` `helper` uses a no-op default match arm; parent selects block/output roles and checks deadline/output overflow with reaped child and completed readers.
- Data `hotfix/remote/wago/http_tests.rs:659` `cgi_child_case` is invoked by the isolated CGI parent with explicit env and exact test name; parent requires success before bounded timeout.
- Data `local_query/query_csv.rs:766` `sql_csv_cross_workspace_process_helper` is spawned by the parent cross-workspace/process test, which requires the conflict/readiness/result evidence.
- Live admission `go_mailbox_v1_admission_process_helper` at `:921`, durable journal `go_mailbox_v1_durable_process_helper` at `:826`, and connection `fresh_process_history_helper` at `:760` have concrete parent spawn/exit/assertion consumers. Crash helpers require exit 91, not normal success.
- Source `process.rs:412` `helper_process` is role-driven by process tests. `tests/native_quota_peer.rs` and Windows `tests/guarded_spawn_fixture.rs` are `harness = false` standalone helpers; no-role exits are not skipped Rust test functions. Windows `tests/support/owned_target.rs:25` is an explicit owned-child entry.
- Source fixed-oracle trace returns are capture/reference-mode and destructor-unwind behavior. Test callbacks, mock stream writers, loop completion, cancellation triggers, fixture-retention Drop methods, native XML expected-negative cases, and partial-string early negative branches return after their intended assertions. They are not missing-platform cases.

## Explicit Rust ignored-test appendix

There are 74 concrete owned tests under explicit `#[ignore]` in this baseline: 59 attribute locations, including one macro expanding to 16 XML tests. This is an inventory of default omission, not 74 bugs, not proof they have never run, and not a new aggregate test-result count. Three are intentionally parent-invoked peers (`trace_row_failure_peer`, `native_owner_fixture`, `lifetime_child_fixture`); remaining rows declare external fixtures, authentic runtime/input, controlled pressure/performance, or privilege prerequisites. Keep these distinctions in execution accounting. Historical LuaLS tests remain historical behavior/reference evidence until a separate consumer audit decides otherwise.

The complete exact path/name/reason list follows. Vendor's one explicit ignore is listed last and excluded from the owned count. Newly edited Delivery `linked_static_slot_member_is_rejected_without_following_it` is not included because it is outside the fixed baseline.

- `crates/lycheedev-cli/tests/source_flow_public.rs:41` — `pinned_context_flow_returns_common_proofs_and_committed_capture`: requires a verified release root with bundled Lua 5.1; invokes the real CLI and owned native worker.

- `crates/lycheedev-cli/tests/source_native_session_public.rs:161` — `public_native_session_reuses_exact_snapshot_and_retires_on_close`: requires verified release and whole-controller owned runner; real multi-CLI native session.

- `crates/lycheedev-cli/tests/source_native_xml_public.rs:25` — `default_xml_research_keeps_manifest_binding_original_ranges_and_cache_identity`: requires verified release and owned runner; real default CLI XML refs/context and cache reuse.

- `crates/lycheedev-data/src/local_query.rs:1258` — `loaded_public_missing_key_does_not_replay_metadata_chain`: actual local encrypted CASC fixture and authenticated pinned public-key document.

- `crates/lycheedev-data/tests/assets_business.rs:200` — `authenticated_local_casc_raw_export_captures_exact_bytes_and_preserves_conflicting_output`: explicit genuine synthetic CASC fixture required; no game installation.

- `crates/lycheedev-data/tests/catalog_navigation_parity.rs:324` — `prepare_catalog_public_fixture`: explicit fresh retained fixture for independent frozen-Go public differential.

- `crates/lycheedev-data/tests/cli_local_parity.rs:85` — `real_definition_transport_and_revision_observation`: explicit task-owned workspace and real upstream HTTPS required.

- `crates/lycheedev-data/tests/cli_local_parity.rs:178` — `real_pinned_public_sql_and_asset_dispatch`: explicit read-only actual client input and task-owned proof workspace required.

- `crates/lycheedev-data/tests/cli_local_parity.rs:277` — `real_db2_record_page_schema_search_foreign_key_profiles`: fixed real local CASC/DBD inputs and isolated DB2 proof home required.

- `crates/lycheedev-data/tests/content_budget_parity.rs:175` — `prepare_content_budget_public_fixture`: prepare a shared real CASC fixture for explicit frozen-Go/Rust public cold/warm boundary calls.

- `crates/lycheedev-data/tests/content_budget_parity.rs:236` — `prepare_compressed_content_public_fixture`: prepare shared compressed CASC input for explicit public Go/Rust observation.

- `crates/lycheedev-data/tests/content_budget_parity.rs:382` — `prepared_compressed_content_inspect_and_raw_export`: run inspect/raw against an explicitly prepared real large-content CASC fixture.

- `crates/lycheedev-data/tests/creature_navigation_parity.rs:268` — `prepare_creature_public_fixture`: explicit fresh retained fixture for independent frozen-Go public differential.

- `crates/lycheedev-data/tests/creature_navigation_parity.rs:922` — `prepare_creature_public_fixtures`: prepare all authenticated creature variants for frozen-Go public comparison.

- `crates/lycheedev-data/tests/encounter_navigation_parity.rs:430` — `prepare_encounter_public_fixtures`: explicit fresh retained fixtures for independent frozen-Go public differential.

- `crates/lycheedev-data/tests/image_export_business.rs:158` — `prepare_image_export_public_fixture`: prepare authenticated BLP inputs for actual public Go/Rust image export.

- `crates/lycheedev-data/tests/image_export_business.rs:207` — `prepare_large_image_export_public_fixture`: prepare retained large BLP inputs for owned Go323/Rust public paired measurements.

- `crates/lycheedev-data/tests/installed_parity.rs:417` — `real_local_authenticated_encoding_root`: requires explicitly named read-only actual client and report directory.

- `crates/lycheedev-data/tests/item_geosets_parity.rs:181` — `prepare_item_geosets_public_fixtures`: prepare authenticated fixtures for frozen Go/Rust public item-geosets comparison.

- `crates/lycheedev-data/tests/item_models_parity.rs:196` — `prepare_item_models_public_fixtures`: prepare authenticated fixtures for frozen Go/Rust public item-models comparison.

- `crates/lycheedev-data/tests/item_textures_parity.rs:195` — `prepare_item_textures_public_fixtures`: prepare authenticated fixtures for frozen Go/Rust public item-textures comparison.

- `crates/lycheedev-data/tests/local_domain_parity.rs:341` — `prepare_authenticated_missing_key_fixture`: prepare actual encrypted CASC fixture and fixed authenticated public-key document.

- `crates/lycheedev-data/tests/skill_data_fixture.rs:11` — `prepare_skill_partial_fixture`: explicit active Skill owner and fresh caller root required.

- `crates/lycheedev-data/tests/spell_info_parity.rs:174` — `prepare_spell_info_public_fixtures`: prepare retained authentic inputs for frozen-Go/Rust public spell-info differential.

- `crates/lycheedev-data/tests/stream_real.rs:40` — `real_typed_stream_complete_filtered_projected_error_and_cancel`: fixed real CASC/DBD inputs and isolated existing proof home required.

- `crates/lycheedev-data/tests/wdc_parity.rs:698` — `local_wdc_same_work_benchmark`: explicit local same-work performance experiment; not a functional acceptance skip.

- `crates/lycheedev-live/src/go_mailbox_v1/admission_tests.rs:874` — `go_mailbox_v1_admission_redirected_claim_file_fails`: requires Windows file-symlink privilege or Developer Mode; run explicitly with --ignored.

- `crates/lycheedev-live/src/go_mailbox_v1/connection/offline_performance_tests.rs:554` — `offline_driver_journal_performance`: opt-in owned offline performance runner; not ordinary lib tests.

- `crates/lycheedev-source/src/native_research.rs:1033` — `fixed_retail_family_loads_use_original_toc_and_complete_matching_program`: explicit fixed Retail home/pin; original FrameXMLUtil TOC/XML closure and CAS.

- `crates/lycheedev-source/src/native_research.rs:1103` — `full_retail_reply_keeps_all_maps_with_identical_bytes`: explicit existing fixed Retail material; extract and measure complete maps without preparing Emmy or launching a worker.

- `crates/lycheedev-source/src/rpc_native_quota_tests.rs:347` — `actual_child_commit_partition_refuses_native_allocations_and_reaps`: exclusive ~1.75GiB actual native commit; sealed helper and fresh report required.

- `crates/lycheedev-source/src/rpc_native_quota_tests.rs:353` — `post_attach_descendant_shares_actual_child_partition_and_reaps`: exclusive ~1.75GiB actual native commit; sealed helper and fresh report required.

- `crates/lycheedev-source/tests/catalog_native_real.rs:7` — `fixed_script_object_owners_and_callbacktype_reach_native_checker`: explicit fixed Blizzard ClientPin and bundled Lua51 compiler.

- `crates/lycheedev-source/tests/environment_real.rs:8` — `fixed_blizzard_catalog_real_luals_semantics`: requires an explicit fixed source home/pin and authenticated LuaLS ZIP.

- `crates/lycheedev-source/tests/fixed_oracle_trace_storage.rs:205` — `trace_row_failure_peer`: spawned by the normal parent test to inspect its exact failing exit.

- `crates/lycheedev-source/tests/loads_real.rs:6` — `authenticated_warm_index_loads_match_source_hash_and_reject_index_tamper`: explicit authenticated source home/pin; full XML/TOC subset reader.

- `crates/lycheedev-source/tests/luac_inline_real.rs:42` — `public_validate_inline_51_errors_original_lines_and_legacy_cycle_order`: explicit bundled luac real process and Git/TOC validation regression.

- `crates/lycheedev-source/tests/luac_ordered_real.rs:8` — `actual_ordered_luac_preserves_parent_and_reaps_compile_children`: requires authenticated bundled luac dev stage; run explicitly in controlled Windows CI.

- `crates/lycheedev-source/tests/luac_real.rs:9` — `actual_51_goto_identifier_structure`: real bundled Lua 5.1 identifier grammar differential.

- `crates/lycheedev-source/tests/luac_real.rs:74` — `actual_51_syntax_unicode_early_error_and_no_bytecode`: requires authenticated bundled luac dev stage; explicit real process regression.

- `crates/lycheedev-source/tests/luals_real.rs:10` — `actual_pinned_check_and_alias_navigation`: requires exact pinned LuaLS ZIP; explicit real process regression.

- `crates/lycheedev-source/tests/matrix_four_real.rs:22` — `four_distinct_client_pins_default_resolver_actual_catalog_luac_luals`: explicit four authenticated source observations and actual bundled tools; default resolver may fetch exact commits.

- `crates/lycheedev-source/tests/matrix_real.rs:6` — `two_target_actual_luac_luals_keeps_independent_failure_and_matrix_capture`: explicit fixed Retail ClientPin and authenticated real bundled tools; two TOC targets, not multiple products.

- `crates/lycheedev-source/tests/native_checker_families_real.rs:7` — `selected_value_call_families_use_native_diagnostics_and_honest_coverage`: explicit fixed Retail ClientPin and bundled Lua51 compiler.

- `crates/lycheedev-source/tests/native_nominal_real.rs:7` — `duration_nominal_methods_follow_actual_values_and_mutations`: explicit fixed Retail ClientPin and bundled Lua51 compiler.

- `crates/lycheedev-source/tests/native_return_count_real.rs:7` — `return_count_family_uses_selected_native_slots_and_source_control_flow`: explicit fixed Retail ClientPin and bundled Lua51 compiler.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_ordered_copy_versions`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_repeated_include_occurrences`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_inline_script_load_order`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_invalidated_mixin_source`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_copied_method_survives_source_overwrite`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_self_survives_global_rebind`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_instance_field_mutation`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_virtual_distinct_instances`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_intrinsic_definition_instances`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_anonymous_instances`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_function_attribute_entity`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_inline_self`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_parent_key_identity`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_empty_registration`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_method_overwritten_noncallable`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/native_xml_real.rs:14` — `xml_native_mixin_not_lua_alias`: explicit fixed ClientPin, bundled Lua51 compiler, native XML/checker/query integration.

- `crates/lycheedev-source/tests/refs_context.rs:116` — `actual_incoming_hover_plus_128_outgoing_calls_share_one_initialized_workspace`: requires exact pinned LuaLS ZIP; 130 real semantic queries across two batches.

- `crates/lycheedev-source/tests/refs_context.rs:422` — `actual_full_source_semantic_relations_preserve_candidates_and_client_gaps`: requires exact pinned LuaLS ZIP; real full-source research regression.

- `crates/lycheedev-source/tests/refs_context/project_selection.rs:375` — `project_research_actual_cli_static_and_native_use_project_selection_and_reselect_identity`: requires exact packaged CLI and release; exercises the real native worker process.

- `crates/lycheedev-source/tests/semantic_validate_real.rs:6` — `public_dispatch_fixed_catalog_luac_native_and_inline_semantic_line_map`: explicit fixed ClientPin, bundled luac and native descriptor checker.

- `crates/lycheedev-source/tests/session_real.rs:13` — `actual_retained_worker_reuses_pid_and_retires_on_cancel_or_new_input`: requires exact pinned LuaLS ZIP; real retained worker and retirement proof.

- `crates/lycheedev-source/tests/source_session_real.rs:10` — `actual_complete_source_workspace_navigation_holds_and_releases_selected_lease`: requires fixed Source pin and exact LuaLS ZIP; authenticated complete Lua workspace.

- `crates/lycheedev-source/tests/source_session_repeated_real.rs:14` — `actual_two_full_workspace_batches_under_aggregate_commit_limit`: exact pinned full Source workspace and LuaLS ZIP; diagnostic two-request 2GiB job.

- `crates/lycheedev-source/tests/validate_contract_real.rs:116` — `single_and_matrix_keep_distinct_go_contracts_and_warm_index_gaps`: explicit verified bundled luac, local Git and persisted dual-entry regression.

- `crates/lycheedev-windows/src/guarded_spawn_tests.rs:318` — `real_child_cannot_run_before_resume_and_stdio_descendant_are_owned`: requires sealed standalone helper and explicit evidence root.

- `crates/lycheedev-windows/src/guarded_spawn_tests.rs:479` — `cancelled_or_injected_prepare_failure_never_resumes_and_reaps_original_child`: requires sealed standalone helper and explicit evidence root.

- `crates/lycheedev-windows/tests/native_source_retirement.rs:14` — `native_owner_fixture`: child process for exact native Source owner retirement.

- `crates/lycheedev-windows/tests/process_lifetime.rs:17` — `lifetime_child_fixture`: owned stdin-controlled child; invoked only by its parent fixture.

- `vendor/schannel-0.1.29/src/test.rs:75` — `invalid_protocol`: unspecified; source comment says upstream Google SSLv3 behavior is inconsistent.

## Subsequent reviewed repairs (runtime pending)

The three baseline gaps P1–P3 now have independently reviewed test-only repairs. P1 separates the valid managed-directory junction from a genuine static-file symlink with explicit privilege prerequisite; Windows default active integration tests increase17→18 and one privileged test remains ignored. P2 uses a verified junction, requires both empty discovery candidates and empty issues, preserves explicit alias selection and foreign bytes; its10 test names remain. P3 keeps generic file checks unconditional and separates a valid-target exact reparse rejection, with only Windows file-symlink creation EPERM/EACCES explicitly skipped. Its affected Linux suite passed34/0/0. Rust/Windows execution is NOT_RUN for these revisions; static syntax and independent review do not prove runtime behavior.

P4 remains explicitly dormant unsupported-host coverage. No production platform shim or broad deletion was introduced. The fixed-baseline ignored inventory above must not be presented as the revised executed denominator.
