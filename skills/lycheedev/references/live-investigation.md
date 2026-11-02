# Live investigation and recovery

Read the original CON's status/results before starting work. Use installed schema
for exact calls, effects and returned continuation. A Data pin does not select or
authenticate a process. Reuse the chosen installation/PID and bind fresh process/
runtime/actor identity; same-build processes sharing files remain separate.

For readiness failures, `doctor` diagnoses without input; apply supported repair
only within existing task scope. Follow its concrete blocker/next step rather
than guessing bindings, offsets or input. A manual reload wait needs one user
action in the exact selected window, followed by the returned CON. File deployment
does not prove loaded readiness. Each selected process needs its own activation.

## Observe only the missing fact

Retained addon errors may answer the question without reproduction. Their provider
can be missing/partial and can include earlier sessions; a current stack/file
does not prove which actor or revision produced it. A stack locates a failure,
not necessarily its cause. Use fixed source/callers, then a [probe](live-probes.md)
only for the unresolved distinction. Read [measurement guidance](runtime-investigations.md)
for CPU/memory claims.

Keep the exact source/probe revision and stable request key. Choose observation
policy only for a wholly repeatable experiment; opaque effects stay opaque.
Transport verification, report assertions/coverage and cleanup are separate facts.
A verified Lua failure is evidence; a successful observation outside failing
conditions does not disprove the fault.

## Finish the original lifecycle

Follow returned continuation, error, blocker and remaining budget on the original
project/CON/request. Wait for an active owner or external condition; do not start
a competing driver. Resume does not renew the durable deadline. Submitted input
proves queue submission, not execution; missing/partial reads never permit replay.
Never change keys, journals, claims, slots or send raw input to force progress.

A verified report with cleanup pending remains useful evidence and unfinished
work. Continue recovery/disconnect when done, unless continued use is needed.
Process/runtime retirement and business completion are separate: a replacement
runtime does not prove an unknown old effect never happened. Nonretryable errors
need a changed cause, not a resume/reload loop. Preserve the entire
`.lycheedev/live` recovery unit and older incompatible journals for matching-version
diagnosis. Report useful retained results and the concrete blocker if progress
requires unavailable evidence or user action.
