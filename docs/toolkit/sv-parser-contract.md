# Observed legacy SavedVariables evidence and new parser target

This document records what the legacy Python reader and its offline fixtures
actually accepted. Those observations are research input for the unified
Toolkit; they are not a compatibility promise and do not require the Go
implementation to reproduce old interfaces or parser quirks. The new target is
a fresh, data-only reader for the LycheeToolkitDB namespace. This is a capability
contract, not an authorization rule or a completion claim.

## Evidence and scope

Primary sources:

- Lychee Dev skill/scripts/automation/saved_variables.py:1-432 — tokenizer,
  parser limits, database loading, ticket selection, evidence checks, and inner
  report checks.
- Lychee Dev skill/scripts/selftest_offline.py:59-348 — fixed report fixture,
  parser acceptance/rejection cases, nil handling, escape cases, and validation
  mutations.
- Lychee Dev skill/scripts/automation.py:238-283,309-375 — bounded retry and
  the sv read selection/validation sequence.
- docs/toolkit/design.md:40,217,273,281 — new namespace, old-data isolation,
  non-executing SV reads, and the 512 KiB report ceiling.

This contract is about the generated WoW SavedVariables data path. It does not
require evaluating Lua, accepting general Lua syntax, or importing old account
data into the unified Toolkit workspace.

## New target grammar

The game database entrypoint `bridge.ReadToolkitState` requires one new root;
the literal decoder beneath it never executes Lua. The root contract is:

~~~
document := ws* "LycheeToolkitDB" ws* "=" ws* table ws*
~~~

It rejects legacy or ambiguous roots and any value requiring evaluation. The
implemented literal decoder also accepts harmless line comments, semicolon
separators, named fields and single-quoted strings. These are literal syntax,
not compatibility aliases or executable Lua. Its table grammar is:

~~~
value := table | string | number | "true" | "false" | "nil"
table := "{" (field (("," | ";") field)* ("," | ";")?)? ws* "}"
field := "[" scalarKey "]" ws* "=" ws* value
       | name ws* "=" ws* value
       | value
scalarKey := string | finiteNumber | boolean
~~~

The Go value model preserves scalar key types, with finite float64 numbers
matching Lua's numeric key model and one-based numeric keys for array entries.
It never manufactures textual keys such as #1. Duplicate keys and duplicate
global assignments are rejected. A trailing separator is not an extra entry.

Target strings are double-quoted UTF-8 literals with the generated escapes
\", \\, \n, \r, \t, and decimal byte escapes \ddd for control/non-ASCII bytes.
Raw CR/LF, unknown escapes, invalid UTF-8, and unterminated strings are errors.
The target excludes the legacy \z whitespace extension. Standard Lua 5.1
simple escapes and escaped line endings are accepted as literal bytes. Numbers are signed decimal
integers or decimal fractions in the forms emitted by the new writer; hex,
named numeric values, operators, concatenation, calls, and parenthesized
expressions are not data.

## Observed legacy grammar

The legacy reader accepted a complete document consisting of zero or more
global assignments, with only space, tab, CR, and LF treated as whitespace
outside strings:

~~~
document   := ws* assignment* ws*
assignment := name ws* "=" ws* value
name       := [A-Za-z_][A-Za-z0-9_]*
~~~

Assignments are read in order. A later assignment replaces an earlier global
with the same name. A top-level nil removes the global instead of storing a
null value. The file loader requires strict UTF-8 before parsing; the in-memory
parser itself receives a string and measures its UTF-8 encoding for the file
limit.

~~~
value := table | string | number | "true" | "false" | "nil"

table := "{" (field (separator field)* separator?)? ws* "}"
field := "[" value "]" ws* "=" ws* value
       | value (ws* "=" ws* value)?
separator := "," | ";"
~~~

The second field form is either a keyed field (value = value) or a bare array
entry. Bare entries are assigned 1-based array positions independently of
keyed fields. The Python representation exposed those positions as string keys
#1, #2, and so on. A keyed field with a non-string key was exposed as # followed
by the parsed value's string form. These are observations about the old Python
object model, not requirements for the typed Go model.

The parser accepts both comma and semicolon separators, mixed in one table, and
accepts a trailing separator. Empty tables are valid. Whitespace may occur
around delimiters and separators. There is no comment production: comments,
even ordinary Lua -- comments, are not part of this observed grammar.

### Numbers

The actual numeric token expression is:

~~~
-?\d+\.\d+(?:[eE][+-]?\d+)?
|-?\d+(?:[eE][+-]?\d+)?
|-?\.\d+
~~~

Integers without a decimal point or exponent become integers; all other accepted
forms become floating-point values. Thus 1, -1, 1e2, 1.5, -1.5e+2, and -.5 are
accepted. +.5, .5, 1., hexadecimal, and named numeric values are not accepted
by the actual value dispatch/tokenizer combination.

### Strings

Only double-quoted strings are accepted. The decoded value is accumulated as
bytes and then decoded as UTF-8. Literal non-ASCII UTF-8 is allowed by the
in-memory parser and strict UTF-8 is required when loading a file. Raw CR or LF
inside a string is rejected.

Accepted escapes are:

| Escape | Meaning |
| --- | --- |
| \", \\, \n, \r, \t | The corresponding quote, backslash, or control character |
| \a, \b, \f, \v, \' | The corresponding Lua 5.1/simple escape |
| \ddd | One to three decimal digits, greedily consumed, for one byte 0..255 |
| \z | Skip all following space, tab, CR, and LF without adding bytes |

Decimal escapes are not required to have three digits. \12x decodes to byte 12
followed by x; \65bc decodes to Abc; \065 decodes to A. Values above 255,
unknown escapes, dangling backslashes, invalid UTF-8 after byte assembly, and
unterminated strings are errors. The decoded string byte limit is checked after
escape expansion and after literal UTF-8 bytes are appended.

## New target budgets

The new parser must reject malformed or over-budget data before unbounded
allocation or recursion. The report budget is fixed by the Toolkit design:

| Budget | New target |
| --- | --- |
| Report content | 512 KiB maximum; 384 KiB is the default target budget. |
| Whole SavedVariables document | An explicit bounded budget sized for the generated Toolkit database and bounded history; do not inherit the legacy 256 MiB value. |
| Table depth, entry count, decoded strings, and JSON structure | Explicit budgets chosen from generated-shape and memory measurements; each exceeded budget is a visible data/limit error. Do not inherit legacy parser constants merely for compatibility. |

The parser must be data-only and fail closed. A valid partial report must carry
an explicit incomplete/truncated state; a budget failure must not be reported as
a complete report. The 512 KiB limit applies to UTF-8 report content, not to a
claim about the entire SavedVariables document.

## Observed legacy limits

The old implementation used the following limits. They are useful evidence for
fixture sizing and threat review, but are not new Go requirements:

| Limit | Legacy value | Observed behavior |
| --- | ---: | --- |
| Whole file | 256 * 1024 * 1024 bytes | load_database reads at most limit+1 bytes and rejects any file larger than the limit; exactly the limit is allowed. |
| Recursive value depth | 200 | A value at depth greater than 200 is rejected. The root value starts at depth 0. |
| Total table entries | 2,000,000 | One global tokenizer counter covers all tables and is checked before each table-entry attempt. |
| One decoded string | 64 * 1024 * 1024 bytes | Reject after decoded/assembled UTF-8 bytes exceed the limit; exactly the limit is allowed. |
| One report content | 1 * 1024 * 1024 UTF-8 bytes | Evidence verification returned a failure when payload.content exceeded the limit. |

One old implementation detail was that a trailing separator caused the next
loop iteration to observe } and consume an entry-count check before accepting
the table. Record this only as an observed quirk; the new parser should count
actual typed fields/elements under its own meaningful budget.

The old parser treated malformed syntax as an error, not a partial result, and
rejected calls, parenthesized expressions, operators, concatenation, unknown
identifiers, and function syntax. The new parser should retain the data-only
security boundary without inheriting the old token set.

The adjacent file-read retry is also bounded but is not parser grammar:
automation.retry_read invokes the loader at most max(1, attempts) times, sleeps
only between failed attempts, and returns the last error after exhaustion. The
CLI derives attempts as max(1, retries + 1) (automation.py:324-332).

## New root and exact report selection target

The new SavedVariables root is LycheeToolkitDB only. The new reader must not
read, migrate, alias, or merge LycheeDevDB or DumperDB. It should reject a
missing root and must not silently choose another top-level global.

The new writer/reader schema must expose one explicit report map keyed by the
requested report identity. Selection is an exact key lookup in that map:

~~~
LycheeToolkitDB.<explicitReportMap>[requestedReportID]
~~~

The exact map name and new report envelope belong to the Toolkit schema; they
must not be inferred from the legacy LycheeDevDB path. A missing exact key is an
error even if another report exists. Never select by array order, insertion
order, timestamp, status, metadata, newest/first record, or a history list.
The selected report's identity must be checked again against its envelope and
content before it is accepted.

## Observed legacy root and selection

The old loader parsed the complete file, then required LycheeDevDB to be a table
(saved_variables.py:261-273). The old selector used:

~~~
LycheeDevDB.exports.records[requestedTicket]
~~~

The old ticket pattern was ^LYCHEE-[A-Za-z0-9-]+$; exports and exports.records
had to be tables, and the exact ticket key had to hold a record table
(saved_variables.py:276-290). A missing exact ticket failed even if another
record existed. This is evidence for exact identity selection, not a nested
schema or interface to reproduce.

The old CLI also refused paths whose basename was not exactly Lychee Dev.lua or
whose path ended in .bak (automation.py:309-320). The new Toolkit does not
inherit the old sv path command; new evidence is selected through its explicit
workspace/archive identity.

## New target report validation

After exact selection, the new reader must validate the selected typed record
and fail closed. The report content limit is 512 KiB, with no legacy 1 MiB
allowance. Identity, schema, completeness, truncation, and checksum fields
must be defined by the new Toolkit result/capture protocol; malformed or
partial data must never be presented as a complete report. Unknown fields may
be retained as data, but unknown executable forms must be rejected.

The new root and report values are typed Go data: string keys remain strings,
integer keys remain integers, and array elements remain array elements. No
stringification convention such as #1 is part of the target.

## Observed legacy evidence validation

The old reader validated a selected record in this order; these fields and
schema names are evidence for the rewrite, not new interfaces to preserve:

1. The record envelope must have schema lychee.evidence.v1 and a ticket equal
   to the requested ticket. If supplied by the caller, createdAt must also
   match.
2. payload must be a table with string content, media type application/json,
   and encoding utf-8. Its byteCount must be an integer (not a boolean) equal
   to the UTF-8 byte length of content. Content above 1 MiB is a failure. If
   payload.content is missing or not a string, the legacy verifier returns
   immediately with that failure.
3. metadata must be a table. Supplied taskId, executionId, and revision must
   match it. It must contain result schema lychee.automation.result.v1, terminal
   status one of succeeded, failed, cancelled, or interrupted, boolean complete,
   checksum algorithm adler32, and an eight-character lower-case hexadecimal
   contentChecksum. Recompute Adler-32 over the UTF-8 content bytes and compare
   the eight-digit lower-case result. Adler-32 is an integrity check here, not
   an identity or authentication mechanism.
4. Parse payload.content as a JSON object. Require schema
   lychee.automation.result.v1, a terminal status, boolean complete, and a
   non-empty object environment. If complete is false, incompleteReasons must be
   an array. requestType must be task or bug. For bug, params must be an object
   with integer, non-boolean count and string scope. For task, params may be
   absent, but when present it must be an object.
5. When expected identity values are supplied, the inner report must match
   taskId, executionId, revision, and requestType. When metadata is supplied,
   its taskId, executionId, revision, status, and complete values must agree
   with the inner report.

The old checks are useful negative-test material. The new implementation should
define its own result/capture fields and identity checks under the Toolkit
protocol; it must not import the old evidence schema as an interface.

## Fixed observed samples and new-target cases

The following legacy samples are fixed observations from
Lychee Dev skill/scripts/selftest_offline.py. They are intentionally small; the
full fixture at lines 104-143 is not reproduced here. They are not a demand to
keep the old root, representation, or limits.

### New-target cases

The new fixture set should additionally include:

~~~lua
LycheeToolkitDB = {
  ["reports"] = {
    ["REPORT-1"] = {
      ["content"] = "{}",
      ["byteCount"] = 2,
    },
  },
}
~~~

This represents the new namespace and typed string keys. A report is selected
only by the explicit requested report identity. A fixture using LycheeDevDB,
DumperDB, a second top-level global, semicolon separators, #1-style keys, or
\z must be rejected by the new grammar. A report of exactly 512 KiB is within
the new report budget; 512 KiB plus one UTF-8 byte is over budget. The new
implementation should also exercise its chosen document, depth, entry, string,
and JSON budgets at exact-limit and limit-plus-one boundaries.

### Observed legacy accepted samples

~~~lua
LycheeDevDB = { ["a"] = 1, ["b"] = nil }
DumperDB = nil
~~~

Result: only LycheeDevDB.a remains; the b key and DumperDB global are absent.
This is the real-client trailer shape tested at lines 202-221.

~~~lua
LycheeDevDB = {
  ["schemaVersion"] = 8;
  ["exports"] = {
    ["order"] = { "LYCHEE-20260912-180000-0001", },
    ["records"] = {},
  },
}
~~~

Result: semicolons, commas, trailing separators, empty tables, and bare array
entries are all accepted. The actual report fixture uses bracketed string keys,
booleans, integers, a floating-point value, JSON content with escapes, and an
empty array (selftest_offline.py:106-143).

String assertions required by the self-test:

~~~
\12x   -> byte 12 + "x"
\65bc  -> "Abc"
\065   -> "A"
\0     -> one NUL byte
\9     -> one tab byte
\z <ws> y -> "y"
UTF-8 bytes escaped as consecutive \ddd sequences -> the original UTF-8 text
~~~

### Observed legacy rejected samples

The following must fail rather than execute or partially recover:

~~~lua
x = (function() end)
x = os.execute("evil")
x = "unterminated
x = "bad \q escape"
x = "\256"
x = "\999"
x = "\300"
~~~

The self-test also requires rejection of a missing exact ticket, a mutated or
missing payload mediaType, encoding, or byteCount, a bad/missing checksum or
checksum algorithm, missing/invalid metadata result schema, non-terminal status,
non-boolean complete, an over-limit content string, invalid inner JSON, an inner
report with the wrong schema/status/environment/request type, and bug params
missing integer count or string scope (selftest_offline.py:174-348).

## Non-goals for the replacement

- Do not evaluate Lua or use an embedded Lua runtime for this data path.
- Do not read arbitrary legacy SavedVariables files as a fallback; use the
  LycheeToolkitDB root and explicit new workspace/evidence identity.
- Do not infer a report from history, exports.order, timestamps, or the newest
  record.
- Do not turn a bounded retry, a changed file timestamp, or a completion notice
  into proof that the selected report is valid; selection and all checks above
  are still required.
