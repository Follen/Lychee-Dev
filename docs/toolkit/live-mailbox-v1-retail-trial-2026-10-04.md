# Mailbox v1 Retail stopped-write incident (2026-10-04)

This is the first formal CLI-to-game write trial, not an addon-only self-check.
It **did not pass**. The exact Retail writer profile is disabled in source. Do
not repeat the stopped-helper route against this client from the earlier local
development package.

Target: Retail `12.1.0.69933`, executable SHA256
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`,
PID `287456` at `D:\Game\World of Warcraft\_retail_\Wow.exe`.
Managed addon receipt: version `3.1.1`, commit `b0212512cdb4c5b6c571f64b55ea8c13a1279c3e`.
After the owner's `/reload`, targeted doctor observed the new layout, advancing
sendbox, exact actor, `world_ready`, and `no_reload_observed`.

`live connect` selected `CON-f4c2253575e6fb68818bd09751ec41d3`. Its first
149-byte probe request was journaled as
`057da37da9e67a67e19502eb32f4dc3a`. At 15:18:42 local time the helper
reported debugger attach and detach, `stoppedMicros=37368`, one 6,293,376-byte
whole-row WPM, and full readback equality. The addon did **not** return a
verified execution result. The host subsequently observed `path_changed` while
reading a changing Lua publication, then rejected the addon's request status
with `json: unknown field "totalBytes"`. The request remains unresolved; the
write receipt does not prove acceptance or execution.

At 15:18:43.611 the client generated an `ACCESS_VIOLATION` at address zero,
classified by its report as `Security Crash` / code 100. The crash text and
dump are retained in the client's `_retail_/Errors` directory as
`2026-10-04_15.18.43_Crash_287456.txt` and `.dmp`. A previous client PID had a same-class
crash at 14:28:19, before this trial. This limits causal attribution: debugger
attachment, the memory write, another external tool, and pre-existing client
failure have not been distinguished. The temporal association is enough to
reject this writer profile and stop live writes.

Exact local crash hashes:

- text SHA256: `ccbfef3877fb111885cfa76b557cc808737cebecc8f8378940a7540cc3bd8afb`;
- dump SHA256: `744252b2f280ecd7174c5780b89946b74354f5db898c1a491457372df380d112`.

The two host read/projection bugs have separate offline reproductions and
fixes. Neither fixes or explains the client security crash. No second game
write, automatic replay, cancel or disconnect was sent after the failure.
Small/1 MiB command execution, ACK, cancel, disconnect, reload, GC, dual
instance and cross-build write acceptance remain unverified in this version.
