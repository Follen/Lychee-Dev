# EmmyLua reconstruction v1

This is a **new** dependency patch series based on the public EmmyLua commit
`08b0873962e83bb8307e9a81052e3f41578fbc78` (MIT). It does not recover or claim
byte equivalence with the missing historical private BindingContext fork.

The complete archive, including upstream license and sources, is authenticated
before extraction. Tracked patches are applied in listed order. Preparation
verifies every final file against the tracked inventory. No upstream programs
or configuration are executed by preparation. The generated source is a build
input and must be included in corresponding-source distributions with its
identity receipt and this patch series; a network URL alone is not sufficient.

Preparation: `python3 third_party/emmy/prepare.py --archive /path/base.tar.gz`.
The default destination is `.tmp/emmy-reconstruction-v1/upstream`. An existing
destination is verified, never silently overwritten. `--download` explicitly
fetches the pinned archive to the supplied archive path when it is absent.

Implementation status and integration gates are recorded by the project's sole
current authority, `handover.md`. Availability of these APIs is not a claim that
all historical binding semantics or full-source acceptance are restored.
