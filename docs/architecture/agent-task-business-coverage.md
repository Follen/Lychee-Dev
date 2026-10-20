# Typed Agent business coverage, 2026-10-20

This is an implementation/validation matrix, not a completion declaration. All
new Rust witnesses below are NOT_RUN until the official Windows queue runs the
exact integrated candidate. Old routes remain available pending proven business
coverage; none is called as a hidden fallback by these tasks.

| Existing business use | New interface | Actual coverage / remaining work |
| --- | --- | --- |
| Known table schema / fields | `fields` (or `discover`) with explicit TableRequest | Owned DatasetInfo from the same Engine; exact schema and provenance, not a second decoder. Fixture declared. |
| Table directory before knowing a name | `catalog` | Engine-owned fixed definition-manifest directory; complete applies only to manifest enumeration. Availability is not_checked, not a promise every table is installed/readable. Requires the core catalog delta. |
| DB2 records by ID | `business` / `batch_ids` | Typed Lookup; exact u32 IDs, duplicate inputs do not duplicate source rows; missing IDs yield no invented rows. Source coverage still governs absence. Fixture declared. |
| DB2 foreign-key matches | `business` / `related` | Typed Filter/In over explicit field and IDs, no coercive string matching. Unknown fields fail rather than return an empty success. Fixture declared. |
| Filtering / projection / aggregation / joins | `query` with typed plan | Existing common Engine, no SQL prerequisite. Business-specific derived presentation not automatically equivalent. |
| Item base and localized records | `business` / navigate `item_records` | Separate named Item and ItemSparse evidence components. No invented fallback row, slot-name presentation or legacy `item get` report parity claim. Fixture declared. |
| Spell effects and aura/summon source evidence | navigate `spell_effects` | Full exact SpellID relation rows; legacy aura classification, summon array interpretation, recursive trigger/name reference closure and `spell info` report remain to migrate and verify. |
| Decor record evidence | navigate `decor_records` | HouseDecor ID records. Legacy list order/limit/extra-row coverage and all get selectors remain separate migration requirements. |
| Creature display and geoset evidence | navigate `creature_displays` | Named display lookup and exact CreatureDisplayInfoID relation. Model traversal, texture arrays and wrapped geoset presentation remain to migrate. |
| Item models / geosets / textures | typed query primitives only | Domain joins, selector rules, array handling and report parity not yet implemented as a dedicated task. |
| Encounter graph | typed query primitives only | Journal/encounter traversal, source selection and partial-relation behavior not yet implemented as a dedicated task. |
| Existing SQL entry | no task SQL variant | Existing route is not unified yet. Do not describe typed-only Agent as a completed migration of every old consumer. |
| CAP evidence catalog | separate owned CAP workstream | Result handles are not CAP IDs. This module does not invent persistence or replace CAP retention. |

## Publication and evidence semantics

A business bundle executes all component typed plans against the same Engine and
its immutable source binding. It retains each ResultOwner and guarded provenance
until publication. Each component gets a separate result handle, complete flag
and provenance; there is deliberately no synthetic bundle-wide `complete: true`.

A source, field, query or handle-admission failure returns an error without
publishing a partial set of handles. Already inserted, unpublished handles are
rolled back. Handle TTL starts after all component queries are ready. Results
are not silently sliced to fit limits; budget exhaustion is an error. No automatic
replay or old-executor fallback occurs.

Missing rows in incomplete source coverage cannot establish absence. Navigation
components are raw evidence records, not promises that a recursive business graph
has been traversed. Clients must inspect each component's coverage/provenance and
retain the needed pages before releasing handles or closing the session.

## Native regression queue

Run the exact candidate's Data library tests including:
- fields_and_batch_tasks_use_real_pinned_source
- item_navigation_publishes_separate_guarded_components_without_invented_rows
- related_task_filters_exact_foreign_ids_and_rejects_missing_field
- multi_component_capacity_failure_rolls_back_unpublished_handles
- catalog_is_pinned_manifest_inventory_not_table_readability
- second_component_source_failure_preserves_preexisting_handle
- task-template wire rejection and 16384-ID boundary unit tests

Also rerun existing Agent session/transport and CLI supervisor tests. Syntax
formatting is not a substitute for type checking, business parity or runtime
ownership/cancellation evidence.

Still pending dedicated evidence: effective multi-component histories, partial-empty source absence handling, TTL under delayed execution, shared-budget refund/peak accounting, and old presentation/recursive closure parity. Existing guards and exact pin use do not by themselves prove those runtime behaviors.

All components share the request OperationControl deadline and the Engine session
retained-memory admission. Engine query_work is currently admitted per component,
not one combined work counter for a business bundle. The present templates have
at most two components. This is an explicit bound, not a claim of whole-request
work accounting or a reason to raise the session memory limit.
