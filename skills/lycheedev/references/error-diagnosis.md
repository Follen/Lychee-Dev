# Error diagnosis

Read existing CON status/results before another request. A verified failed probe
can have complete cleanup; a host timeout can leave Lua running. Neither alone
justifies reproducing the error or changing request keys.

For authorized retained errors, use the supported live bugs command. It takes a
bounded !BugGrabber snapshot, not every addon error. Provider unavailable, missing
fields, partial scope and zero rows are different. Keep session, message, stack,
counts/timing and scope metadata. Rows can aggregate occurrences and follow reverse
provider storage rather than a newly sorted chronology.

Bind collection to the current verified process/actor/build, but do not attribute
all retained rows to that actor or loaded addon revision: storage can contain
prior sessions. A current file or stack path does not prove what ran at error time.

A stack locates where an exception surfaced, not necessarily where its value
originated. Inspect fixed source and its callers; distinguish API change,
precondition failure and addon value flow. Use a question-specific
[probe](live-probes.md#test-source-hypotheses) only for the missing causal distinction
within existing live scope. Do not repeatedly trigger a known forbidden operation.

Interpret report.ok/assertions separately from report verification and cleanup.
Submitted/uncertain input with no report stays unresolved; verified results with
cleanup pending need [same-CON recovery](live-recovery.md). Timeout, provider
failure or an empty partial page never means “no error.” Finish with the supported
cause or hypothesis, fixed identity/captures and the remaining material gap.
