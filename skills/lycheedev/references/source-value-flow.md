# Secret values and secure taint

Read for a concrete value, consuming operation or taint report; ordinary API
lookup does not require a value-flow investigation.

For these questions, start from the reported operation, value or source location.
Pin both the addon revision and the relevant client API source; inspect the exact
generated declarations and their conditions. Preserve metadata such as
`SecretReturns`, `SecretPayloads` and `SecretArguments` as written. Do not assume
Forever follows Retail rules or lacks secret values because it is an older client.
Known `SecretArguments` modes such as `AllowedWhenTainted` and
`AllowedWhenUntainted` describe conditional permission, not a static proof of
the caller's taint state. Keep the raw mode in the finding and classify the
path as possible until runtime conditions are observed.
An API marker can establish a possible source; it alone does not prove that the
reported call produced a secret value or that the consuming operation forbids it.

Use the installed capabilities reported by `describe --format json`. Query and
inspect can support a source-based explanation; inferred calls and LuaLS types
alone are not a verified value-flow analysis. Request `source context --flow`
only for a concrete value/operation question. Interpret returned steps,
conditions, rule identity and unknown boundaries; do not claim a proven
propagation path when the result has only static candidates or incomplete coverage.

Select the Lua declaration containing the reported path, not merely the generated
API declaration. For third-party code, a bounded entry is:

```text
lycheedev source context --snapshot <addon-pin> --symbol-id <lua-symbol-id> --environment <client-pin> --flow --depth 1 --limit 50 --max-lines 120 --format json
```

`flow.coverage.state=bounded_complete` applies only to the selected closure and
modeled operations. Read `flow.boundaries` and the outer coverage reasons as well
as findings. Event payload metadata is retained, but handler registration and
payload routing are outside the current flow scope; inspect that missing link
directly or carry it into a bounded live hypothesis. A larger depth cannot resolve
an unsupported edge.

Trace the relevant value through assignments, table fields and function arguments
or returns, preserving each step's original location. Confirm symbol identity and
loading context before crossing files. A guard applies only to the checked value
on the relevant branch before reassignment; merely calling `issecretvalue` does
not establish that later operations are safe. Check the consuming operation's
versioned restrictions before reporting a violation. Mark dynamic calls, unknown
aliases, missing dependencies and truncation as gaps rather than inventing edges.

Keep secret-value propagation separate from secure execution taint. A stack made
entirely of Blizzard frames is not sufficient to identify who caused the taint;
static hook/write candidates need further evidence. Source-only research does
not authorize game input. For deep investigation with live work in scope, carry
the fixed source locations, suspected value path, conditions and missing links
into [the live hypothesis workflow](live-investigation.md#test-source-hypotheses).
Continue with existing error evidence and a bounded check that distinguishes the
remaining explanations; do not stop at an unresolved static path when live can
answer the next question. Feed observed conditions back into the source analysis.

Keep proposed checks within the same trust boundary: observing a value does not
authorize writing protected frames or Blizzard tables, replacing secure handlers,
or extracting a secret through coercion. Combat state and permitted argument
conditions belong to the hypothesis, not assumptions used to make it pass.

Finish with the supported path and restriction, a minimal correction when
justified, and any conditions still requiring verification. Distinguish observed
runtime evidence, source deductions and unresolved hypotheses. Do not treat no
findings as proof of safety, or propose coercion/default substitution as a generic
way to remove secrecy. When the path cannot be established, state the missing
link and the evidence needed to resolve it.
