# Mailbox v1 execution

Use a retained idle CON selected by [live investigation](live-investigation.md).
Run targeted [doctor](live-doctor.md) before the drive; the CLI performs its
own final process, actor, lifecycle and writer checks immediately before WPM.
Tell the user to avoid manual `/reload` during the write and do not initiate it
while writing. An external reload after the last check can still destroy the VM
during WPM; this accepted residual risk is not removed by strong GC roots.

```text
lycheedev live execute --project <project-directory> --session <CON-id> --file <probe.lua> --request <stable-key> --budget-seconds 30 --wait-seconds 120 --format json
```

Use `--probe <immutable-PRB-revision>` instead of a file for registered source.
Keep the exact code, request and budget for continuation. `--budget-seconds`
bounds addon execution (1..120); `--wait-seconds` bounds this host call
(1..600, default 120). The [durable recovery deadline](live-recovery.md#durable-result-and-next-command)
survives between calls. Host timeout does not cancel Lua or renew either budget.

The inbox holds one complete command of up to 1 MiB. The CLI submits it in one
whole-row publication; the addon copies and validates it before executing once.
It checks the selected process/actor, runtime, single-use ready challenge,
request identity, source SHA256 and exact previous-result acknowledgement.
The CLI verifies and durably saves the current result before the next command
carries that ACK. With no next command, disconnect ACKs and closes through the
small stop row. Unknown execution never replays; cancel of an unaccepted command
must leave exact `not_started` terminal evidence before another admission.

Read the result on three independent axes:

| Fact | Meaning |
| --- | --- |
| `reportState: verified` | Exact result bytes were verified and retained |
| `report.ok`, error, assertions and result coverage | Whether the probe's business check succeeded and what it observed |
| `complete: true` with `cleanup: complete` | This command finished its lifecycle |

A verified Lua error/timeout is useful evidence, even when its business result
failed. Pending cleanup still needs [recovery](live-recovery.md), not another
execution. Completed native work needs no ACK/finish/hide command. Repeating
the exact completed request reads retained bytes without rerunning Lua, even
after later operations or game exit; edited parameters/code are not that retry.

The command row is reused without a capacity-triggered reload. An explicit
reload follows [live reload](live-reload.md); its replacement runtime does not
inherit or rerun the original request. Scene-dependent probes must reconstruct
prerequisites or record that they cannot. Ordinary observation
does not need an input shield, and UI activity labels prove neither execution
nor result retrieval.

```text
lycheedev live disconnect <CON-id> --project <project-directory> --wait-seconds 120 --format json
```

Disconnect when the authorized investigation ends unless continued use is needed.
It verifies unbind or [process/runtime retirement](live-recovery.md#gc-damaged-rows-and-runtime-retirement)
before releasing ownership. Completed-close retry is read-only or repairs its
interrupted host retirement. `closed: true` can coexist with unavailable report/
`execution_unknown`; connection cleanup does not establish a business outcome.
Keep the project's whole `.lycheedev/live` evidence directory together.
