# Retail 12.1.0.69933 reload-state research

This note records a read-only observation of a manually initiated reload on the exact Retail process instance listed below. It separates direct observations from static interpretation. No game input, memory writes, reload calls, or process control were used by the observer.

## Evidence identity and coverage

- Product/build: `retail@12.1.0.69933`; executable SHA-256 `d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`.
- Observed process: PID `140100`, process instance `140100:134355550237345516`; `Wow.exe` base `0x7ff7bc080000`.
- The observer verified this PID, process instance, executable hash, module path, and build in its records. A post-run `wowdump builds ensure` returned the same process instance and reused capture `2c2724df-1baf-46fb-9b8a-00fbd7c75a04`.
- Runtime capture is complete but non-atomic. Its manifest SHA-256 is `ba5aa2cc907427d3aa5c3a8cc14e4a21dc54cb23bad27db659e8749038fe0100`; captured `.text` SHA-256 is `d36e6e25067556d50526d25c5ecf42b1bb17f1296d2bc648e289d38f052604b9`. An older archived runtime capture (2026-09-30) has the same executable SHA but a different `.text` SHA-256: `4fa9cbb1a90fa114d26f64a8e2e14e66852a5c71450ede69cdfd079c23348f75`; its archived manifest SHA-256 is `c1703a7237f9502d15fc212534ba5be90654fe7e8304a038372cee8a44444154`. The four captured sections `.text`, `.rdata`, `.pdata`, and `.data` were complete with no gaps.
- `wowdump observe` ran for 179,981.9885 ms, with 9,000 samples at a requested 20 ms cadence; mean interval was 20.000079 ms, with zero skipped intervals and zero indeterminate samples. It read 99,000 bytes in 27,000 calls. The JSONL observation SHA-256 is `fdf2a570ac52b84d91c98b4558d13c5a88e33fbb09e07481cb73c6c7f92cf496`.
- The observation is explicitly non-atomic. Each sample reads the mode/world word, the pending byte, and the Lua root pointer separately; cross-field combinations in a row are near-time correlated observations, not an atomic snapshot.

## Direct transition samples

The owner reported manually reloading while the observer was active. These are the samples at which at least one selected field changed. The 16-bit word bytes are shown in little-endian order. `world(bit 4)` is calculated as `word & 0x10`; `bit 8` is calculated as `word & 0x100`. The pending byte stayed `00` in all listed samples.

| Sample | UTC | Offset ms | Word bytes | Word | bit 4 | bit 8 | Pending | Lua root | Atomic |
|---:|---|---:|---|---:|:---:|:---:|---:|---|:---:|
| 1705 | 02:54:57.629Z | 34107.380 | `91 03` | `0x0391` | 1 | 1 | `00` | `0x281aed470a0` | no |
| 1706 | 02:54:57.644Z | 34122.168 | `81 03` | `0x0381` | 0 | 1 | `00` | `0x281aed470a0` | no |
| 1707 | 02:54:57.673Z | 34151.824 | `81 01` | `0x0181` | 0 | 1 | `00` | `0x281aed470a0` | no |
| 1735 | 02:54:58.235Z | 34713.403 | `0d 01` | `0x010d` | 0 | 1 | `00` | `0x281aed470a0` | no |
| 1758 | 02:54:58.688Z | 35166.810 | `0d 01` | `0x010d` | 0 | 1 | `00` | `0x2823461f0a0` | no |
| 1913 | 02:55:01.787Z | 38265.308 | `09 03` | `0x0309` | 0 | 1 | `00` | `0x2823461f0a0` | no |
| 1914 | 02:55:01.817Z | 38296.005 | `19 03` | `0x0319` | 1 | 1 | `00` | `0x2823461f0a0` | no |
| 1931 | 02:55:02.152Z | 38630.525 | `11 03` | `0x0311` | 1 | 1 | `00` | `0x2823461f0a0` | no |
| 1978 | 02:55:03.088Z | 39566.265 | `11 02` | `0x0211` | 1 | 0 | `00` | `0x2823461f0a0` | no |

The process instance was identical in all nine samples. The Lua root pointer changed from `0x281aed470a0` to `0x2823461f0a0`. The bit-4 state became true again at sample 1914 while bit 8 was still set; bit 8 cleared at sample 1978. Therefore, the observed bit-4/world-ready state alone did not coincide with the observed end of the bit-8 transition. This is a temporal inference from separate non-atomic reads, not a claim that these bits form a complete or universally safe reload gate. The normal pending byte remained zero throughout these recorded changes.

## Static provenance and anchor candidates

The source is pinned to Retail 12.1.0.69933, executable SHA above, and the archived source manifest. The masks below are copied from `wowdump` candidate artifacts. A unique full-section hit only establishes uniqueness in that source image. These artifacts remain **unconfirmed candidates**, not certified recipes; they have not been promoted or confirmed, and same-build recipe replay and cross-build reuse have not been run.

| Candidate ID | Source RVA → value | Resolve | Full mask | Hits / scan | Candidate artifact SHA-256 |
|---|---|---|---|---:|---|
| `retail-c-ui-reload-wrapper-69933` | `0x12d2fb0` → `0x12d2fb0` | match instruction 0 | `48 89 5c 24 ?? 57 48 83 ec 20 48 8b f9 e8 ?? ?? ?? ?? 0f 1f 40 ?? 66 66 0f 1f 84 00 ?? ?? ?? ?? 90 75 ?? c0 f1 00 74 ?? 5b` | 1 / complete | `76ef8159f6b8e98681528a67881ece7b02311cc60d7851acc81e1fdf15ee40f9` |
| `retail-c-ui-reload-native-handler-69933` | `0x2706c00` → `0x2706c00` | match instruction 0 | `0f b7 05 ?? ?? ?? ?? 66 0f ba e0 09 0f 83 ?? ?? ?? ?? 4c 8b 05 ?? ?? ?? ?? 4d 85 c0 74 ?? 45 8b 40 ?? 48 8d 0d ?? ?? ?? ?? 4c 8d 0d ?? ?? ?? ?? 48 8b 15 ?? ?? ?? ?? 44 39 01 74 ??` | 1 / complete | `a2bf7447ca70bf6407427e71bbdd40e7107dbcb23e603463beb1730f9f6ba73c` |\n| `retail-c-ui-reload-wrapper-pid140100-69933` | `0x12d2fb0` → `0x12d2fb0` | match instruction 0 | `48 89 5c 24 ?? 57 48 83 ec 20 48 8b f9 e8 ?? ?? ?? ?? 0f 1f 40 ?? 66 66 0f 1f 84 00 ?? ?? ?? ?? 90 75 ?? c0 f1 00 74 ?? 5b` | 1 / complete | `97f68ad220b53c09bf8907bd68fa38e6819e79995e1d549ac51e9cab3179e17d` |\n| `retail-c-ui-reload-handler-pid140100-69933` | `0x2706c00` → `0x2706c00` | match instruction 0 | `0f b7 05 ?? ?? ?? ?? 66 0f ba e0 09 0f 83 ?? ?? ?? ?? 4c 8b 05 ?? ?? ?? ?? 4d 85 c0 74 ?? 45 8b 40 ?? 48 8d 0d ?? ?? ?? ?? 4c 8d 0d ?? ?? ?? ?? 48 8b 15 ?? ?? ?? ?? 44 39 01 74 ??` | 1 / complete | `eb1ecc0936dceb81e35d27acf9d9513219a6ac0291d103a0bba1aa1ea79e3b68` |
| `retail-reload-modeword-rip-pid140100-69933` | `0x2706c00` → `0x6066a38` | RIP instruction 0 | `0f b7 05 ?? ?? ?? ?? 66 0f ba e0 09 0f 83 ?? ?? ?? ?? 4c 8b 05 ?? ?? ?? ?? 4d 85 c0 74 ?? 45 8b 40 ?? 48 8d 0d ?? ?? ?? ?? 4c 8d 0d ?? ?? ?? ?? 48 8b 15 ?? ?? ?? ?? 44 39 01 74 ??` | 1 / complete | `81b49f289f36d9ddd1b4c8713183d36cb6f98b31532dff44747685bf4ec7a231` |
| `retail-reload-pending-byte-rip-pid140100-69933` | `0x22b350a` → `0x59f900d` | RIP instruction 0 | `80 3d ?? ?? ?? ?? 00 74 ?? 80 3d ?? ?? ?? ?? 00 75 ?? e8 ?? ?? ?? ?? c6 05 ?? ?? ?? ?? 00 80 3d ?? ?? ?? ?? 00 74 ?? 80 3d ?? ?? ?? ?? 00 75 ?? 48 8d 0d ?? ?? ?? ?? e8 ?? ?? ?? ??` | 1 / complete | `14b7fdfeca6979b7aa224752fc6be458680982762f4768d8f01c227167708a0f` |
| `retail-reload-guard59f9048-rip-pid140100-69933` | `0x2706ca7` → `0x59f9048` | RIP instruction 0 | `80 3d ?? ?? ?? ?? 00 75 ?? c6 05 ?? ?? ?? ?? 01 c3` | 1 / complete | `eda827a038e9691e3db7fc93e8dd15f96e0618bf2993dd16f0afda4ed2b59a06` |
| `retail-isplayerinworld-getter-rip-69933` | `0x17bb14b` → `0x6066a38` | RIP instruction 0 | `0f b6 05 ?? ?? ?? ?? c0 e8 04 24 01 48 89 35 ?? ?? ?? ?? 88 44 24 ??` | 1 / complete | `26a2d0cbb2402273f09118a1a8b33cc5b4450d9b9b908bcc011beef8c551db5e` |
| `retail-isplayerinworld-setter-69933` | `0x2625dae` → `0x2625dae` | match instruction 0 | `41 bf 04 00 00 00 66 44 0f ab f8 66 89 05 ?? ?? ?? ??` | 1 / complete | `ecb93418f08572a0b41535c36f963348dd582497ba457bbc683d3cc940150c87` |
| `retail-isplayerinworld-leave-clear-69933` | `0x2627980` → `0x2627980` | match instruction 0 | `41 bf 04 00 00 00 66 44 0f b3 f8 66 89 05 ?? ?? ?? ??` | 1 / complete | `7528b0969eea63055edf489fa359172314516b0c85f259ec3e0b08abf9c32e8b` |

A current-runtime C_UI wrapper and handler candidate were also captured from the observed process `.text` hash. Each had one full-section hit. Those candidates are tied to the non-atomic runtime capture and are distinct from the archived-image candidates; they are not cross-build evidence. The IsPlayerInWorld registration pattern remained ambiguous at 32 hits even with a longer window and is excluded as a locator.

## Static/API interpretation

The pinned UI source commit is `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`. Its `PlayerScriptDocumentation.lua` documents `IsPlayerInWorld` at lines 1236–1243 (source SHA-256 `6d2687568a7b144cd40579f5f9f3c68db478652257ce7e6c12b385f57a1eff67`; capture `CAP-ab14c66dfd59545ba3c990339a434c0b10ce97f739fe4c9a5e7a602224ba2d09`). Static native provenance ties its getter at `0x17bb14b` to bit 4 of the word at `0x6066a38`; the owner setter at `0x2625d80` sets bit 4, and the leave/teardown owner at `0x2627930` clears bit 4.

`C_UI.Reload` registers at `0x12d3c3c`, wrapper `0x12d2fb0`, handler `0x2706c00`. Static control flow reads bit 9 of word `0x6066a38`; one path may set byte `0x59f900d` after checking bytes `0x59f8fe7` and `0x59f9048`, while another path may set bit 7 or call `0x2834920`. A separate bit-7 consumer checks bit 5 and enters worker `0x2636b40`; that worker sets bit 8, calls teardown `0x2628e30` and initialization `0x2625470`, and initialization clears bits 7 and 8. These branches are distinct and the relevant guard/product semantics are not fully audited here.

A separate read-only `lycheedev doctor` snapshot after the reload reported native lifecycle `no_reload_observed`, mode flags `529` (`0x0211`), `worldReady=true`, method `known_anchors`, and qualification `getter_pattern_observed` after 20 reads / 800 bytes. The overall doctor command exited 3 because the addon duplex schema did not match; the diagnostics also reported another project's native connection claim. Neither condition was changed. The doctor JSON result SHA-256 is `47363c6cf5beb158dbaf3186313c03254c792bdc0411863103742bc6988e9d3a`. That doctor observation is a separate reader result, not a replacement for the raw wowdump sample trace.

## Evidence status

- **Observation:** passed for the exact captured process instance; full raw trace hash above; non-atomic, 20 ms requested interval.
- **Static/API audit:** partial. Getter, bit transitions, and Reload entry path have source/disassembly provenance; complete teardown/readiness proof and guard semantics remain unverified.
- **Same-build recipe reuse:** not run. Candidate uniqueness in one full source image is not recipe replay/confirmation.
- **Cross-build reuse:** not run. No second build was sampled or validated.
- **Reload safety gate:** not established. In particular, the observed `worldReady` bit returned to 1 while bit 8 remained set.


## Strict locator records and read confirmation

The raw candidate artifacts listed above remain unconfirmed candidates. Three separate strict `wowdump.anchor.v1` records were produced by `wowdump recipes candidates promote` against the complete same-build runtime manifest, then stored with `wowdump recipes save` in the isolated taskhome recipe library. Their full-source scans each had one hit. `wowdump recipes confirm --level read` accepted successful query reports for the exact build; no semantics or reuse confirmation was recorded.

| Strict recipe ID | Target RVA | Strict anchor SHA-256 | Read confirmation |
|---|---:|---|---|
| `retail-reload-modeword-rip-pid140100-69933` | `0x6066a38` | `031d10b02c5d2dc77dabe0a06885f4f58a6fb4c1446277380142c8ddf94f0320` | `retail@12.1.0.69933` |
| `retail-reload-pending-byte-rip-pid140100-69933` | `0x59f900d` | `a8a8feabc85d15fbb4902be67b6219452849d4940b9abd22a47b5ec302b0a398` | `retail@12.1.0.69933` |
| `retail-isplayerinworld-getter-rip-live140100-69933` | `0x6066a38` | `411a1209e32aa1dbf46bf0a0e30a8ba08974c8d71328c6ced90f8e5cc8f0a19d` | `retail@12.1.0.69933` |

The strict recipe files are stored under `recipes/retail@12.1.0.69933/` in the wowdump taskhome; the original candidate files remain under `anchor-candidates/retail@12.1.0.69933/`. The modeword and pending-byte read confirmations cite the exact-process `wowdump query` JSON report (SHA-256 `9798765ef6e2c1dd4a61654d3fc8fd4ead0749ed654242d4891382248db3f5a6`). The getter read confirmation cites the independent same-build Go doctor result (SHA-256 `47363c6cf5beb158dbaf3186313c03254c792bdc0411863103742bc6988e9d3a`), which reports `getter_pattern_observed`, `modeFlags=0x0211`, and `worldReady=true`.

The read confirmations establish only that these unique same-build locators resolved to the corresponding state RVAs and that same-build readers returned data. They do not confirm decoded bit semantics, reload safety, writer authority, or cross-build reuse. The wowdump profile at `pid140100-native-flags-and-lua-root.profile.json` passed `profiles lint` and live `query`, but remains `readerStatus=candidate`; no reader-ready capability was saved. The isolated capability save was blocked by the missing `retail-lua-state-root-69933` dependency in that taskhome, so the Lua root is present in this profile/trace but not included in a saved capability.
