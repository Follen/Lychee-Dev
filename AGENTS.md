# Lychee Dev Development Guide

## Scope and reference

- This repository contains Lychee Dev, a lightweight graphical World of Warcraft data inspection addon.
- `/dev` lazily opens the main window. The primary workflow is to paste Agent-generated `/run` or `/script` Lua, execute it explicitly, inspect the result, and select the result text for copying.
- Lychee Dev keeps a bounded SavedVariables history of recent runs so users can reopen prior input and output without allowing unbounded database growth.
- Use [EllesmereUI](https://github.com/EllesmereGaming/EllesmereUI) as the primary reference for engineering discipline, module boundaries, UI consistency, taint avoidance, and performance. It is a reference, not a runtime dependency.
- Do not copy EllesmereUI branding, product names, media, or implementation wholesale. Adapt its principles to this addon's existing architecture and naming.
- Match nearby code before introducing a new helper, pattern, dependency, or abstraction. Keep every change focused and minimal.

## Supported client build

- Product: World of Warcraft Retail only.
- Expansion/patch baseline: Midnight 12.1 or newer within the 12.1 API line.
- Minimum supported TOC Interface: `120100`.
- Do not add Classic, Classic Era, Anniversary, PTR, Beta, or pre-12.1 compatibility branches unless the project owner explicitly changes the product scope.
- If the TOC temporarily lists `120000`, `120001`, `120005`, or `120007` so a pre-12.1 safety gate can load, those builds remain unsupported. The gate must return before migrations, database initialization, hooks, events, or feature code touch user state.
- Treat the addon's own `.toc` as the release source of truth. When changing the supported client, update the `.toc` and this section together, then verify replaced APIs and CVars against versioned Blizzard UI source.

Reference baseline last checked on 2026-08-19: EllesmereUI `v8.9.1`, commit `1b37158d7533deb2d5b0a74292438a8ea2191588`. Its public contribution rules target Midnight 12.1+, and its client gate uses Interface `120100` as the minimum supported value.

## Non-negotiable acceptance criteria

1. **Zero cost while disabled.** An opt-in feature must create no frames, register no events, run no hooks, and perform no polling until enabled. Build lazily on first enable and unregister or stop owned work when disabled.
2. **Zero behavior change without opt-in.** New user-facing features and settings default to off. Only a narrowly scoped bug fix may alter existing behavior without explicit opt-in.
3. **Low cost while enabled.** Prefer events and callbacks. Do not use `OnUpdate` polling or wall-clock timers as state/logic gates. Avoid allocations, repeated scans, redundant writes, and unbounded loops in hot paths.
4. **Zero taint risk.** Protected UI and Blizzard-owned objects are trust boundaries. A feature that cannot be made combat-safe and taint-safe must not ship.
5. **One supported API generation.** Write for Retail Midnight 12.1+. Do not retain old API fallbacks, old CVars, or version branches merely for speculative compatibility.

## Lua and file organization

- Use the WoW Lua 5.1 language subset. Do not use `goto`, labels, or syntax unavailable to the live client.
- Keep ordinary Lua source, comments, identifiers, and user-facing English strings ASCII. Locale data files may contain UTF-8 text and must be UTF-8 without BOM.
- Start addon modules with `local ADDON_NAME, ns = ...` when a shared namespace is needed. Keep implementation state local or under `ns`; create globals only for declared SavedVariables or a deliberate public API.
- Keep the `.toc` load order explicit: compatibility gate first when present, then libraries/framework, database and migrations, shared core, runtime features, and finally optional UI/options code.
- Put distinct features in distinct modules. Runtime behavior, options UI, migrations, localization, and reusable widgets should not be mixed into one catch-all file.
- Keep options and other heavy, infrequently used UI lazy or LoadOnDemand when the project structure permits it.
- Reuse the project's shared widget, styling, tooltip, popup, media, and layout helpers. Do not create a second visual system for one feature.
- Add comments only for non-obvious invariants, API hazards, ownership, combat restrictions, performance decisions, or migration rationale.

## Lifecycle and SavedVariables

- SavedVariables are not reliable at Lua file scope. Read, initialize, or migrate them only after the owning addon's `ADDON_LOADED` event.
- Defaults must fill missing keys without overwriting user values. Avoid persisting values that merely duplicate defaults when the database layer supports pruning them.
- Every schema migration must be versioned, idempotent, ordered before feature initialization, and safe to run more than once. Preserve unknown and newer data where possible.
- Never cache a profile table across profile switches or database re-rooting. Fetch the current profile at use time, or update the cached reference through the project's profile lifecycle.
- Disabling a feature must unregister its events, cancel its owned timers/tickers, release or hide its owned transient UI, and leave no active hook work beyond a constant-time disabled guard.

## Events, hooks, combat, and taint

- Use `HookScript` or `hooksecurefunc` for Blizzard-owned frames and functions. Never replace their scripts with `SetScript`.
- Do not write custom fields onto Blizzard-owned frame tables. Store addon state in external weak-key tables when it must be keyed by a Blizzard object.
- Do not hide, reparent, rename, or mutate protected frames during combat. Guard with `InCombatLockdown()` and defer required protected work until `PLAYER_REGEN_ENABLED`.
- Prefer addon-owned secure frames and documented secure templates/state drivers when secure behavior is required. Keep insecure configuration out of restricted execution paths.
- On Midnight APIs, check `issecretvalue` before comparing, formatting, indexing with, or branching on values that may become secret in combat. Fail open to a harmless visible state rather than throwing or leaving stale hidden state.
- Event handlers must filter early by event arguments or unit, avoid work when the feature is disabled, and unregister one-shot events immediately after use.
- A timer may debounce presentation work or defer to the next frame; it must not replace a real game event or become a perpetual polling loop.

## UI and localization

- Follow the existing addon's spacing, typography, colors, border treatment, control sizes, and interaction patterns. Find the nearest existing component and match its structure before creating a new one.
- Prefer project-owned tooltip and confirmation helpers for plain addon UI. Use `GameTooltip` directly only for native item, spell, unit, or hyperlink content.
- Build stable layouts: controls must not resize or shift when labels, values, hover states, or dynamic content change. Verify long localized strings at representative UI scales.
- Translate at the rendering boundary. Keep internal identifiers, routing keys, profile keys, and search identities stable and language-independent.
- Preserve positional format placeholders such as `%1$s` and `%2$d`; translators may reorder them. Missing translations must fall back safely to the source string.
- Visual changes require before/after screenshots and in-game inspection at the supported client build.

## Performance rules

- Keep disabled fast paths first and constant-time.
- Prefer memoized state changes over writing the same texture, alpha, text, point, or attribute every event.
- Avoid per-frame table/function creation, full-table scans, and repeated API calls in combat or high-frequency events. Cache immutable lookups and invalidate derived caches from concrete events.
- Use local aliases in genuinely hot paths only when profiling or call frequency justifies them; do not sacrifice clarity in cold code.
- Reuse persistent event drivers where ownership is clear. Do not create disposable event frames or unbounded tickers.

## Change and verification workflow

- Make one focused behavioral change at a time. Do not mix feature work with unrelated cleanup or formatting churn.
- Before editing, inspect the relevant `.toc`, initialization path, database defaults/migrations, module entry point, and nearest equivalent feature.
- For API or compatibility work, verify names and behavior against the exact supported Retail source/build; do not rely on memory or a latest-branch assumption.
- At minimum, review the diff and confirm every referenced file is listed in the correct TOC order. Run any repository-provided lint, syntax, tests, packaging, or validation commands relevant to the touched files.
- Test behavior both disabled and enabled. For stateful changes, also test `/reload`, logout/login persistence, profile switching, first install, upgrades from prior data, and reset-to-default behavior.
- For protected UI or combat-sensitive changes, test entering/leaving combat and relevant instance states with Lua errors enabled. Confirm there are no blocked actions, forbidden calls, or taint errors.
- Record the exact client version/Interface used for in-game testing. Never claim in-game verification when only static checks were run.
- Any visual change must include before/after evidence. Any localization-key change must regenerate and verify the repository's locale key index if one exists.

## Final review checklist

- New settings default off and disabled features have no runtime machinery.
- Enabled behavior is event-driven, bounded, and allocation-conscious.
- No Blizzard-owned scripts or frame tables are overwritten.
- Combat lockdown and secret values are handled before unsafe operations.
- SavedVariables initialization and migrations preserve user data.
- Lua remains 5.1-compatible and the TOC order is correct.
- Only Retail Midnight 12.1+ / Interface `120100` is presented as supported.
- Relevant static checks and in-game scenarios are reported accurately.
