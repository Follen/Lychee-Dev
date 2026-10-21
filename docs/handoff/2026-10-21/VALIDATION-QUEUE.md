# Unified validation queue: resume at cfdefd46

Current source is cfdefd46a11ec8dce470d296b424a3eaa1372b8b. Both checks and Data/CLI listing codegen passed; Core ignored-list is blocked by cache admission. Actual business cases remain zero. Resume only after the authorized narrow cache maintenance and fresh source/owner checks. This plan is not a pass report. The designated maintainer owns subsequent development and validation; coordinate with the existing queue to avoid duplicate runs. Performance remains later.

## Custody and source

Use the current verified Windows validation environment. Full Git history is required for fixed Go and earlier Rust contract blobs; retain the real new Skill anchor commit. Run only the official owned Windows runner, maximum 600 seconds per group, default stack, locked/offline and j1, no one-shot. Preserve every timeout, Cargo error, exit and descendant cleanup result. Do not retry uncertain writes or claim zero selected tests as a pass. The installed Emmy source must pass its existing pinned source identity check.

## Order

1. Native all-target compilation for Windows, Core, Data, CLI, Source and Live using the official runner. Include production default and explicit migration-oracle configurations, separately record them. Fix actual errors before runtime groups.
2. Enumerate Rust and Node test lists before execution; record exact count and selected names. Include library tests and the explicit feature-gated Data targets below, all CLI integration tests (including the two Agent replacements), Core quota/CAP/staging/owned-reader tests, and existing Source/Live lifecycle/public tests.
3. Execute correctness groups under the same controls: actual Agent fixture parity, session/engine generic and cache coverage, all 12 domain paths, CAP ownership/commit errors, real supervisor process/timeout/backpressure, Source and Live regression. Compare exact values/coverage/evidence, not old internal step counters.
4. Run Node contract/oracle/identity negative tests and current Agent client/paired/Skill declarations. Historical report checkers remain independent historical checks, never new product success. Record each required-feature skipped target explicitly.
5. Build the approved production Release artifact without migration-oracle, verify its exact source/dependencies and command inventory. Package and npm-install the matching 25-file Skill. Run pure no-home discovery, installed real Source workflow and Agent Data workflow with independently verified evidence. Explicitly prepare the three exporter/importer targets with no one-shot; do not silently build inside measured tasks.
6. Only after business correctness/coverage and resource custody pass, coordinate scheduling of the existing owned paired performance protocols. Keep twenty repetitions and no helper-cost subtraction; CSV is correctness-only. Final audit loops follow the complete correctness and performance gates.

## Explicit migration-oracle targets

- agent_native_parity
- aggregates_parity
- assets_business
- catalog_navigation_parity
- cli_local_parity
- compound_parity
- content_budget_parity
- creature_navigation_parity
- cte_parity
- db2_reader_limits_repro
- derived_parity
- effective_sql_parity
- encounter_navigation_parity
- explain_parity
- hotfix_scan_parity
- hotfix_table_parity
- image_export_business
- installed_parity
- item_geosets_parity
- item_models_parity
- item_textures_parity
- joins_parity
- local_domain_parity
- recursive_parity
- relational_parity
- resource_parity
- semantic_metadata_repro
- semantic_sql_parity
- spell_info_parity
- sql_csv_parity
- sql_explain_parity
- sql_lazy_provider_parity
- sql_resource_parity
- stream_real
- syntax_diagnostics
- topk_parity

Count: 36. This list is source-derived, not a discovery/test result.
