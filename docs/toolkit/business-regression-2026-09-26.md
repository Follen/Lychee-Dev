# wowdoc / wowdata business regression — 2026-09-26

This run audits all **55 migration cases** (19 wowdoc, 36 wowdata) through the current Toolkit. It does not invoke retired executables or import legacy user data. Full automated coverage and sampled real-input coverage are different claims. **The business matrix is not an all-pass result:** encrypted-data reads remain blocked, and several external integrations are fixture-only.

Base: `39ea9764cc279d728c13f72e279160e39be36a34`. Preparation used installed 2.0.3; follow-up queries used the working-tree candidate. These fixes are not a newly published version.

## Verified in this pass

- Retail **12.1.0.69933**, Classic **5.5.4.69934**, Titan **3.80.2.69874**: fixed source sync/index, installation identity and real ChrClasses rows. Region/locale: cn/zhCN. Definitions: `e989e99e6f5f97c57b2e138d4d28b16066b4ee9c`. Exact pins and capture IDs are in the [machine-readable evidence](business-regression-2026-09-26.json).
- Precise API query, exploratory query, symbol/path inspection, diff, reindex and addon/static matrix validation. Matrix targets are valid but each retains **2471 unresolved static references**; no runtime/taint claim follows.
- DB2 schema, rows, search, foreign-key lookup, cached next page, parameterized SQL, CSV, and a three-row JSONL stream with a legal end frame. Stream truncation is explicitly reported. Classic CDN reads used the same exact DataPin as local reads.
- Three actual DBCache files; Classic typed SpellName decoding (Chinese values) and continuation from an archived capture. Wago returned a bounded incomplete page. No current Raidbots input was supplied.
- Real JournalEncounterSection ID132: **16 sections**. ID1122 was absent and was not counted as a positive encounter result. Spell expansion is not proven by this sample.
- Community listfile: **152,780,332 bytes**, **2,255,617 entries**, no rejected lines. ID/name/text/extension queries passed. Real file626008 is `interface/icons/classicon_warrior.blp`; raw, PNG and lossless WebP exports passed, with identical decoded RGBA hashes for both image outputs.
- Isolated workspace initialization, configuration, target add/list/show/remove, cache status/verify and prune dry run. Actual user cache and installations were not removed.

## Fixes found by the regression

1. Windows skill launcher could not execute `npm.cmd` with shell disabled. It now resolves the adjacent npm package's JavaScript launcher and invokes Node directly, preserving argument boundaries and exit codes. A failing-before/passing-after test includes Chinese paths, spaces and an ampersand argument.
2. Static matrix validation treated a comma-separated TOC Interface declaration as one number. Exact-token intersection now accepts a supported member without accepting prefix matches; failing-before/passing-after tests cover both outcomes.
3. The official listfile exceeded the old64MiB budget. Asset search now accepts `--max-bytes` (default256MiB, max512MiB), preserves typed cache budget errors, and allows a bounded120-second download. The actual153MB download and all four search modes passed afterward.
4. Skill instructions and command help no longer frame the end of `live run` as the end of the Agent's task. Authorized live work continues through report retention, same-operation ACK and final `live hide` verification. Connection-only receipts are distinguished from probe operations. Explicit pause/retention requests and genuine external blockers are preserved.

## Limits and failures

- **17 sampled semantic commands returned exit3 / `container.key_unavailable`** across spell, item, creature and Retail decor workflows. This proves structured failure, not semantic business success. The full error messages are retained in the evidence JSON.
- wowexport text/binary listfiles, real VP9/Opus media, current Raidbots data, large real DB2 streams, full encounter asset sets and historical same-product multi-build reuse remain fixture-only or not_run as indicated below.
- No game input or new real-client QR lifecycle run was performed in this pass. Existing live protocol/Lua/recovery tests passed. The skill routing, implicit-invocation setting, launcher and command consistency were checked; **fresh-model automatic skill selection and adherence are not_run**, not inferred from static checks. Earlier manual client observations remain in [the separate audit](audit-2026-09-26.md).

## Validation

- `go build ./...` and `go vet ./...`: passed.
- `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 -parallel=4 ./...`: passed, including addon, protocol, parity and process suites.
- `node --test tools/*.test.mjs`: **46 passed**, none skipped.
- Skill contract: **79 commands / 185 references / 0 violations**; skill-creator quick validation and version consistency passed.
- Raw local outputs/logs: `.tmp/business-full-20260926/`. The tracked JSON retains command arguments, exits, output hashes, capture IDs, fixed identities and per-case mapping; raw outputs are not committed. Exit0 alone does not imply a complete data page.

## Case-by-case coverage

“Offline status” is preserved from [the parity ledger](../../tests/parity/coverage.json); it is not promoted by this report. Empty actual-run lists mean automated/contract evidence only.

| Case | Former workflow | Offline status | This pass |
| --- | --- | --- | --- |
| DOC-001 | --version | passed | Automated version/build provenance assertions.  |
| DOC-002 | init | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `fresh-init`, `retail-source-sync`, `classic-source-sync`, `titan-source-sync`. |
| DOC-003 | update | intentional-change | Intentional package update contract; no retired updater run.  |
| DOC-004 | clean | intentional-change | Isolated empty-cache dry run only; populated deletion covered by fixtures. Runs: `cache-prune`, `fresh-init`. |
| DOC-005 | uninstall | intentional-change | Managed remove/upgrade fixtures; no user installation removed in this regression.  |
| DOC-006 | doctor | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `doctor`. |
| DOC-007 | source list | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-list`. |
| DOC-008 | source check | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `doctor`. |
| DOC-009 | source sync | fixture-backed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `retail-source-sync`, `classic-source-sync`, `titan-source-sync`. |
| DOC-010 | index build | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `retail-source-index`, `classic-source-index`, `titan-source-index`. |
| DOC-011 | index refresh | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-reindex`. |
| DOC-012 | index status | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-list`. |
| DOC-013 | query precise | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-precise`. |
| DOC-014 | explore | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-explore`. |
| DOC-015 | inspect symbol | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-symbol`. |
| DOC-016 | inspect path | fixture-backed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-path`. |
| DOC-017 | diff | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `source-diff`. |
| DOC-018 | validate addon | fixture-backed-partial | Static valid; unresolved references remain, runtime safety not established. Runs: `source-addon`. |
| DOC-019 | validate-matrix | passed | Multi-Interface false mismatch reproduced then fixed; all three static targets valid, each has 2471 unresolved references. Runs: `source-matrix`, `source-matrix-fixed`. |
| DAT-001 | sql | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `sql`, `sql-csv`. |
| DAT-002 | hotfix query | fixture-backed-partial | DBCache raw/typed/capture pagination works; Wago bounded page is incomplete. Raidbots: fixtures only, no current external input. Runs: `classic-hotfix-dbcache`, `retail-hotfix-dbcache`, `titan-hotfix-dbcache`, `hotfix-typed`, `hotfix-capture-page`, `hotfix-wago`. |
| DAT-003 | warmup | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `classic-resolve`, `cache-status`. |
| DAT-004 | db2 schema | fixture-backed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `schema`. |
| DAT-005 | db2 rows | fixture-backed-partial | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `retail-db2`, `classic-db2`, `titan-db2`, `next-page`. |
| DAT-006 | db2 search | fixture-backed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `search`. |
| DAT-007 | db2 foreign-key | fixture-backed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `foreign-key`. |
| DAT-008 | db2 stream | fixture-backed-partial | Three records plus legal end frame, complete=false/truncated=true. Large real stream not_run. Runs: `stream`. |
| DAT-009 | spell info | passed | Blocked on encrypted table key. Runs: `retail-spell`, `classic-spell`, `titan-spell`. |
| DAT-010 | spell auras | passed | Blocked on encrypted table key. Runs: `spell-auras`. |
| DAT-011 | spell summons | passed | Blocked on encrypted table key. Runs: `spell-summons`. |
| DAT-012 | encounter get | passed | ID1122 absent; selected ID132 from real DB2 and read 16 sections. Does not prove spell expansion. Runs: `encounter`, `encounter-populated`. |
| DAT-013 | encounter export | intentional-change | Split export contract. Icon export proved, encounter asset-set integration not_run. Runs: `asset-raw`. |
| DAT-014 | file lookup/search/extension | passed | Real community CSV 152780332 bytes; four lookup modes pass. wowexport text/binary remain fixture-only. Runs: `asset-search-retry`, `asset-search-query`, `asset-search-extension`, `asset-search-name`. |
| DAT-015 | file get/export | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `asset-raw`. |
| DAT-016 | file exists/encoding | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `asset-inspect`. |
| DAT-017 | icon export | fixture-backed-partial | Real BLP2 64x64; PNG and WebP decoded-pixel hashes match. Runs: `asset-png`, `asset-webp`. |
| DAT-018 | casc info | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `classic-resolve`, `cache-status`. |
| DAT-019 | casc products | fixture-backed-partial | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `target-list`, `target-available`. |
| DAT-020 | casc diagnose | intentional-change | Read-only doctor and isolated empty-cache verification; corruption checks are fixture-backed. Runs: `doctor`, `cache-verify`. |
| DAT-021 | item get/models/geosets/textures | passed | All sampled get/models/geosets/textures blocked on encrypted table key. Runs: `retail-item`, `classic-item`, `titan-item`, `item-models`, `item-geosets`, `item-textures`. |
| DAT-022 | creature display/model | passed | Display/model samples blocked on encrypted table key. Runs: `retail-creature`, `classic-creature`, `titan-creature`, `creature-model`. |
| DAT-023 | decor list/get | passed | Retail list/get blocked on encrypted table key. Runs: `retail-decor`, `decor-get`. |
| DAT-024 | video demux | fixture-backed | VP9/Opus demux fixtures pass; no real supported media supplied.  |
| DAT-025 | profile list/show/set/remove | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `target-add`, `target-list`, `target-show`, `target-remove`. |
| DAT-026 | cache status | passed | Isolated fresh cache. Runs: `cache-status`. |
| DAT-027 | cache verify | passed | Isolated fresh cache; corrupt/populated cases use fixtures. Runs: `cache-verify`. |
| DAT-028 | cache prune | passed | Dry run only on isolated fresh cache; populated deletion uses fixtures. Runs: `cache-prune`. |
| DAT-029 | cache clear | intentional-change | Explicit pruning replaces broad clearing; no user cache removed. Runs: `cache-prune`. |
| DAT-030 | cache config | intentional-change | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `config-set`, `config-show`. |
| DAT-031 | doctor | passed | Current command executed against the fixed source/data or isolated workspace; see run evidence. Runs: `doctor`. |
| DAT-032 | update | intentional-change | Retired updater intentionally absent.  |
| DAT-033 | uninstall | intentional-change | Explicit managed removal covered by fixtures; no actual user removal.  |
| DAT-034 | golden capture/compare | intentional-change | Command contract and existing golden/parity tests; retired golden generator not run.  |
| DAT-035 | remote current-build discovery | fixture-backed-partial | CN availability and exact Classic pin CDN read, no latest fallback. Runs: `target-available`, `cdn-db2`. |
| DAT-036 | multi-build/cache reuse | fixture-backed-partial | Three current builds and cached page; historical same-product build reuse remains fixture-only. Runs: `retail-resolve`, `classic-resolve`, `titan-resolve`, `next-page`. |
