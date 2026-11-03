# Source interpretation

Use fixed Toolkit Source for WoW API/addon research by default. Reuse matching
pins and returned locations. Discover a missing repository/track or revision
through the CLI; a named addon release, repository track and client build are
separate identities. Third-party `main` source can use a separately pinned client
API environment. A Blizzard source pin supplies its own matching environment.

A declaration and its conditions can settle an API question. Follow relations
or request context only when callers, loading or value propagation matter.
Query hits are candidates; resolve same-name collisions by repository, commit,
path and scope, and retain the returned symbol ID. Generated API declarations
are contracts, not necessarily implementation bodies.

For empty or insufficient results, check topic, terms, requested version and
coverage before treating the source as unavailable. Use external sources when
the user asks for them or a concrete source/version/capability gap remains.
Keep that evidence and version separate from the pin. Follow verified paths and
returned URLs; do not guess raw GitHub paths or silently replace the source.

Confirmed semantic edges, static candidates and dynamic unknowns have different
strengths. Mapping completeness does not prove a TOC/XML load closure. Client
Family/Game conditions, missing dependencies and runtime bindings can leave
loading partial. Inspect the actual selected TOC, included XML and client
environment when compatibility is the question. LuaLS types and syntax checks
do not establish behavior in a running client.

Each query, refs or context cursor belongs to its original source, environment,
filters and budgets. Continue only when another page can change the answer.
Changing that scope starts without the old cursor. A last page does not resolve
unreadable files or unsupported dynamic edges. A source diff's line movement
alone is not an API break; inspect the changed declaration or implementation.

## Secret values and secure taint

Pin the reported addon's revision and relevant client API source. Preserve
versioned `SecretReturns`, `SecretPayloads` and `SecretArguments` conditions.
A potential source does not prove the call produced a secret, and permission
conditional on taint does not prove the caller's taint state.

For value flow, select the Lua declaration containing the reported operation.
Follow the same value through assignments, fields, arguments and returns;
confirm identity and loading before crossing files. A guard covers the checked
value on its branch until reassignment. Read bounded-flow coverage and unknown
boundaries; increasing depth does not resolve an unsupported edge.

Secret propagation and secure execution taint are distinct. A Blizzard-only
stack does not identify the source of taint. Dynamic aliases, missing event
routing and dependencies remain open links. When Live is authorized, carry the
smallest unresolved hypothesis into a [bounded probe](live-probes.md). Observe
secrecy markers and permitted conditions; do not coerce secrets, replace secure
handlers or write protected objects to make an observation possible.
