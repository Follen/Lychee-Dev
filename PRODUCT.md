# Lychee Dev

<!-- impeccable:product-schema 1 -->

## Platform

World of Warcraft addon UI using the Lua Frame API, delivered together with a
Windows amd64 native CLI, a first-party Agent Skill and an npm package. This is
an in-game desktop interface, not a website or a mobile application.

## Users

WoW addon developers and agents investigating official Blizzard source, common
third-party addon source, game data and a running game client. A player may keep
the game open while an agent performs an authorized investigation.

## Product Purpose

Provide one CLI, in-game workbench and Agent Skill for source research, typed game
data, assets and live investigation, with fixed references and inspectable
evidence. Source, data and asset work stand on their own; live work uses the
existing addon and Lycheedev mailbox protocol v1.

Author publishing is a separate Rust CLI capability for CurseForge, NewBeeBox,
NetEase DD, Heybox and ModUs. It covers each platform's supported queries,
plugin/configuration/WA writes, versions, media, relationships, projects and
selective cloud backups. Current implementation gaps are listed in [todo.md](todo.md).
The selected command's schema explains authentication, inputs, effects and recovery.
Original artifacts are reused; uncertain writes are recovered by reading their
original target. Publishing does not require game Live access.

## Operating Context

The workbench has Run, Objects, Events, Trace, Diagnostics, Export Records,
Automation and About pages. There is no Settings surface. The Agent uses native
memory and the existing 500-slot Mailbox installation independently of the
workbench. A compact activity indicator remains at the top left.

## Capabilities and Constraints

The architecture, public contracts and live safety boundaries in
[DESIGN.md](DESIGN.md) and the working rules in [AGENTS.md](AGENTS.md) are
authoritative. UI changes preserve the existing capabilities, opt-in
investigation resources, bounded lifetimes, protected UI boundaries, paired
English and Simplified Chinese copy and exact operation identity.

The addon selects six runtime profile families from
`addon/Core/ClientGate.lua`: Retail, Classic, Classic Era, Titan, Forever and
Anniversary. Retail has two admitted interface baselines. The six supported
products include formal Forever: canonical ID and Source family `forever`,
official remote product and installation flavor `wowf`, client directory
`_forever_`, executable `Wow.exe`. The two Forever beta products are outside the
current support scope. Historical pins, captures and recovery records retain
their original identities. Actual implementation and verification status is
recorded in [handover.md](handover.md); interactive acceptance requires the client.

## Brand Commitments

Keep the Lychee Dev name and existing lychee logo. The incumbent design uses a
dark surface, warm readable text and restrained lychee red. The owner explicitly
requests rounded outer corners, readable text and a usable Agent receiver, and
has rejected the oversized, sparse settings layout and confusing navigation.

## Evidence on Hand

The owner supplied real-client screenshots of earlier workbench iterations; the
current addon implements the accepted layout. Source and offline fixtures are
available; visual acceptance requires actual client observation. No screenshot or
test fixture proves runtime bridge completion by itself.

## Product Principles

- Make common inspection tasks and their current state immediately readable.
- Let agents complete the full authorized workflow, including report cleanup.
- Keep verified results, uncertain execution and pending cleanup distinct.
- Retain evidence and surface partial or truncated results honestly.
- Use the game UI without altering unrelated player bindings or protected state.

## Open Decisions

No new UI capability is authorized by this document; new features are agreed
with the owner before implementation. Visual acceptance still requires the real
client.
