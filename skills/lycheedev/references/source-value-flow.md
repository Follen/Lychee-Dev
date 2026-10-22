# Secret values and secure taint

Read for a concrete value, consuming operation or taint report. Pin the addon
and relevant client API source; start from the reported operation/location.
Preserve `SecretReturns`, `SecretPayloads` and `SecretArguments` as written.
Do not assume Forever follows Retail rules or lacks secrets because it is older.
Modes such as `AllowedWhenTainted` and `AllowedWhenUntainted` are conditional
permissions, not proof of this caller's state. An API marker identifies a possible
source, not that this call produced a secret or its consumer forbids one.

Use query/inspect for the actual path, and flow context only when supported and
relevant. Select the containing Lua declaration, not just the generated API.
Interpret steps, conditions, rule identity, boundaries and outer coverage.
`bounded_complete` covers only the selected closure and modeled operations.
Event payload metadata alone does not establish handler registration/routing;
more depth cannot resolve an unsupported edge.

Track assignments, fields, arguments and returns at fixed locations. A guard
applies to its checked value and branch before reassignment. Merely calling
`issecretvalue` proves no later operation safe. Check the consumer's versioned
restrictions; leave unknown aliases, dynamic calls and missing dependencies open.
Secret propagation and secure execution taint are separate. A Blizzard-only stack
does not identify who caused taint, and no findings is not proof of safety.

Source-only work stays static. When live work is authorized, carry the missing
link into [hypothesis testing](live-probes.md#test-source-hypotheses), then reconcile
the observation with this path. Do not coerce/extract secrets, repeat known
forbidden operations, mutate protected frames or replace Blizzard handlers.
Report a minimal correction only when supported; defaults/coercion are not a
generic secrecy fix.
