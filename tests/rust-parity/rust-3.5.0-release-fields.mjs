// Approved independent Rust release identity; Go 3.2.2 oracle blobs stay unchanged.
// Only these two complete file identities are accepted, never a path/regex whitelist.
export const CANDIDATE_VERSION = "3.5.0";
export const CANDIDATE_DIST_TAG = "beta";
export const CANDIDATE_RELEASE_FIELDS = Object.freeze([
  {
    "path": "addon/Core/Runtime.lua",
    "expected": {
      "bytes": 2615,
      "sha256": "5dcbf55e3018a1b29ee4e92368f34283dadf277b3346289cd14fd1287bcbbc28"
    },
    "retained": {
      "bytes": 2615,
      "sha256": "e43dd852d0447774215da803391bf7a130c384d2265b4cb402e1baa8bcebc0bd"
    },
    "expectedText": "ns.Release = \"3.2.2\"",
    "retainedText": "ns.Release = \"3.5.0\""
  },
  {
    "path": "addon/Lychee Dev.toc",
    "expected": {
      "bytes": 2005,
      "sha256": "73573ae8336ceac7505032aa2403804313a8d06ff69c68b1d8044fc81f303833"
    },
    "retained": {
      "bytes": 2005,
      "sha256": "7d2594d5676dbe8e10c7a644873cd2e00b8cb26f4a539eb19ebbc81a1595d09c"
    },
    "expectedText": "## Version: 3.2.2",
    "retainedText": "## Version: 3.5.0"
  }
].map(row => Object.freeze({...row, expected: Object.freeze(row.expected), retained: Object.freeze(row.retained)})));
