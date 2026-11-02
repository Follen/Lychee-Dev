# Secret values and secure taint

Pin the addon and matching client API source, starting at the reported consuming
operation. Preserve `SecretReturns`, `SecretPayloads` and `SecretArguments` as
written. Conditional taint permissions do not establish this caller's state;
an API marker does not prove this call produced a secret or that a consumer
forbids it. Older clients do not inherit Retail rules by assumption.

Select the containing Lua declaration and inspect supported flow context: fixed
steps, aliases, branches, conditions, rule identity and coverage. A guard applies
to its checked value/branch before reassignment; calling `issecretvalue` alone
proves no later operation safe. `bounded_complete` covers only the selected,
modeled closure. Unknown routing, aliases, dynamic calls and dependencies remain
open. Secret propagation and secure execution taint are separate.

No findings does not establish safety; a Blizzard-only stack does not identify
the cause. For an authorized runtime check, carry the missing link into a
[discriminating probe](live-probes.md#test-source-hypotheses). Do not coerce/extract
secrets, repeat known forbidden operations or replace protected Blizzard objects.
Recommend a correction supported by the actual value/consumer path.
