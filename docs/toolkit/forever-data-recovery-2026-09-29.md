# Forever static-data recovery — 2026-09-29

Working-tree repair, not an npm release. No game input, installation changes,
region/locale substitutions or project-lock changes were made.

## Fixed identity and reproduced failures

- Project: `D:/task/playground/mewo`.
- Pin: `PIN-45969029a8911803a610b77f887787d65edd12a013de02c43fb485b64bc687f1`.
- Product/context: Forever, US, enUS, `1.60.1.70009`.
- Build config: `05215079e3905ef5922ae0b03ffefb73`.
- CDN config: `d2692387b8aea7cfb23cbc7888ba6d5a`.
- Definitions: `61b81d1e518fd276aa879e662c53393ec135681f`.

The original `data db2 --project ... --installation ... --table SpellName
--id 1082` returned `command.failed: archive.index_integrity`; the same pin
with `--cdn` returned HTTP 404. Neither error established corrupt shared
storage or absence of the build from Blizzard's CDN.

## Causes and corrections

1. Shared `Data/data` indexes validated and located Encoding EKey
   `e103d037f40e72c1b4cabd19a2f83bfe` in `data.156`, offset 718239144,
   extent 160260002 bytes. Its 30-byte local preamble was entirely zero.
   The old parser rejected it before authenticating the BLTE header. The
   parser now accepts this exact empty-preamble case, still authenticates
   the full EKey, and rejects nonzero key mismatches. Chunk/page checks and
   decoded CKey verification remain in their owning layers.
2. `data --cdn` built `wow_forever/cdns`, confusing install identity with
   publishing slot `wow_classic_beta`. CDN preparation now uses the existing
   slot mapping. Both remote target preparation and query preparation validate
   `build-uid` against that slot; the user-facing pin stays Forever.
3. An HTTP 403 from one loose-file mirror and 404 from another stopped archive
   lookup. A later authenticated archive copy can now satisfy the request.
   If none succeeds, unknown availability remains an error; corruption,
   cancellation, offline cache misses and budgets do not become permission
   to skip validation.
4. Local missing objects and archive parse/integrity/budget errors were falling
   through to `command.failed`. They now retain typed query codes and exit
   classes. Exhausted content encodings identify the CKey in the error.
5. `target available` displayed the correct publishing-slot locator but fetched
   the install flavor. It now fetches the same slot it reports. An HTTP-transport
   regression reproduces the old 404 and verifies the actual requested URL, not
   merely the display string. Real US discovery now returns Forever 1.60.1.70009.
6. HTTP metadata errors now identify the host/path and status without leaking
   userinfo or query credentials; a 404 no longer hides which endpoint failed.

Both exact configuration keys returned HTTP 200 on
`https://us.cdn.blizzard.com/tpr/wow/config/<prefix>/<key>` during this run.
The earlier guessed `tpr/configs/data/.../buildconfig` failure is not evidence
of unavailable configurations. The original locally resolved pin's group-index
request returned 404 on us.cdn and the other mirror returned 403; that cold lookup
was cancelled after roughly ten minutes, not recorded as a successful query.

The US versions manifest advertises the same BuildConfig under a different
CDNConfig, `7b0257bf0fe52d0f11cd8e7b71f7f832`. A separate remote resolution,
keeping product, region, locale, full build and definition commit identical,
created `PIN-43705be3fc6a7f1d32cfd3f33e5de691fa6bb56ce43548ee30e6a457869c21fd`.
Its target evidence is `CAP-6604ba21694f3b2bfddb8ae3f17cd070b14adf606262d66941d1cda1fa5a28f2`.
The original project lock was not changed. Exact pins remain distinct even when
the BuildConfig agrees; public archive inventory is not inferred from region.

The remote pin successfully reads Claw effects, with evidence
`CAP-6e63d935b524e387c443864dc134c9891447f6e5417d7bb5a2b35c37201d8cc5`,
and SpellName 1082 = Claw, with evidence
`CAP-a08d93f5c1d111f032fe597906e36817f03bbde18ce60f41c53e4b0e13fd35f9`.
The latter cold lookup took roughly four minutes; warm name search returned
within two seconds. These are individual observations, not performance guarantees.
Both reads preserve unavailable encrypted partitions and `complete: false`.
Real `data db2 search SpellName --field Name_lang --query Claw --limit 50`
also succeeded, returned 1082 among multiple matches, and correctly reported
both truncation and partial coverage. It does not identify every matching rank.
Artifacts: `.tmp/forever-availability-fixed.json`, `.tmp/forever-remote-pin.json`,
`.tmp/forever-remote-claw-effect.json`, `.tmp/forever-remote-claw-name.json`,
`.tmp/forever-claw-name-search.json`.

## Actual local result and remaining coverage

The corrected CLI reads `SpellEffect` from the original installation and pin.
`WHERE SpellID = 1082` returns IDs **679797, 679798, 1338554**. Result evidence:
`CAP-83a32f8d73f9433f63ea620523daff9bcf03bcf0b267121f096c2a3c2473aed5`;
local envelope `.tmp/forever-claw-local.json`.

The table has eight unavailable encrypted partitions, so the query remains
partial. Static `EffectBasePointsF`, AP/spell coefficients and effect codes
are raw client data, not proof of server-side damage coefficients.

enUS `SpellName` remains locally unavailable: its selected EKey
`e60f6a2b318e02ad4428cbfca73339a2` was absent from the validated shared
indexes. An additional byte search found neither the full key nor its reverse
in the product-local indexes; this is diagnostic evidence, not a claim of
support for their different v8 format. The CLI reports the missing content
CKey `2f8a667f9a66d347e2f740a31feb158e` instead of an integrity error.

## Independent DBCD comparison

DBCD owns DB2 parsing and DBD binding; it does not establish CASC transport
availability. Its provider/parser separation is preserved in our source,
archive, table/schema and query boundaries. No .NET production dependency.

- Upstream: `wowdev/DBCD@e732093f8864240fc5884bd1bba6b02f3dfc0d56`.
- All 84 supplied upstream source files compared against a freshly downloaded
  archive of that exact commit; oracle rebuilt with the local .NET 10 SDK.
- Input partial DB2 SHA-256:
  `8bb626e251b0f81c673f11d4f1dd7f909d4b3f557a1045f8f789495956cca7d8`.
- DBD SHA-256:
  `c399fdb601bce498dc43c4c337d21b58d7b2fde7f3e0c6abed5fce576c6e73a1`.
- Missing partitions 1–8 come from the production reader's authenticated
  missing-range evidence, not inferred from empty rows.
- Both decoders agree on **42,365** readable IDs, their complete sorted digest,
  all encrypted-ID sets, and every field in the first **200** readable rows.
- A second comparison checks every field of all three Claw effect rows.
  The adapters now accept explicit sampled IDs, rejecting unavailable ones.

Artifacts: `.tmp/forever-dbcd-{oracle,lychee}.json`,
`.tmp/forever-dbcd-claw-{oracle,lychee}.json`, and verified inputs in
`.tmp/forever-dbcd-inputs/`. These are local evidence, not bundled game data.

## Regression and skill scope

Red-before-fix tests cover zero preambles, corrupted payloads, nonzero preamble
mismatch, publishing-slot URLs, build-uid validation, offline exact-pin reuse,
successful archive recovery after mirror errors, and preservation of unknown
availability. Existing source/index integrity and cancellation suites remain.

The data skill now routes preparation errors to the data workflow, explains
shared installation layout, distinguishes local missing files from global
availability, and separates doctor health from snapshot readiness. It retains
the exact pin and partial-coverage requirements. Source skill validation and
the command contract pass; no independent-agent evaluation was performed.
It now documents discovery versus saved targets, name-to-ID search and ambiguous
matches. CLI correctness is caller-independent: humans and agents use the same
validation and source resolution, with no skill-only recovery prerequisite.

Validation logs: `.tmp/forever-records-tests.log`,
`.tmp/forever-full-go.log`, `.tmp/forever-full-go-final.log`,
`.tmp/forever-full-go-discovery.log`.
Final build and vet passed. The last full run used
`LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` and completed with exit 0.
The skill contract checked 87 commands and 211 references with zero violations;
skill validation, version consistency and `git diff --check` passed.

Live server fitting, npm publication and global CLI/skill replacement are
outside this repair's verified scope.
