---
name: wowdev
description: Use when working with the Lychee Dev WoW addon to read persisted runtime evidence by Ticket or prepare bounded /run memory/CPU investigations, object inspection, and event monitoring. Do not use for generic WoW API research or unrelated addon implementation.
---

# WoW Dev

Use this skill as the operational guide for Lychee Dev, an in-game evidence workbench. The desired output is either (a) an evidence-backed diagnosis from SavedVariables or (b) a command/workflow the user can paste into WoW and report back.

## Choose the workflow

- **Persisted evidence:** locate the WoW `WTF` SavedVariables file, find the exact Ticket, and read the complete payload.
- **Run:** open `/dev`, use the Run page, and paste `/run` or `/script` Lua.
- **Memory/CPU investigation:** use Run with a question-specific script; read [references/runtime-investigations.md](references/runtime-investigations.md) before designing a probe.
- **Objects:** open `/dev` > Objects and inspect a global/object path or use the mouse picker.
- **Events:** open `/dev` > Events, search the current client's catalog, select events, start monitoring, reproduce the behavior, then stop and save.

The Performance page, automatic capture, health scan and function benchmark have been removed. Run, Objects, Events, Trace, Errors and Saved Records remain available. Do not route a new investigation to the removed UI. There are no separate `/object` or `/event` slash commands. Do not invent them; give the `/dev` UI steps instead.

## Read SavedVariables

1. Search the user's WoW installation for the global SavedVariables file, normally `WTF/Account/<account>/SavedVariables/Lychee Dev.lua`. The exact account/realm path is installation-specific. Use a filename search or `rg` for `LycheeDevDB`/the Ticket rather than assuming a path.
2. Treat the file as data, not trusted code. Do not `dofile` or execute an unknown SavedVariables file in the host environment. Use a restricted Lua parser/sandbox or inspect the relevant assignment safely.
3. For a Ticket `LYCHEE-...`, read:

   ```text
   LycheeDevDB.exports.records[TICKET]
   LycheeDevDB.exports.records[TICKET].payload.content
   ```

   Validate `schema == "lychee.evidence.v1"`, and use `source.kind`, `source.title`, `source.path`, `environment`, and `createdAt` to identify the record. Validate the report schema inside the payload separately; record-level schema/source do not belong to `metadata`. Prefer the exact Ticket already supplied to the owning task, including directly delivered messages; do not ask the user to resend it to several tasks. The payload is the authoritative complete report; `metadata` is bounded context, not a replacement for it.
   Current kinds include `run_result`, `object_snapshot`, `object_node`, `event_log`, `function_trace`, and `error_log`. Legacy `performance_*` records remain valid historical evidence; read their complete payload without requiring the deleted module or discarding them.
4. `LycheeDevDB.exports.order` is newest-first. Recent ad-hoc runs are in `LycheeDevDB.history` and may contain `code`, `result`, and a bounded `tree`; they are not the same as a saved export. Old installations may contain `DumperDB`, which the addon migrates into `LycheeDevDB`.
5. A record created in-game is only in memory until `/reload`, logout, or exit. If the user has not completed one of those writes, explain that the Ticket cannot yet be found on disk. Exports and history are bounded and older entries can be pruned.

When reporting persisted evidence, quote the Ticket, source kind, client environment, and the exact payload path. Do not claim that a record is absent until the likely SavedVariables locations and the exact Ticket have been searched.

## Run commands

The runner accepts either `/run` or `/script` prefixes and evaluates Lua 5.1 code. Prefer returning a compact structured report; use `print` for short scalar messages. Check the installed runner and serializer limits before delivery. A returned value is captured immediately, so an asynchronous script needs a verified completion/export writer; do not return an unfinished table and assume later mutations will be saved.

```text
/run print(GetBuildInfo())
/run return { addon = MyAddon, profile = MyAddonDB and MyAddonDB.profile }
/script return UIParent and { name = UIParent:GetName(), shown = UIParent:IsShown() }
```

Give commands that are explicit and bounded. Avoid unbounded table walks, per-frame polling, network calls, protected UI mutations, and writes to SavedVariables unless the user explicitly requests them. Lychee Dev truncates large run output, and combat blocks the workbench and active inspection/monitoring tools.

One complete investigation does not mean pasting a bundle of implementation source. Use Run for a small probe; for large source-dependent studies, use a verified independent diagnostic carrier and a short entry command, with its installation/loading and cleanup explained. Never assume WoW can read arbitrary host files or hot-load Lua. When the user asks for one paste, cover the automatic matrix from one practical entry point and one final report; list external actions and missing coverage honestly. For a requested check, include the exact pasteable command, what result to expect, and what field or error the user should report back. If a value may be secret in combat, tell the user to leave combat before running it and do not suggest comparing or formatting that value.

## Object inspection

Use the Objects page for live object state. Valid paths are `_G`, dot-separated identifiers, numeric indexes, and quoted string indexes, for example:

```text
_G.MyAddon
MyAddon.db.profile
_G["MyAddon"]["db"]
UIParent
```

The page can search globals or fields, expand tables, show frame overview/children/regions, and save an object snapshot or node. The mouse picker temporarily hides the window; press `F` or `Enter` to capture the object under the cursor, `Esc` to cancel. If a path is not known, use a short run command to return the candidate global, then inspect that path in Objects.

Never promise a complete object graph: inspection is bounded, cyclic tables are marked, and secret combat values stop the read. Protected Blizzard frames must not be mutated as part of a diagnostic command.

## Event monitoring

Use the Events page, not a guessed slash command:

1. Search for the uppercase event name in the catalog shown for the running client (for example `PLAYER_TARGET_CHANGED`).
2. Select one or more events and click **Start Monitoring**.
3. Reproduce the behavior, click **Stop Monitoring**, inspect payload arguments, and use **Save** if the evidence must be handed to an Agent.

Event names and payload signatures are build-specific. The supported profiles are Retail `12.1.0`/Interface `120100`, Classic Mists `5.5.4`/`50504`, and Classic Titan `3.80.2`/`38002`; do not transfer a signature between profiles without checking the catalog. `ALL`/“monitor all events” is an explicit diagnostic mode, not a default recommendation. Monitoring is bounded (newest 500 records; at most 16 arguments per record, each shortened), and it stops on combat entry.

## Delivery and ownership

Use available computer controls only within the authorized workflow. Inspect their actual capabilities before attempting game input; a browser-only tool cannot type into WoW. After a failed input attempt, verify focus/state once and use one manual paste when reliable native input is unavailable. Do not loop speculative key presses.

If the user authorized clipboard delivery, announce the exact script/version, write it, and read back the clipboard to verify equality. Keep that version stable while awaiting execution. A revision needs an explicit replacement notice and a new verification; another task's clipboard, game session, or pending probe is not yours to replace.

Do not deploy addon changes or run destructive Provider/storage experiments merely to diagnose a problem. Preserve the current task's decision scope: a request for a proposal remains a proposal; an authorized implementation can proceed. A past project's memory target or plan-only decision is not a universal skill restriction.

## Handoff format

For a command request, answer with:

1. **Paste:** the exact `/run` or `/script` text, or the `/dev` page and fields to use.
2. **Reproduce:** the short action sequence and when to start/stop monitoring.
3. **Report back:** the expected output fields or the Ticket to copy after saving and reloading.

For a Ticket request, return the record's `source.kind`, client/build, creation time, payload path, and a concise evidence-based finding. Keep observations separate from hypotheses, and state when the evidence is truncated, pending disk write, client-specific, or blocked by combat/secret values.
