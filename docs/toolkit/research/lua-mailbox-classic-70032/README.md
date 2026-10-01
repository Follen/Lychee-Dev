# Classic 70032 public Mailbox evidence

Immutable Classic-specific evidence bundle; Retail assets remain unchanged.

Target: Classic 5.5.4.70032, PID 50368, process instance 50368:134352957691792733, WowClassic.exe SHA-256 5cb9bcb453c006eb2ff37b8342b582ec4ea7fca323b6d66e6c12fefcb3df463f. Actor: 次年雪 / 祈福 / Player-4778-073BD91C.

The Go resolver migrated the unique 58-byte masked runtime-code pattern to anchor RVA 0x62dc34 and root RVA 0x701dda8. It did not reuse Retail's numeric RVA. The independently captured complete runtime .text is 56,605,244 bytes, SHA-256 603f45a5d310629028a8b2ad58aa7f89477cd0a3dad5a0bc261332123ae31238; exact section dump stays at .tmp/mailbox-live/classic-text-dump/.text.bin. Its complete manifest is included; the dump is non-atomic, with no gaps. wowdump analyze anchor generated root.anchor.json from that dump; analyze relocate returned one structural match.

The production CLI verified identity/input/HEAD/BODY, an independent game API actor probe, normal and 81920-byte outputs with and without cache, readonly execute/reload/disconnect retries with identical journal bytes, and execution after verified reload. Initial activation returned a transient short_read during reload; the original CON was resumed, bound and fully verified without replay. Final CON-7e9009797f9be47a11c9102cd7666d71 is closed with complete=true and cleanup=complete.

root.profile.json exposes only luaState, threadTag and globalsValue. Actual wowdump root query independently read thread tag 8 and globals tag 5 / secret 0, with guards checked. Profile lint and recipe structural relocation passed. The recipe and root capability have read confirmations; profile validation metadata remains candidate/not_checked. Semantic confirmation and full-reader cross-process reuse remain not_checked. No Classic JavaScript complete reader was created or claimed.

Local recipe: C:/Users/follen/.wowdump/recipes/classic@5.5.4.70032/classic-lua-state-root-rip-v1-70032.anchor.json; immutable SHA-256 1b1ef234c82f88d5c547a5f411cd68f88c8961adc3acad67fa16790fbf489de4. Root capability: C:/Users/follen/.wowdump/capabilities/classic@5.5.4.70032/classic-lua-mailbox-root-v1-70032.json. Library evidence copy: C:/Users/follen/.wowdump/evidence/classic@5.5.4.70032/classic-lua-state-root-rip-v1-70032-106a3baf269a.json, SHA-256 106a3baf269afb2b9c0a99db541f4b318923544acfa080d609b1ff91b775b966. The original Retail recipe also has a separate append-only read confirmation against Classic; its immutable recipe bytes are untouched.

Use wowdump query classic-lua-mailbox-root-v1-70032 --pid 50368 --build classic@5.5.4.70032 --verify --require-validation read. This reads only the root profile and grants no game input authority.

All registration reports, root raw read evidence, Go root trace, complete acceptance JSON and final status are retained here. asset-digests.json seals the included files. Capacity exhaustion, fresh process reuse, first-install and relogin were not run.

## Sealed candidate repeat acceptance (2026-10-01)

Candidate SHA-256 3ebce6f071af6a35053d37639a86d511b57e23561b9e4e6f227d9e8177e229fc includes the second root template and safe Snapshot. Classic retained PID 50368 and the same actor. A fresh managed installation was followed by explicit activate-current-bytes reload. The first connect returned live.channel_observation_budget at activation_waiting_runtime after submitted input; its evidence was retained and the original CON was resumed. This is a recovered acceptance, not a claim that the initial call passed. The 12 subsequent core steps passed; CON-518d3178a70bb522b18ad2320efe24b5 is verified closed with complete cleanup.

The independent capacity chain passed all 54 steps, including 47 ordinary requests, verified capacity runtime transition and history rotation, historical read-only retry, compile-error handling, cleanup-error runtime destruction, bugs, and disconnect retry. CON-d8246f26caff3366535543f2e0a05b08 is verified closed with complete cleanup. An additional --no-cache disconnect retry kept journal SHA-256 unchanged. These results supersede the original bundle's capacity not_run statement. Fresh-process reuse, first-install, and relogin remain not_run for Classic.

sealed-candidate/ retains complete initial failure, original resume, recovered core and capacity JSON, final closed status, managed installation receipt and summary. Its original-asset-digests.json preserves the earlier seal. Registered immutable anchor/profile/binding bytes are unchanged.

## Final actual-location candidate acceptance (2026-10-01)

Candidate SHA-256 9b4846015f9a22d64994b494be8c0412eace70c0fbdd24344239283bc125f7a8 includes requested-location mapping intersection guards and transparent original read errors. The fresh managed Classic install and immediate explicit activation reload succeeded on the first call. All 13 core steps and 54 capacity-chain steps (47 ordinary requests) passed; direct JSON counts show zero nonzero exit codes in both reports. Capacity runtime transition, history rotation and historical read-only retry, compile-error handling, cleanup-error runtime destruction, bugs, and final disconnect were verified. CON-dd51b69cd9f2e4041aabd0b6796fd39b and CON-663500f910df2fc23c0bbe6b424b9555 are verified closed with complete cleanup. The extra --no-cache disconnect retry preserved journal SHA-256 8f706ff67e7593e4908632791d948fd1dd48463014e42a7f7b39505500d37a77.

current-candidate/ retains full raw JSON, the original preceding seal, and an independent confirmed-read root-profile query. Earlier activation failures and recovered acceptance remain retained as earlier candidate facts. Registered anchor/profile/binding are unchanged. Fresh-process reuse, first-install, and relogin remain not_run for Classic. The appended library confirmation is read level only.
