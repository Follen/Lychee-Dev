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

Other folders may exist on a machine (`_classic_era_`, `_anniversary_`,
`_beta_`). They are reported as **not served by the addon** and are deliberately
skipped. Do not install into them and do not tell the user they are supported.

The Titan folder is `_classic_titan_`. An earlier release used
`_classic_arena_`, which does not exist; that made an installed Titan client
invisible to both `install` and `doctor`. If a user reports "Titan was never
installed", check the folder name before anything else.

## The executable name does not identify the build

Every non-retail client ships as `WowClassic.exe`. `Wow.exe` is retail,
`WowB.exe` is beta, and `WowVoiceProxy.exe` is a helper, not a client.

The **install path** does identify the build, because each one lives under a
`_build_` directory:

```text
D:\Game\World of Warcraft\_retail_\Wow.exe          -> retail
D:\Game\World of Warcraft\_classic_\WowClassic.exe  -> classic
D:\Game\World of Warcraft\_classic_titan_\WowClassic.exe -> titan
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
classicEra   _classic_era_      v1.15.9.69547      not served by the addon
```

### `lycheedev instances`

Which clients are running right now, with the pid, window handle and build:

```text
running instances:
  [0] Retail         pid=62460    hwnd=0x12410c7a    "魔兽世界"

commands target this instance automatically
```

The `[index]` is what you pass to `--instance` and to `use`.

### `lycheedev use [index]`

Pin one running client as the default target, so later commands need no flag:

```text
lycheedev use        # the only running client
lycheedev use 1      # the instance at index 1
lycheedev use --clear
```

Only the build and the instance ordinal are stored, because pids and window
handles change on every launch. The pin therefore survives a restart: it
re-resolves against whatever is running now. If the pinned build is not
running, commands fail with a clear message instead of typing into the wrong
game.

## Choosing a target: precedence

1. `--hwnd` / `--pid` — explicit, for scripted use
2. `--instance <index>` — one command only, overrides the pin
3. the saved pin from `use`
4. `--client retail|classic|titan` — filter by build
5. the single running instance, when there is exactly one

Several instances of **different** builds are separated by `--client`. Several
instances of the **same** build are genuinely ambiguous: the CLI prints the
table and fails instead of guessing, because a wrong guess types a slash command
into the wrong game.

## Multiple installations

A machine can hold more than one WoW root. `--wow-root <path>` selects which one
to install into and is remembered in `~/.lycheedev/config.json`. `doctor`
reports the root it is using. Run `lycheedev clients --wow-root <other>` to
inspect a second root without changing the saved one.

## What cannot be done from outside the game

- The **character** on a running client is not readable without a
  build-specific memory reader. Nothing here guesses it. Two instances of the
  same build can only be told apart by pid, window handle or index.
- The **account** behind a running client is likewise not exposed. SavedVariables
  files are per account, and the newest file is not necessarily the live
  session's account when several accounts are configured.
- WoW ignores posted window messages, so keyboard delivery needs the window in
  the foreground. That is why targeting the right window matters.

If the user must be certain which client is which, ask them to run a trivial
in-game command (`/dev auto status <task>`) in the window they mean, or to close
the others.
