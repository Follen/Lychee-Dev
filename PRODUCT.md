# Lychee Dev Toolkit

<!-- impeccable:product-schema 1 -->

## Platform

World of Warcraft addon UI using the Lua Frame API, with a Windows amd64 CLI.
This is an in-game desktop interface, not a website or a mobile application.

## Users

WoW addon developers and agents investigating official Blizzard source, common
third-party addon source, game data and a running game client. A player may keep
the game open while an agent performs an authorized investigation.

## Product Purpose

Provide one CLI, in-game workbench and agent skill for source research, game data,
assets and live investigation, with fixed references and inspectable evidence.

## Operating Context

The workbench has Run, Objects, Events, Trace, Diagnostics, Export Records,
Automation and About pages. Settings controls reduced motion and dedicated
receiver bindings. The small Agent command receiver is independent of the main
workbench. Optical receipts remain at the top left of the game window.

## Capabilities and Constraints

The repository architecture and live safety contracts in `AGENTS.md` and
`docs/toolkit/design.md` remain authoritative. UI changes preserve the existing
capabilities, opt-in investigation resources, bounded lifetimes, protected UI
boundaries, paired English and Simplified Chinese copy and exact operation
identity. Retail, Classic and Titan are the acceptance targets; code support is
not proof of interactive acceptance on every client.

## Brand Commitments

Keep the Lychee Dev name and existing lychee logo. The incumbent design uses a
dark surface, warm readable text and restrained lychee red. The owner explicitly
requests rounded outer corners, readable text and a usable Agent receiver, and
has rejected the oversized, sparse settings layout and confusing navigation.

## Evidence on Hand

The owner supplied real-client screenshots showing the earlier sharp-corner UI
and the later rounded but still unsatisfactory Settings page. Source and offline
fixtures are available; visual acceptance requires actual client observation.
No screenshot or test fixture proves runtime bridge completion by itself.

## Product Principles

- Make common inspection tasks and their current state immediately readable.
- Let agents complete the full authorized workflow, including report cleanup.
- Keep verified results, uncertain execution and pending cleanup distinct.
- Retain evidence and surface partial or truncated results honestly.
- Use the game UI without altering unrelated player bindings or protected state.

## Open Decisions

The next layout is being revised under the owner's instruction to continue
autonomously. No permanent image-first or code-first workflow preference has
been established. The custom WoW platform is outside Impeccable's web/mobile
platform presets; its general Operate and craft guidance applies.
