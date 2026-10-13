# Isolated XML extraction prototype

`emmy_xml_program` is an opt-in example under `emmy-prototype`. It does not
replace Source, `facts.rs`, or `xml_inline.rs`. It reads only its explicit
input file and its compiled-in schema companion. It never opens Include or
Script targets, executes Lua, loads a project config, or enters the game.

The companion is the exact Retail `UI.xsd` from source commit
`31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`. Its identity is sealed in
`xml-companion-manifest.json`; the original 4,036-document input manifest is
unchanged. The schema supplies handler names from `ScriptsType`,
`ActorScriptsType`, `AnimScriptsType` and `AnimGroupScriptsType`, including
`PreClick`, `PostClick` and animation/actor handlers. It supplies no implicit
parameter catalog.

## Data contract

- All spans are zero-based UTF-8 byte intervals `[start,end)` into the original
  file, including a UTF-8 BOM. The input is never rewritten.
- `occurrences` preserve source order, repeated Include edges, Script file
  declarations, object attributes, structural ancestry, handler registration,
  and separate `function`/`method` attribute references. Generic `xml_element`
  entries retain anonymous element ancestry without inferring their object type.
- Top-level Script bodies are unwrapped. Each handler has a syntax-only
  `return function(...)` envelope. `self`, `event`, and `button` are **not**
  injected or resolved. Wrappers are explicitly synthetic and retain anchors.
- Decoded and compiler inputs retain segmented source maps: exact bytes,
  entity expansion, physical CR/CRLF normalization, and synthetic bytes.
  Numeric/predefined entities are decoded; CDATA delimiters are omitted.
  Multiple raw segments stay separate. A covering range is never labeled an
  exact token range. Reverse projection keeps the same relation.
- Emmy's fixed parser exports syntax names and call expressions only, never
  binding facts. Invalid Lua exports no name/call facts. Parser acceptance is
  not authoritative Lua 5.1 compilation: the known raw-parser label gap remains.
- `ScriptSource::compiler_input()` and `XmlProgram::compiler_inputs()` provide
  exact bytes for the existing pinned Lua 5.1.5 runtime. `compiler_line()` maps
  line-only diagnostics and always reports `columnKnown=false`.

Malformed structure, unknown entities, DTDs, processing instructions, prefixed
elements, unsupported encodings/versions, unknown Scripts handlers, and nested
script markup fail transactionally. Attribute values normalize physical XML
whitespace before decoding references. This is a supported-profile extractor,
not an XSD validator; `coverage.schemaValidation` records that distinction.

Input is bounded to 8 MiB, decoded source to 16 MiB aggregate, map segments to
200,000 aggregate, occurrence count to 20,000, script count to 4,096, XML nesting
to 128, and name/call facts to 20,000 per script. Output serialization uses an
8 MiB bounded buffer and writes only a complete report. The 30-second parser
deadline is cooperative; the parent verification runner must supply a hard
process timeout/memory bound, especially for the upstream Lua parser.

## Verification entry points

The implementation agent does not build or run Cargo, the example, or luac.
Each new revision is **not_run** at handoff; owner-run historical evidence is
recorded below. Compilation goes through the governed build admission channel,
and all example/luac execution goes through an owned native process Job. From
the repository root, the owner selects fresh evidence directories and uses:

```text
node tools/rust-cargo.mjs --owner source-xml-build -- build --offline --locked -p lycheedev-source --features emmy-prototype --examples
node tests/rust-source-engine/xml-owned-probe.mjs .tmp/rust-emmy-xml-NEW <successful-retired-build-receipt-id>
node tests/rust-source-engine/xml-wellformed-owned.mjs .tmp/rust-emmy-xml-wellformed-NEW .tmp/rust-emmy-xml-NEW
node tests/rust-source-engine/xml-boundary-owned.mjs .tmp/rust-emmy-xml-retail-NEW retail .tmp/rust-emmy-xml-NEW
```

Replace `NEW` and the receipt placeholder with the new run's identities. Never
overwrite a previous failed run or invoke an unsealed executable directly.
`xml-owned-probe` verifies the retired successful build receipt, seals matching
EXE/PDB hashes, and owns the golden plus actual fixed-luac children in a 2 GiB /
30-second Job. `xml-wellformed-owned` preserves the independent six negative
expectations. `xml-boundary-owned` provides the 90-second outer Retail bound and
retains original per-file failures. Its `line` scenario also owns the real CR
compiler/mapping comparison. The existing build-cache budget and ownership
rules apply; no separate target directory or bypass of admission is implied.

`golden` checks the immutable 204-byte example, exact Run/event/button ranges,
entities, newline normalization, synthetic provenance, ordered declarations,
duplicate Include occurrences, root Script, PreClick/PostClick, identifiers
split by entities/CDATA, string/comment negatives, malformed XML, and invalid
Lua. It emits `luacActualRun=not_run` regardless of parser outcomes. A separate
supervised pinned-luac invocation must consume each returned `compiler.text`
as UTF-8 stdin without a shell/code wrapper and record its real exit/status.
That runner can reuse the existing `Runtime::check_with_control` interface;
the prototype itself deliberately launches no child process.

`xml-golden-204.xml` expected original intervals:

| Item | Original bytes |
| --- | --- |
| Run | `[92,95)`, `[123,126)`, `[159,162)` |
| event / button | `[96,101)` / `[163,169)` |
| quote entities | `[65,71)`, `[75,81)` |
| emoji | `[71,75)` |
| newline entity | `[82,87)` |

TOC execution phases, object instantiation, template/mixin value identity,
implicit handler parameters, name/member bindings, types, diagnostics that
depend on those bindings, and four-client catalogs remain separate unpassed
work. `semanticComplete` is always false. This prototype establishes extraction
and mapping inputs for that work, not a complete WoW analysis engine.

## Full Retail XML attempt

The owner recorded XML01 passing 23 golden checks and six actual fixed-luac
stdin compilations under the original native Job 2 GiB / 30-second wrapper.
That evidence applies to XML01. The following new modes are **not_run** at
handoff and require a newly sealed binary/receipt.

```text
emmy_xml_program retail <absolute-manifest> <absolute-source-root>
emmy_xml_program line <absolute-xml-file> <zero-based-script-index> <one-based-compiler-line>
```

`retail` requires the exact original manifest SHA256
`7c2ba06f96d302621f9f9f0f505e0f66fd978e1509325119ba769ba511b1934a`, the pinned
commit above, and document-path/SHA256/size digest
`88d68f2c4bdb352245cbcb53cd41ec906b294e2ea599ee6374e00be169be7928`.
It verifies all 4,036 original files' complete bytes first, including the
2,551 Lua files / 23,354,176 Lua bytes. It then re-reads and rechecks each of
the 1,091 XML files immediately before extraction. The bounded file/path
validation logic follows the already reviewed `emmy_library_probe` loader;
Include/Script target declarations never cause file loading.

This first full attempt deliberately preserves the Ui-only root profile.
Bindings or other roots will remain explicit unsupported records. Before the
owner runs it, their actual counts and first failure remain **not_run**.
Do not broaden the profile or replace the first failed evidence with a later
success. The source change only makes root rejection more specific.

Every expected XML file has a report row. Successful extraction rows retain
source identity, counts, script kinds, and hashes of the complete program,
all decoded/compiler text and source maps, all syntax facts, and all ordered
occurrences. The hash encoding is compact deterministic serde_json UTF-8 with
no final newline. Full programs are released after each file. Per-file hashes
allow reconstruction from the pinned source bytes; aggregate row hashes cover
the entire ordered file report. Invalid Lua remains `syntax_failed` and still
retains its full program/map/fact hashes. Failed extraction exports no partial
facts. Unsupported/failed/not-run rows retain path, original bytes/hash, reason
and bounded error detail; none are silently excluded. `firstFailure` is stable
in original manifest order. At most 24 expanded failure samples are retained,
with total counts and a visible truncation flag. Syntax samples retain at most
three failing scripts and two errors per sampled script.

Limits are 128 MiB verified document input / 256 MiB maximum two-pass source
reads; the existing 8 MiB single-file limit; 128,000 retained scripts; 64 MiB
retained decoded script bytes; 2,000,000 retained decoded+compiler map segments;
2,000,000 retained syntax name/call facts; 1,000,000 retained occurrences; and
512 MiB aggregate complete-program serialization hashed with a 64 MiB per-file
streaming hash limit. Remaining aggregate capacities are passed into extraction;
a file that cannot fit fails transactionally and remaining files are recorded
as not_run. One bounded program is resident at a time. Report serialization is
still capped at 8 MiB. The whole attempt has an 80-second cooperative deadline;
the owner must keep the 90-second hard outer bound and native Job memory cap.
`state=passed` requires all 1,091 files extracted and Emmy-parsed within budget.
It still leaves real full-Retail luac compilation `not_run` and
`semanticComplete=false`.

## Preserved entity-CR line mapping counterexample

`xml-entity-cr-top.xml` contains exactly
`<Ui><Script>local x = 1&#13;local = 2</Script></Ui>`.
`xml-entity-cr-handler.xml` places the same body inside Frame/Scripts/OnLoad.
The owner preserved an actual failure in `rust-emmy-xml-cr-red-02`: fixed luac
reported line 2 (top-level) / line 3 (handler), while the old LF-only map returned
outside_input / the synthetic suffix. The expected original spans are `[28,37)`
and `[44,53)` respectively. The preceding CR-red-01 harness filename failure
also remains preserved; it is not a compiler/mapping result.

The focused fix now scans the unchanged compiler bytes using Lua 5.1.5
`llex.c:129` `inclinenumber`: CR or LF advances one line; adjacent opposite
bytes CRLF or LFCR form one advance; repeated CRCR or LFLF form two advances.
Entity CR stays CR, as required by XML decoding. `line` still calls the same
public mapping method and always reports `columnKnown=false`. Source-map
projection still includes every segment on the compiler line, including a
synthetic terminator if present; the covering raw span remains separate.
This mapping change advanced `xmlRuleRevision` to `xml-map-v2`. That fixed
revision is included in snapshot identity, every full program, and each mode's
report, so mappings produced by different rules cannot share the old identity.

The original 23 golden checks and their four exported fixture programs remain
unchanged. Eight additional checks cover both actual CR regressions plus bare
CR, bare LF, paired CRLF, paired LFCR, repeated CRCR and repeated LFLF. The exact
`xml-newline-contract.xml` fixture stores all six forms as entity references;
assertions check unmodified compiler text, exact original line spans, unknown
columns and the absence of an extra line. The owner subsequently recorded
XML03 passing all 31 golden checks and six fixed-luac compilations, plus both
actual CR regressions passing in `rust-emmy-xml-cr-fixed-01`. No XML lexical,
handler catalog, root-profile or nested-markup behavior changed in that batch.

## BOM cursor correction, rule revision 3

The first full Retail attempt remains preserved: all 4,036 inputs verified;
1,091 XML files attempted; 999 extracted; 84 unsupported (76 handlers and eight
roots); eight extraction failures (seven nested-markup, one unbalanced).
Independent review verified that all eight extraction failures used a UTF-8
BOM and contained legal self-closing elements. They were extractor defects.

The focused correction translates every quick-xml numeric `buffer_position`
back into original bytes through one checked cursor adapter: add three only
when the original input begins with the UTF-8 BOM. Borrowed event/attribute
spans already address original bytes and receive no extra adjustment. Empty
element identity now comes directly from `Event::Empty`, never a guessed
`/>` at a numeric cursor. Opening ranges, whole-script end positions and
synthetic closing anchors all use the corrected original-byte coordinates.
`xmlRuleRevision=xml-map-v3` participates in every snapshot identity/report.

Two exact fixtures, `xml-empty-control.xml` and `xml-empty-bom.xml`, contain
identical XML after the optional three-byte prefix. They cover an empty Script,
an external Script, Include, Frame with empty Size/TexCoords children, a handler
body, a self-closing Frame and a top-level Script body. Nine new golden checks
assert prefix identity, exact opening/end ranges, ancestry, complete decoded
and compiler text, token positions and both synthetic anchors. The preceding
31 checks and original four exported fixture programs remain unchanged, giving
40 checks total. This revision is **not_run** at implementation handoff.

The next controlled full-Retail attempt should retest the same eight failures
without changing any input hashes. The 84 unsupported records remain expected;
this batch does not expand handler/root support or repair the separate XML
lexical-validation gap. The only unrelated edits collapse two Clippy-reported
nested conditions without changing their behavior.

## Complete pinned handler catalog, rule revision 4

The owner recorded revision 3 passing XML04's 40 golden checks and six actual
fixed-luac compilations; strict Clippy also passed on the same source hash.
Full Retail XML02 verified all 4,036 files and attempted all 1,091 XML files:
1,003 extracted, 88 unsupported and zero extraction failures. All eight BOM
structural failures disappeared. Four of those files then exposed missing
handler support, producing 80 unsupported-handler files plus eight Bindings
roots. The original reports remain preserved.

Revision 4 extracts the four handler classes from the unchanged pinned XSD:
75 declarations in `ScriptsType`, six in `ActorScriptsType`, six in
`AnimScriptsType`, and seven in `AnimGroupScriptsType`. Handler recognition is
the union of explicit declarations, not an `On*` naming rule. Unknown handler
names are still rejected. No input, root support, XML lexical rule, compiler
text, parameter injection or semantic binding behavior is changed by this batch.

Each handler registration now carries schema provenance: the handler's catalog
declarations and original XSD byte spans, XML owner element and its XSD type,
type extension chain with source spans, and the Scripts container's source.
The container is found through its local declaration or an explicit element
reference/substitution chain. For example, LineTranslation inherits Scripts
from `LineTranslationType → TranslationType → AnimationType`, while Frame uses
the global `Scripts → LayoutField` substitution. Anonymous nested complex types
do not accidentally contribute declarations to the outer named type.

This remains extraction rather than complete XSD validation. Owner resolution
is recorded as `matched`, `unknown_owner_element`, `owner_scripts_unresolved`
or `known_owner_mismatch`. A known handler name can be extracted while its
owner context is unproven or mismatched; that state is explicit in every
registration and aggregate Retail counts. It is not a schema-validity claim.
Implicit handler parameters remain uninjected and `semanticComplete=false`.

`xml-handler-catalog.xml` is an exact fixture covering the six previously
missing names: OnModelLoading, OnPlay, OnPause, OnStop, OnFinished and OnLoop.
Twelve additional golden checks assert class counts, full body text and byte
ranges, six exact owner/type/inheritance/declaration proofs, Frame substitution
provenance, unchanged parameter handling, unknown-name rejection and visible
owner mismatch. The preceding 40 checks and original four exported fixture
programs remain unchanged, giving 52 checks total. `xmlRuleRevision=xml-map-v4`
participates in snapshot identity and reports. This revision is **not_run** at
implementation handoff; the next actual Retail report must retain all remaining
unsupported or failed files, including the eight Bindings roots.

## XML lexical gate, rule revision 5

The owner recorded revision 4 passing XML05's 52 golden checks and six actual
fixed-luac compilations, plus strict Clippy. Full Retail XML03 verified the same
4,036 files and attempted 1,091 XML files: 1,083 extracted, zero structural
failures and exactly eight unsupported Bindings roots. These reports and sealed
EXE/PDB artifacts remain preserved.

Revision 5 adds the focused lexical checks from
[XML 1.0 Fifth Edition](https://www.w3.org/TR/2008/REC-xml-20081126/#sec-common-syn):
element and attribute names use the complete NameStartChar/NameChar ranges,
including supplementary Unicode. Raw `<` is rejected inside attribute values;
an escaped `<` remains valid. Literal `]]>` is rejected only in ordinary Text,
so comments, CDATA and properly escaped/split text retain their distinct rules.
Outside the document element, only space, tab, CR and LF are accepted. A leading
UTF-8 BOM remains valid, while extra or trailing BOM/NBSP are rejected. An XML
declaration must begin immediately after the optional initial BOM. These checks
operate on original bytes without rewriting entity-produced CR or source maps.

The independent runner's six malformed input strings are unchanged and appear
as six additional golden checks. `xml-lexical-valid.xml` covers Unicode element
and attribute names, namespace attributes, an escaped attribute `<`, raw `<` in
CDATA, legal comment text, CDATA/entity segments decoding to `]]>`, and all four
outside-root whitespace characters. Additional checks cover Unicode range gaps,
invalid initial name characters, the leading BOM and declaration placement.
The previous 52 checks remain unchanged; 13 additions give 65 checks total.
The original four exported fixture programs remain unchanged.

`xmlRuleRevision=xml-map-v5` enters snapshot identity and reports. This revision
is **not_run** at implementation handoff. Bindings, prefixed-element support,
full namespace/XSD validation, implicit parameters and semantic bindings remain
outside this batch. Any new real-source rejection must retain its first actual
evidence; do not weaken the XML rules or remove files to obtain a green result.

## Bindings extraction profile, rule revision 6

The owner recorded revision 5 passing XML06's 65 golden checks, 25 actual
fixed-luac/mapper checks, the six independent malformed-XML checks and strict
Clippy. Full Retail XML04 verified the same 4,036 files and attempted 1,091 XML
files: 1,083 extracted, zero structural failures, and eight unsupported Bindings
roots. The first compile failure and its subsequent corrected build remain
preserved; this batch retains that correction.

Revision 6 recognizes `sourceProfile=Bindings` independently of `Ui`. The profile
and `xmlRuleRevision=xml-map-v6` both enter snapshot identity. This is extraction
of observed source declarations, without claiming that UI.xsd validates Bindings.
The existing XML lexical, cursor, newline and entity rules apply unchanged.

The Bindings profile records the root and its direct Binding/ModifiedClick
children in source order, retaining all decoded attribute values and their raw
value ranges. Complete element ranges include paired closing tags. Duplicate
Binding names are retained, including names containing spaces; a binding name
does not declare a Lua global. Empty and self-closing Binding declarations
remain metadata without a generated script. Nonempty decoded bodies, including
whitespace-only bodies, retain their bytes rather than being trimmed. Each body
records `binding_body_raw`, decoded maps and compiler maps. ModifiedClick is
metadata only. Unknown direct children, nested markup and executable text
outside Binding bodies fail closed. This observed profile does not infer missing
required attributes, execution order, effective overrides or load relationships.

Binding bodies use the syntax-only `return function(...)` envelope. Comments
inside a body contribute no text or inserted whitespace. CDATA and entities
use the existing segmented map, physical CRLF becomes LF and an entity-produced
CR remains CR. No `self`, `event`, `button` or `keystate` parameters are injected.
At pinned commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`,
`Interface/AddOns/Blizzard_APIDocumentationGenerated/KeyBindingsDocumentation.lua`
bytes `[5533,5688)` document InputCommandCallback's non-nil cstring `keystate`;
there is no proven reference connecting that callback to the complete native
XML Binding signature. Name/call facts remain syntax facts, including a local
variable named keystate; no global-binding conclusion is exported.

The independent fixed-source inventory found eight Bindings files totaling
184,481 raw bytes: 1,033 Binding declarations, 1,025 nonempty bodies, eight empty
WoWHack declarations and 77 ModifiedClick declarations. The files have no BOM,
CR, CDATA or entity references. There are 322 bodies with `runOnUp=true` and
keystate reads, with 328 keystate spelling occurrences. Only one of 66 XML
comments is inside a body: Mists TOGGLECHARACTER, bytes `[41694,41872)`. These are
source observations to compare against the next actual run, not claimed results
from revision 6. Retail reports now compute `bindingFiles`, `bindingRawBytes`,
`bindingDeclarations`, `bindingScripts` and `modifiedClicks` from extraction;
complete program hashes include the profile, attributes and body metadata.

`xml-bindings-394.xml` is exactly 394 UTF-8 bytes with physical CRLF, no BOM and
SHA-256 `aca963840dc4e0201609be4443c35f4985199d72556500c95e6957db18c16e96`.
Twenty additional golden checks cover the profile, declarations, duplicate and
space-containing names, empty bodies, ModifiedClick, root metadata, exact body
and token spans, entity/CDATA splits, physical LF versus entity CR, comment gaps,
a fake declaration inside a comment, local keystate shadow syntax and rejected
structures. The first Run maps to `[75,76) + [76,82) + [91,92)`; the second maps
to `[107,108) + [116,118)`. The previous 65 checks and original four exported
fixture programs remain unchanged, giving 85 checks. The independent compilation
runner can extract this new fixture and compile its two exact compiler inputs.

Revision 6 is **not_run** at this implementation handoff. Only the isolated
example, this fixture and this document changed in this batch. Default Source,
upstream engine code, semantic binding resolution and existing runners are
outside the batch. Run through the same bounded owner-controlled build/probe
workflow above, with fresh sealed artifacts and no replacement of prior evidence.

## Root character data evidence, rule revision 7

The owner recorded revision 6 passing XML07's 85 golden checks and 27 actual
fixed-luac checks, plus strict Clippy. Full Retail XML05 retained a real failure:
all 1,091 XML inputs were verified, 1,090 extracted, one failed, none were
unsupported. The failing Commentator file was not malformed XML. It contains
ordinary character data after the TOGGLE_SMART_CAMERA_LOCK closing tag:
`</Binding>>`. Revision 6's invented root-text restriction discarded the entire
file's 57 Binding declarations. That original failure remains preserved.

Pinned source `Interface/AddOns/Blizzard_Commentator/Bindings.xml` has 8,797 bytes
and SHA-256 `0044e6a9881e3a151b4def1e746e930d80352cfc48752c75fa19bd7378bb7b2c`.
The extra `>` is at raw bytes `[1132,1133)`. Its complete Text event occupies
`[1132,1136)` and is exactly `>\n\n\t`. No source bytes are repaired.

Revision 7 retains non-whitespace Text events directly under Bindings as
`unexpected_profile_text` occurrences. `raw` and `sourceText` preserve the full
original Text event; the `binding_root_text` diagnostic identifies the bounding
non-whitespace subrange. The diagnostic has `severity=warning`,
`validationState=unresolved` and `nativeRuntimeBehavior=unknown`, and points to
the final ordered occurrence index. It does not assert that the game ignores,
rejects or executes this text. These occurrences never become ScriptSource,
name facts or call facts. Unknown child elements, nested body markup and
non-whitespace ModifiedClick bodies continue to be rejected.

Coverage now distinguishes `inputCoverage=complete_original_document` and
`extractionComplete=true` from `profileValidationPassed=false`. A diagnostic
sets `profileValidationState=unresolved`; a diagnostic-free Bindings document
passes only the observed extraction profile, with native validation explicitly
not performed. Ui profile validation remains `null`/`not_performed`. Per-file
Retail summaries retain the typed diagnostics and the original text occurrences.
Aggregate counts include `profileDiagnostics` and `filesWithProfileDiagnostics`.
When extraction and syntax parsing finish but profile diagnostics remain, the
Retail report has `extractionState=passed` and
`state=extracted_with_profile_diagnostics`, while `profileValidationPassed=false`.
Exit 0 acknowledges complete extraction, not profile validation. A validator
must inspect these fields rather than treating extraction completion as approval.

One existing golden expectation changes under this corrected contract. Its
exact input `<Bindings>Run()</Bindings>` is retained, but now must yield zero
scripts, a preserved `Run()` occurrence and a diagnostic at `[10,15)`, complete
extraction and failed profile validation. The other 84 previous expectations
remain unchanged. The new 90-byte `xml-bindings-root-text.xml` fixture
(SHA-256 `a565f2446b7a976fd8f42b6acdc6e56808c7b2dc4d80a6d8a74f10a68e0cee5d`)
contains two Binding declarations separated by the actual `</Binding>>` pattern.
Seven added checks assert exact bytes, both declarations and bodies, full text
and significant spans, parent ownership, diagnostic fields and summary visibility,
plus diagnostic-free fixture behavior. There are 92 golden checks total.

Revision 7 is **not_run** at implementation handoff. The next actual Retail
expectation is complete extraction of 1,091 XML files, including eight Bindings
files / 184,481 bytes / 1,033 declarations / 1,025 scripts / 77 ModifiedClicks,
with the Commentator root-text diagnostic still visible and profile validation
not passed. The separately observed declaration/attribute lexical counterexamples
are outside this batch and remain unresolved; this revision does not claim full
XML well-formedness validation. The original sources and prior reports remain
unchanged.

## Declaration and attribute framing, rule revision 8

The owner recorded revision 7 passing XML08's 92 golden checks, 29 actual
fixed-luac checks and strict Clippy. Retail XML06 verified all 4,036 documents
and extracted/Emmy-parsed all 1,091 XML files, with no failed, unsupported,
excluded or not-run files. The exact eight-file Bindings inventory matched
184,481 bytes, 1,033 declarations, 1,025 bodies and 77 ModifiedClicks. Its one
Commentator diagnostic remained at `[1132,1133)`, with the complete `[1132,1136)`
Text event retained and profile validation false/unresolved. The harness passed
the expected-diagnostic contract; this was not full native validation approval.

Revision 8 addresses the independent declaration suite whose six real failures
were retained in `.tmp/rust-emmy-xml-declaration-red-02`. The unchanged inputs are:

```xml
<Ui a="1"b="2"/>
<?xml version="1.0" standalone="maybe"?><Ui/>
<?xml version="1.0" other="x"?><Ui/>
<?xml version="1.0" version="1.0"?><Ui/>
<?xml version="1.0" standalone="yes" encoding="UTF-8"?><Ui/>
<?xml version="1.0" encoding="UTF-8" broken?><Ui/>
```

A shared borrowed-byte scanner now consumes the complete quoted-field sequence.
Each attribute requires an XML S separator (space, tab, CR or LF), an equals
sign with optional XML S around it and a matching single or double quote pair.
Trailing XML S is allowed; trailing junk, missing separators, missing equals,
unquoted/unclosed values and duplicate element attributes are rejected. Existing
Unicode-name, raw `<`, entity decoding, XML attribute whitespace normalization
and raw value ranges remain in force. The scanner does not rewrite source bytes.

The declaration must have the exact `<?xml` opener and `?>` closer. Its first
field is `version='1.0'`; an optional UTF-8 encoding field precedes an optional
standalone field whose value is exactly `yes` or `no`. Encoding value spelling
is case-insensitive. Duplicates, unknown fields, wrong order, references in
pseudoattribute values and unconsumed declaration tails are rejected. The
existing UTF-8/XML-1.0-only extraction policy remains narrower than all XML
encodings/versions. A declaration still must begin immediately after the
optional initial BOM. This implements the relevant XML 1.0 productions for S,
Eq, XMLDecl, VersionInfo, EncodingDecl, SDDecl and attribute framing; it does not
claim full XSD, namespace, native Bindings or runtime validation.

The previous 92 golden expectations remain unchanged, including revision 7's
explicitly corrected root-text expectation. Twelve new checks give 104 total:
the six exact negative inputs above, six legal declaration variants in one
check, each of the four legal element-attribute separators in one check, the
Unicode/BOM fixture and its unchanged Lua byte range, 19 additional negative
framing cases in one check, and a failure after a previously parsed Lua body
to verify transactional extraction. The negative matrix includes missing first
version, unsupported version/encoding, adjacent declaration fields, duplicate
encoding/standalone, uppercase standalone, pseudoattribute references, missing
quotes/equals, unclosed declarations, NBSP separators and broken/duplicate element
attributes. An extraction error still produces `factsExported=false` at the CLI
boundary; prior valid facts are not returned as a partial result.

`xml-declaration-valid.xml` is 156 bytes, including a leading UTF-8 BOM, CRLF,
all four XML whitespace characters, mixed quote styles, mixed-case UTF-8 and
Unicode attribute names/values. SHA-256 is
`59b1714957341a5da89ffb260556b08cb758036bfe6f1247d8dd2d6a66a29144`;
its `Run` token remains at raw `[137,140)`. A read-only .NET XML reader accepted
this exact fixture. This fixture check is not a prototype execution result.

Revision 8 is **not_run** at implementation handoff. Prototype builds, golden
execution, independent negative cases and Retail verification remain the owner's
bounded follow-up. No pinned inputs, prior reports, default Source or upstream
engine sources were changed, and no profile diagnostic was suppressed.
