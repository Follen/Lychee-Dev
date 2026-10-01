# Error diagnosis

Separate source facts, static data, live evidence and operation state before
acting. When a native CON or pending request already exists, read `live status`
for that CON in its original project and follow its continuation before creating
another request. A failed probe can have a verified report and completed cleanup;
a host timeout can leave successful Lua still running. Neither is a reason to
recreate the error or change request keys.

For an authorized snapshot of errors already retained by the addon provider:

```text
lycheedev live bugs --project <project-directory> --session <CON-id> --request <stable-key> --count <1-100> --wait-seconds 120 --format json
```

This reads !BugGrabber provider storage newest-first through a bounded native
observation. It does not claim to capture every addon error. Provider unavailable,
partial fields, zero rows and incomplete coverage remain distinct. Native bugs
completes verified result persistence and release; inspect `report.ok` and the
snapshot's own status, then require `complete: true` and `cleanup: complete`.
Do not add legacy ACK/finish/hide commands. Use [recovery](live-recovery.md) on interruption, retaining the same CON.
Use a question-specific probe only when the retained error snapshot cannot
distinguish the hypotheses. Follow [live-investigation.md](live-investigation.md)
and keep sampling, output and async lifetime bounded.

Retain the provider's session, message, stack, occurrence counts and available
timing fields without inferring unavailable fields. Repeated provider rows may
aggregate occurrences; row count is not necessarily error count. Ordering is
reverse provider storage, not a newly sorted chronology. Retain `scope`,
`requestedCount`, `returnedCount`, `availableCount`, session and missing-field
markers. A count limit covers that retained slice, not every error since login.
Bind snapshot collection to the verified process/character/build and loaded
addon revision. Provider storage may include earlier sessions; do not attribute
every retained error to that current actor or revision. A stack path or currently
installed file alone does not establish the code that ran at the error's time.

Choose the next step from the evidence:

| Finding | Next useful action |
| --- | --- |
| Verified error with stack and reproduction conditions | Inspect the pinned source at that path and its callers; form a causal explanation before adding a probe. |
| Provider unavailable or partial snapshot | Preserve that coverage limit; use another authorized observation only if it answers the actual question. |
| Verified probe with `report.ok: false` | Read its error/assertion and logs; distinguish invalid preconditions from evidence against the hypothesis. |
| Submitted/uncertain input with no verified report | Continue the original CON/request via recovery; no conclusion about whether the effect ran. |
| Verified report with cleanup pending | Use the report, preserve pending closure and recover that same CON. |

A source stack identifies where an exception surfaced, not necessarily where the
bad value originated. Trace the value's acquisition, transformations and consumers
at the fixed source revision. State which observation would distinguish an API
change, a scene prerequisite failure or an addon-owned value-flow error. Test only
the remaining material distinction; do not repeatedly trigger a known forbidden
operation to gather more copies of its error.

For secret-value or secure-taint errors, connect the relevant fixed source path
to [live hypothesis testing](live-probes.md#test-source-hypotheses).
Use runtime observations to distinguish the remaining source hypotheses, then
return to those locations to explain the result; an error snapshot alone need
not end an authorized deep investigation.

To package known verified evidence:

```text
lycheedev evidence bundle --ids <CAP-a,CAP-b> --output <new-file.zip> --format json
```

Explain the observed error and fixed identity first, then the causal hypothesis,
the evidence that would distinguish alternatives, and any unverified coverage.
Never convert a timeout, missing provider, parse failure or empty candidate page
into “no error”.
