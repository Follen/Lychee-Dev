# Mailbox v1 direct Retail trial (2026-10-04)

The owner authorized a direct whole-row write without debugger attachment or
thread suspension after the stopped-write client crash. This trial passed two
commands and their independent close messages on one real Retail instance.

Target: exact `D:\Game\World of Warcraft\_retail_\Wow.exe`, Retail
`12.1.0.69933`, SHA256
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`,
PID `180212`, creation `2026-10-04 15:40:45.321039 +08:00`. Actor was
灵止光 / 死亡之翼, GUID `Player-741-066A34E3`. The managed addon was clean at
version `3.1.1`; its installed commit was `b021251`. The new local CLI package
was built from clean commit `39844a4` and was not published. Targeted doctor
reported `world_ready`, `no_reload_observed`, advancing runtime and the exact
process and actor identities before publication.

| Request | Source bytes | CLI result | Request creation to observed execution |
| --- | ---: | --- | ---: |
| `41c7b54a9a06c4237f3455e9c0101d03` | 149 | Build `69933`, `inWorld=true`; verified | 267 ms |
| `b650b3037f0151583620c4f894c1798f` | 1,048,576 | `true`; verified | 19,156 ms |

Each command trace says `mode=direct`, `stop.attached=false`, one
6,293,376-byte whole-row WPM and complete readback. The separate close for
each connection wrote one 8,064-byte control row; both close receipts verified
release, `closed=true` and `cleanup=complete`. The game process with the same
PID and creation identity remained alive after both operations. The host
journal intervals above include admission, addon validation and observation;
they do not measure WPM time alone. In particular, the 1 MiB result does not
support a claim of low end-to-end latency.

The direct route checks the process, exact image, Lua root, actor, sendbox,
world and reload state before WPM. Strong private Lua references protect the
row from ordinary GC. A `/reload`, logout or VM teardown that begins after the
last check can still free it before or during WPM. This race is explicitly
unresolved; the successful trials did not exercise it. Cancel, reload,
multi-instance competition, other builds and long-duration memory behavior
remain outside this real-client observation.

Local evidence (ignored by Git):

- `Analyze/duplex-mailbox/evidence/offline-direct-20261004-01/report.json`:
  full build, vet, mandatory Lua 5.1/Go, Node, version and skill baseline passed.
- `Analyze/duplex-mailbox/release-direct/`: clean local CLI package manifest.
- `Analyze/duplex-mailbox/live-project/.lycheedev/live/duplex/CON-e35d54fd0e6347b37964cd096f5f854a/`:
  small-request journal, result and command/close write traces.
- `Analyze/duplex-mailbox/live-project/.lycheedev/live/duplex/CON-7017f059db1e824546f28444820fc703/`:
  1 MiB journal, result and command/close write traces.
