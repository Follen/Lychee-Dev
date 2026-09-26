# Error diagnosis

Separate source facts, static data, live evidence and operation state before
acting. Start with `live status` when an operation ID already exists; do not run
another probe merely to recreate an error.

For an authorized snapshot of errors already retained by the addon provider:

```text
lycheedev live bugs --session <session-id> --request <stable-key> --count <1-100> --format json
```

This reports !BugGrabber provider storage newest-first. It does not claim to
capture every addon error. `provider_unavailable`, partial fields, zero returned
rows and incomplete coverage must remain distinct. After reading the verified
report, call `live finish <operation-id>` to acknowledge and dismiss the final
receipt. Check `display.state: cleared` and `complete: true`; a pending display
retains the verified report and is recovered with finish on the same operation.
Perform this within the
authorized task without asking for a separate ACK confirmation. A QR or verified
report with pending cleanup is not the end of this workflow.

Use a question-specific probe only when the retained error snapshot cannot
distinguish the hypotheses. Follow [live-investigation.md](live-investigation.md)
and keep sampling, output and async lifetime bounded.

To package known verified evidence:

```text
lycheedev evidence bundle --ids <CAP-a,CAP-b> --output <new-file.zip> --format json
```

Explain the observed error and fixed identity first, then the causal hypothesis,
the evidence that would distinguish alternatives, and any unverified coverage.
Never convert a timeout, missing provider, parse failure or empty candidate page
into “no error”.
