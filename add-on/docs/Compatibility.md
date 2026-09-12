# Client compatibility

Lychee Dev ships one implementation behind three client profiles and three TOCs.

| Product | Version | Interface | TOC | Client profile | wow-ui-source commit |
| --- | --- | ---: | --- | --- | --- |
| Retail | 12.1.0 | 120100 | `Lychee Dev_Mainline.toc` | `Core/Clients/Mainline.lua` | `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` |
| Classic | 5.5.4 | 50504 | `Lychee Dev_Mists.toc` | `Core/Clients/Mists.lua` | `1028c1e687f721ba9d3af14d1b12a5745e4227c7` |
| Classic Titan | 3.80.2 | 38002 | `Lychee Dev_Wrath.toc` | `Core/Clients/Titan.lua` | `825d29d3662b372f0bead725ee6abd339e4a77b5` |

The `_Wrath.toc` filename is the client selector used by the Titan product. The metadata inside that TOC identifies the build as Titan and uses Interface `38002`.

## API evidence

The following APIs exist in all three selected official source snapshots. Line numbers are the API entry line in `Blizzard_APIDocumentationGenerated`.

| Capability | Retail 12.1.0 | Classic 5.5.4 | Titan 3.80.2 |
| --- | --- | --- | --- |
| Addon metadata | `AddOnsDocumentation.lua:165` | `AddOnsDocumentation.lua:154` | `AddOnsDocumentation.lua:154` |
| Mouse focus list | `InputDocumentation.lua:54` | `InputDocumentation.lua:30` | `InputDocumentation.lua:30` |
| Secret-value check | `FrameScriptDocumentation.lua:263` | `FrameScriptDocumentation.lua:233` | `FrameScriptDocumentation.lua:233` |

`Core/Compatibility.lua` owns addon metadata and mouse-focus boundaries. Feature modules must not add product checks or legacy API fallbacks.

## Events

Each TOC loads a catalog generated from its exact official source snapshot. Retail 12.1.0 exposes 1,782 documented events, Classic 5.5.4 exposes 1,483, and Titan 3.80.2 exposes 1,486. Event payload signatures also come from the selected build; the catalogs are not treated as interchangeable supersets. `RegisterEvent` still rejects unavailable events defensively, and the explicit `ALL` mode uses the running client's `RegisterAllEvents`.

## Dependency

`!BugGrabber` remains a required dependency on every build. The installed BugGrabber package must declare the current client Interface or the user must explicitly allow out-of-date addons. Lychee Dev does not embed, replace, or emulate BugGrabber.

## Verification

Run `powershell -ExecutionPolicy Bypass -File add-on/tests/TestAll.ps1`. It executes six functional suites under each client profile and then verifies Interface metadata, client-profile and event-catalog selection, shared TOC ordering, and referenced files.

Run `powershell -ExecutionPolicy Bypass -File add-on/tools/AuditCompatibility.ps1` for the full static audit. It isolates each TOC load closure, validates it against the locked official product commit, checks literal events against the matching generated catalog, and reports compatibility logic that escaped the shared boundary. The method and limitations are documented in [StaticCompatibilityAudit.md](StaticCompatibilityAudit.md).

For an API upgrade, update the three exact refs and commits in the audit tool, this file, the client profiles, generated event catalogs, README support matrix, and build tests together.
