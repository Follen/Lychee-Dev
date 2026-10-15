# Formal repair delivery candidate R2

Oracle: released Go v3.2.0, commit
`3057a9666ab078270b63c7f06de748069b0d1f4b`.

This is a source candidate, not native acceptance. Rust/Cargo, Go differential,
Windows fixture, real-game and performance execution are all **NOT_RUN**.

## Composition and scope

The Live bridge holds formal installation admission with no outstanding CON,
OP or BTP claims. The original CON is closed separately before this adapter;
Delivery never reads/resumes business journals. The host must supply the real
compile-only AddonValidator and the original OperationControl. Delivery stages
the complete verified release, revalidates the exact selected client, acquires
the existing maintenance then installation namespaces, and publishes managed
main/500-slot changes. Existing archives are not implicitly resumed.

R1 source and its two review findings remain available in the R1 freeze/test map.
R2 fixes close-before-path-delete substitution and resource-array-order equality.
The R1 snapshot is `/workspace/shared/formal-repair-delivery-r1-63047a33728e`.

## Identity-bound cleanup

Windows `owned_cleanup` adds one reviewed-boundary candidate using the existing
Stage FILE_DISPOSITION_INFO algorithm. An opaque CREATE_NEW temporary token may
mark only its original object for deletion. A cleanup candidate produces deletion
authority only after ordinary-kind/reparse checks and caller validation on its
same deny-write/delete handle. Deletion consumes that authority; its result and
the subsequent checked close are both retained. There is no pathname deletion
fallback or uncertain-close retry.

Private snapshot cleanup establishes a held ancestor chain before descending.
It authenticates and retains all admitted files and directories before deleting
anything. Failure or cancellation during admission closes every acquired owner
without deletion. Subsequent mutation can neither replace those held objects
nor redirect cleanup into another parent. Foreign new children make directory
deletion fail rather than being recursively removed. Success releases all files
and directories; normal repetition does not accumulate preparation roots.

Resource reservation follows the actual expected inventory, not an eager 65,536
handle allocation. Every open and scan checks the original control. The combined
handle/node ceiling is 65,536 and ancestry is bounded at 1,024 components. These
safety limits and the OS costs require native/performance verification.

After a temporary's original handle has been consumed, a close failure, late
stop or publication failure retains its path as unconfirmed evidence. The helper
never deletes a possibly substituted object. There are 64 temporary slots per
exact destination filename namespace, not 64 across unrelated directory writes.
Four private preparation slots per selected target bound retained failed or
uncertain attempts. Their ownership record includes target, release, creation,
process, byte and entry bounds. Existing or foreign entries are not adopted or
reclaimed by age or PID. Exhaustion blocks further attempts for explicit review.
These are new conservative safety constraints, not claimed Go limits. A validator
error always retains the complete release snapshot, including an unresolved
compiler executable; no assumption of child-process termination is made.

## Receipt equality

Both receipts first reject duplicate resource paths (including case collisions).
Header identity remains exact. Resource entries are compared by path-to-resource
maps, matching Go sameReceipt's order-insensitive contract. Reordering only the
release resources is an idempotent fresh retry. Different hashes, lengths, paths,
components, schemas, versions or commits remain mismatches.

## Required integration

The parent exports installation::inspect_main_controlled. The activation/doctor
batch owns ManagedDoctorDelivery and its checked formal-admission close. The CLI
batch owns the authenticated compile-only validator and invocation routing.
No release publication, user desktop, game input, main merge, Cargo or rustc was
performed for this candidate. New Rust/native tests are declarations only; tests
that inject close outcomes are fault models, not real kernel close-failure proof.
