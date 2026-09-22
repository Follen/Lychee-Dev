# lycheedev

Unified native World of Warcraft development toolkit. One npm package carries the
native `lycheedev` CLI for five platforms (`native/<os-arch>/`) plus the deployable
Lychee Dev addon (`payload/addon/`) and the `lycheedev` agent skill
(`payload/skill/`).

## Requirements

- Node.js `>=22.14.0`. The launcher only selects the packaged platform binary and
  forwards `argv`/stdio/exit codes; the toolkit itself is a native program.
- No runtime npm dependencies. No install scripts are required for use.

## Use

```text
npm install --global lycheedev --ignore-scripts
lycheedev version --format json
lycheedev describe --format json
```

The addon and the skill are deployed only by explicit CLI commands from the
installed package, never as a side effect of `npm install`:

```text
lycheedev skill install --release <installed-package-directory> --path <parent>/lycheedev
lycheedev addon install --release <installed-package-directory> --installation <client-directory>
```

Uninstalling the npm package does not remove deployed addon or skill copies;
removal is an explicit command that only touches files the toolkit owns.

The launcher never downloads fallback binaries and never reads legacy tool homes
(`~/.wowdoc`, `~/.wowdata`). Unsupported platforms fail with a precise error.

## Contents

- `bin/lycheedev.mjs` — platform selector and process forwarder.
- `native/<os-arch>/lycheedev[.exe]` — windows-amd64, linux-amd64, linux-arm64,
  darwin-amd64, darwin-arm64.
- `payload/addon/`, `payload/skill/` — deployable resources listed in
  `release.json` (`lycheedev.release.v1`) with byte sizes and SHA-256 digests.
- `release.json` — release manifest consumed by the launcher and the installer.

Third-party components and their licenses are recorded in `THIRD_PARTY_NOTICES`.
Package licensing terms are recorded in `LICENSE`.
