# Lychee Dev Toolkit

**WoW source research, game data, asset export and live investigation in one native toolkit.**

[![npm](https://img.shields.io/npm/v/lycheedev)](https://www.npmjs.com/package/lycheedev)
[![CI](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-ci.yml/badge.svg?branch=main)](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-ci.yml)
[![Windows amd64](https://img.shields.io/badge/platform-Windows_amd64-0078d4)](https://github.com/Follen/Lychee-Dev)
[![Zero runtime dependencies](https://img.shields.io/badge/runtime_dependencies-0-52a788)](https://www.npmjs.com/package/lycheedev)
[![MIT](https://img.shields.io/badge/license-MIT-52a788)](https://github.com/Follen/Lychee-Dev/blob/main/LICENSE)

[English documentation](https://github.com/Follen/Lychee-Dev#readme) · [简体中文](https://github.com/Follen/Lychee-Dev/blob/main/README_zhCN.md)

## For developers

| Workflow | Capabilities |
| --- | --- |
| Source | Pin Blizzard UI source; search APIs, symbols and paths; compare revisions and validate addon closures. |
| Data | Query DB2 and SQL; inspect spells, items, creatures, encounters, decor and independent Hotfix records. |
| Assets | Search listfiles; inspect CASC files; export raw files and BLP2 images as PNG or lossless WebP. |
| Live | Use the in-game `/dev` workbench or bounded Lua probes with archived reports and explicit cleanup. |
| Evidence | Reuse fixed project references, verify captures and manage bounded caches. |

Windows x64 and Node.js **≥22.14.0** are required. The CLI is native Go; the Node launcher forwards arguments and exit codes. There are no runtime npm dependencies or required install scripts.

```powershell
npm install --global lycheedev --ignore-scripts
lycheedev version --format json
lycheedev describe --format json

$release = Join-Path (npm root -g) 'lycheedev'
lycheedev addon install --release $release --installation 'D:\Game\World of Warcraft\_retail_'
lycheedev skill install --release $release --path (Join-Path $env:USERPROFILE '.agents\skills\lycheedev')
```

The last two commands explicitly deploy the included resources for first installation. npm installation alone does not deploy them. For upgrades, inspect component status and use a recovery directory on the same drive; see the [installation guide](https://github.com/Follen/Lychee-Dev#install).

Source and data investigation do not require a running game. Live investigation requires a logged-in client with the addon loaded. Encrypted records may be unavailable; bounded Hotfix pages and static validation retain their completeness limits.

## For Agents

The included `lycheedev` skill routes source, data, assets, validation and running-client questions. Automatic invocation is enabled; discover the installed command surface through `lycheedev describe --format json` and carry returned fixed references between calls.

**A visible QR is an intermediate state.** Within an authorized live investigation, read and retain the verified report, ACK the same operation, then hide the final receipt and verify it cleared. Recover interruptions by the existing operation ID. Respect an explicit pause or report-retention request.

See the [Agent guide](https://github.com/Follen/Lychee-Dev#for-agents) and [skill](https://github.com/Follen/Lychee-Dev/blob/main/skills/lycheedev/SKILL.md).

## Package contents

- `native/windows-amd64/lycheedev.exe` — native CLI.
- `bin/lycheedev.mjs` — platform selection and process forwarding.
- `payload/addon/`, `payload/skill/` — deployable addon and Agent skill.
- `release.json` — component sizes and SHA-256 digests checked by the installer.
- `LICENSE`, `THIRD_PARTY_NOTICES` — licensing and third-party attribution.

The launcher never downloads fallback binaries. The toolkit uses `~/.lycheedev` and does not import retired wowdoc/wowdata homes or legacy SavedVariables. Uninstalling npm does not remove deployed resources; component removal is explicit.
