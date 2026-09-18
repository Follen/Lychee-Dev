# Clients, builds and instances

How to tell which World of Warcraft client you are driving, and how to target
one when several are running. Read this before any command that types into the
game (`send`, `run`, `bugs`, `capture`, `ack`).

## Supported builds

The addon ships one catalog per supported build. Nothing else loads it.

| Build | Version | Interface | Catalog | Install folder |
| --- | --- | --- | --- | --- |
| Retail | `12.1.0` | `120100` | `Lychee Dev_Mainline.toc` | `_retail_` |
| Classic | `5.5.4` | `50504` | `Lychee Dev_Mists.toc` | `_classic_` |
| Classic Titan | `3.80.2` | `38002` | `Lychee Dev_Wrath.toc` | `_classic_titan_` |
| WoW: Forever (无限服) | `1.60.1` | `16001` | `Lychee Dev_Forever.toc` | `_classic_beta_` or `_forever_` |

Other folders may exist on a machine (`_classic_era_`, `_anniversary_`,
`_beta_`). They are reported as **not served by the addon** and are deliberately
skipped. Do not install into them and do not tell the user they are supported.

`_classic_beta_` is the exception and must not be lumped in with those: it **is**
served when it holds WoW: Forever (see below). Never decide either way from the
folder name — read the client's own product code and build first.

The Titan folder is `_classic_titan_`. An earlier release used
`_classic_arena_`, which does not exist; that made an installed Titan client
invisible to both `install` and `doctor`. If a user reports "Titan was never
installed", check the folder name before anything else.

## A folder is a location, not an identity

Do not decide the build from the folder name. The launcher reuses a test folder
for whatever is on the test track, so **WoW: Forever currently ships under
`_classic_beta_`**, a folder that a MoP-era classic test client also uses. The
same folder can therefore be two different products over time.

The signal that actually identifies a client is the client's own files:

| Signal | Where | Example |
| --- | --- | --- |
| product code | `.flavor.info` in the flavor folder | `wow_forever`, `wow_classic_titan`, `wow` |
| build | `version.txt` in the flavor folder | `1.60.1.69893` |
| exe version | file metadata on `WowClassic.exe` | `5.5.4.69585` |

Resolution order the CLI uses, and that you should follow when reasoning about a
machine: product code first, then the build, then the folder default. So
`_classic_beta_` holding `1.60.x` / `wow_forever` is **Forever**, while the same
folder holding `5.5.x` / `wow_classic_beta` is the **Classic Beta** test track.

`_forever_` is also accepted, for installs that give Forever its own folder.

## The executable name does not identify the build

Every non-retail client ships as `WowClassic.exe`. `Wow.exe` is retail,
`WowB.exe` is beta, and `WowVoiceProxy.exe` is a helper, not a client.

So the executable name narrows a running process to *some* non-retail client and
nothing more. The install path narrows it to one install folder — but not to a
product, because a folder can be reused (see above). Only the client's own
`.flavor.info` product code and build settle which client it is:

```text
D:\Game\World of Warcraft\_retail_\Wow.exe                 folder default: retail
D:\Game\World of Warcraft\_classic_\WowClassic.exe         folder default: classic
D:\Game\World of Warcraft\_classic_titan_\WowClassic.exe   folder default: titan
D:\Game\World of Warcraft\_classic_beta_\WowClassic.exe    product decides: forever or classicBeta
```

Never infer the build from the window title. Retail's title is localized (for
example `魔兽世界`) and gives no build information.

## Commands

### `lycheedev clients`

Every build on disk, with its real version read from the executable, and whether
the addon is installed:

```text
retail       _retail_           v12.1.0.69587      addon installed
classic      _classic_          v5.5.4.69585       addon installed
titan        _classic_titan_    v3.80.2.69496      addon installed
forever      _classic_beta_     v1.60.1.69893      addon installed
classicEra   _classic_era_      v1.15.9.69547      not served by the addon
```

The `id` column already reflects the resolved product, so `forever` appearing
next to `_classic_beta_` is expected and is exactly the case a folder name alone
would get wrong. When the folder's own product code differs from the resolved
one, it is shown in brackets before the version.

### `lycheedev instances`

Which clients are running right now, with the pid, window handle and build:

```text
running instances:
  [0] Retail         pid=62460    hwnd=0x12410c7a    "魔兽世界"

commands target this instance automatically
```

The `[index]` is what you pass to `--instance` and to `use`.

### `lycheedev instances --identify`

Reads a small identity QR code from the top-left corner of each running window and
prints the character and build behind it:

```text
running instances:
  [0] Retail         pid=62460    hwnd=0x12410c7a    v12.1.0.69587
        character: 荔枝-白银之手  client: retail  build: 12.1.0
  [1] Forever        pid=71824    hwnd=0x2b30e1f4    v1.60.1.69893
        character: 测试小号-白银之手  client: forever  build: 1.60.1

commands target this instance automatically
```

The marker is not shown by default: it occupies the same corner as the completion
notice. Ask the user to run `/dev auto identify` **in each window they care about**,
then run this command. `/dev auto unidentify` hides it again.

Reading uses window capture (`Windows.Graphics.Capture`), so no window needs the
foreground and nothing is typed into the game. A window that shows no marker is
reported as having no identity rather than failing; that is the normal state until
the user asks for one.

Use this to turn an arbitrary index into a name the user can confirm. Tell the user
which character you are about to drive before you send anything.

### `lycheedev use [index]`

Pin one running client as the default target, so later commands need no flag:

```text
lycheedev use                       # the only running client
lycheedev use 1                     # the instance at index 1, when the builds differ
lycheedev use --character 荔枝-白银之手   # the window reporting that character
lycheedev use --clear
```

The pin stores the build and the instance ordinal, because pids and window handles
change on every launch. When the window showed its identity marker the character is
stored too, and that is the durable half: the pin then follows the character even if
the windows are reopened in a different order. It re-resolves against whatever is
running now, and if the pinned build is not running, commands fail with a clear
message instead of typing into the wrong game.

Pinning by name is how you choose **between windows of the same build**; see the
ambiguity rule below for the case where even that cannot separate them.

## Choosing a target: precedence

1. `--hwnd` / `--pid` — explicit, for scripted use
2. `--instance <index>` — one command only, overrides the pin
3. `--character <name>` — the window reporting that character (`Name` or `Name-Realm`)
4. the saved pin from `use`
5. `--client retail|classic|titan|forever` — filter by build
6. the single running instance, when there is exactly one

Several instances of **different** builds are safe to keep open: the install path
separates them, so `--client` picks one.

Several instances of the **same** build are distinguishable only when each window
reports a different character. Show the markers (`/dev auto identify` in each
window) and then either pass `--character <name>` or pin by name:

```text
lycheedev use --character 荔枝-白银之手
```

`use` probes the markers by itself when several windows share a build, because
reading them is capture-only and types nothing.

Two windows reporting the **same character and realm** — or two windows that show
no marker at all — are genuinely indistinguishable. Pinning an ordinal there would
only look precise while risking a slash command in the wrong game, so the CLI
refuses and names the pids to close:

```text
error: 2 Retail clients cannot be told apart
       close all but one, or close these pids: 62460, 71824
       or run `/dev auto identify` in each game window and then
       `lycheedev use --character <name>`
```

**In that case ask the user to close all but one window of that build.** Do not
invent a way to pick between them, and do not offer to pin one by pid.

## Multiple installations

A machine can hold more than one WoW root. `--wow-root <path>` selects which one
to install into and is remembered in `~/.lycheedev/config.json`. `doctor`
reports the root it is using. Run `lycheedev clients --wow-root <other>` to
inspect a second root without changing the saved one.

## What cannot be done from outside the game

- The **character** behind a window is not passively observable. It only becomes
  readable when the addon is loaded in that client and the user shows the identity
  marker (`/dev auto identify`), which `instances --identify` then reads. Without
  that cooperation, two instances of the same build can only be referred to by pid,
  window handle or index — never by name, and never guessed.
- Two windows showing the **same character and realm** stay indistinguishable even
  with the marker, because the marker content is identical.
- The **account** behind a running client is not exposed. SavedVariables files are
  per account, and the newest file is not necessarily the live session's account
  when several accounts are configured.
- WoW ignores posted window messages, so keyboard delivery needs the window in the
  foreground. That is why targeting the right window matters.

If the user must be certain which client is which, and the marker is unavailable,
ask them to run a trivial in-game command (`/dev auto status <task>`) in the window
they mean, or to close the others.
