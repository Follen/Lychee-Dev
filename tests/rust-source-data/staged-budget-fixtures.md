# Source resource fixture drafts

These files began as unregistered drafts while the Source compilation inputs
were frozen. After the root owner released that freeze, the two Rust modules
were moved into `crates/lycheedev-source/src` and registered as Windows test-only
children of `luals`, so their actual bytes enter the governed source digest.
Root's artifact-bound light and native runs are recorded below. They do not alter
the current CLI, normal Retail workload, diagnostics configuration, 2 GiB
aggregate ceiling or 1.75 GiB child partition.

Registration changes the Source input identity and requires controlled builds
and affected checks. Run
the native quota cases separately in an exclusive load interval.

| Draft | Intended executable check | Current evidence |
| --- | --- | --- |
| `crates/lycheedev-source/src/rpc_budget_tests.rs` | 128 actual semantic requests per batch, two batches on the same child; query 129 refusal before any request; shared accumulated result admission; descriptor admission before second RPC; workspace and original child cleanup; actual OS thread admission refusal at each partial-start boundary | Actual 44-light runs passed on source95e and naming-repair sourcee1b. OS stack rejection code87/held exits7 demonstrates generic partial-start cleanup, not actual memory-pressure causality. |
| `crates/lycheedev-source/tests/native_quota_peer.rs` | Native `VirtualAlloc(MEM_RESERVE\|MEM_COMMIT)` allocations retained until the production Rpc Job retires; direct child and a descendant created after Job attachment | Standalone harness=false Cargo target, inactive without AUTH. Root governed seal04 helper b1c5/lib540c00 and run05 passed inactive/direct/aggregate on sourcee15. |
| `crates/lycheedev-source/src/rpc_native_quota_tests.rs` | Original handles held for child, post-attach descendant and typed OS consoles; successful retained allocations approach the configured partition, then refuse; exact Job limits/members and all retirements | Root run05 passed direct1872756736 bytes and aggregate1870659584 bytes/error1455, original2GiB parent/1.75GiB child, all held exits7 and parent-onlyactive1/outeractive0. Scope and prior failures remain explicit. |

The native quota draft records allocation-error values and held-process
PrivateUsage separately. A failure near the expected ceiling is an observation,
not exclusive attribution to Job quota: machine-wide commit exhaustion can also
fail an allocation. JobGuard now has read-only memory-accounting and bounded
membership queries against its exact owned Job handle. The ignored fixtures
bind those observations to original held-process identities. Current aggregate
commit remains unavailable and there is no memory-limit completion notification;
the causal attribution limitation remains explicit after actual execution.

`.tmp/rust-source-native-quota-run-05/report.json` preserves the passed scoped
native result, all original held identities, full canonical locked system
console path/hash and queried membership. It retains historical peaks above the
configured limit; no peak<=limit invariant is claimed. Runs01/02/03 failed before
allocation (evidence parent, unknown member, then observed console under the old
assertion); run04 actually allocated then exposed an unsupported peak assertion.
Those failures remain intact. Run05 is not LuaLS repetition, full lifecycle,
exclusive quota-refusal cause, pre-attachment inheritance or historical AV proof.

The parent-controlled build selects `cargo test --offline -p lycheedev-source
--lib --test native_quota_peer --no-run --message-format=json`, then seals the
compiler-artifact-selected lib-test/helper EXEs and matching PDBs, full source
digest and helper source SHA256. No direct rustc compilation is used. The two
ignored tests run separately using the sealed helper path/hash and a fresh case
directory. Each fixture generates a fresh 32-character lowercase hexadecimal
AUTH nonce only for its owned helper; both replies must echo it. The queried
2 GiB enclosing Job, 1.75 GiB child limit/flag and held-identity-bound membership
are checked before the allocation request. Ordinary test runs emit an explicit
inactive helper record and allocate nothing.

The descendant deliberately starts after the first request, when Rpc attachment
has completed. It tests inheritance after attachment and does not cover or fix
the documented pre-attachment descendant race.

The 5 MiB descriptor fixture reaches the public WorkspaceSession SDK directly.
It cannot reach the production broker, whose entire request frame has a 64 KiB
limit. It verifies SDK admission and must not be counted as a public CLI quota
case. The 128-query and result cases similarly exercise actual Rpc consumption
and WorkspaceSession lifecycle; public broker/CLI batch composition still needs
its own artifact-bound coverage.

The Windows contract distinguishes committed-memory budgets from working set:
[JOB_OBJECT_LIMIT_JOB_MEMORY](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
rejects commits that would exceed the job-wide limit. A completion port can
receive `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT`;
[extended limit information](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
exposes the configured JobMemoryLimit and peak usage. No draft treats RSS,
historical peak or a failed allocation as current aggregate commit.
