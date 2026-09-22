# Error diagnosis

Use this workflow when the user asks about an existing game error, a failed
Toolkit operation, or a symptom that may require a live probe.

## Classify before acting

First separate the evidence classes:

- source evidence: what the pinned code/API/TOC contains;
- data evidence: what the fixed static or change dataset returns;
- live evidence: what the selected client reported and persisted;
- operation evidence: what the host requested, confirmed, or left unresolved.

Use an existing capture when it answers the question. Do not rerun a probe just
to obtain a familiar error or to fill a requested count. If the issue is a
failed operation, use `live status <operation-id>` to inspect its available
report and cleanup status before starting anything new.

For new, explicitly authorized live collection, use the bounded `live run`
path from [live-investigation.md](live-investigation.md), then inspect the
verified report. A dedicated error-store command is not implemented yet. A
question-specific probe must name the actual inspected error store; do not
imply it captures every addon error. Keep error counts, scope, ordering and
completeness tied to the returned evidence.

## Explain without overclaiming

Report the concrete error code/message, stage, fixed identity, and complete
capture or log location. Then distinguish:

1. observed fact;
2. causal hypothesis and the evidence that would distinguish alternatives;
3. safe next investigation or implementation option;
4. unverified or unavailable coverage.

Do not convert a parse failure, timeout, empty result, or missing dependency
into “no error.” Do not switch source/build/locale or recommend destructive
cleanup to make the symptom disappear. If a host crash occurred after an
external input was accepted, keep the operation unresolved until a matching
report or protocol state proves what happened.
