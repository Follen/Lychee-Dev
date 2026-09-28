# Error diagnosis

Separate source facts, static data, live evidence and operation state before
acting. Start with `live status` when an operation ID already exists; do not run
another probe merely to recreate an error.

For an authorized snapshot of errors already retained by the addon provider:

```text
lycheedev live bugs --project <project-directory> --session <CON-id> --request <stable-key> --count <1-100> --wait-seconds 120 --format json
```

This reads !BugGrabber provider storage newest-first through a bounded native
observation. It does not claim to capture every addon error. Provider unavailable,
partial fields, zero rows and incomplete coverage remain distinct. Native bugs
completes verified result persistence and release; inspect `report.ok` and the
snapshot's own status, then require `complete: true` and `cleanup: complete`.
Do not add legacy ACK/finish/hide commands. Resume the same CON on interruption.
Use a question-specific probe only when the retained error snapshot cannot
distinguish the hypotheses. Follow [live-investigation.md](live-investigation.md)
and keep sampling, output and async lifetime bounded.

For secret-value or secure-taint errors, connect the relevant fixed source path
to [live hypothesis testing](live-investigation.md#test-source-hypotheses).
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
