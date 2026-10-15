// Source/table verification only. This does not compile or execute Go or Rust.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
const goRoot = process.argv[2];
assert(goRoot, 'Pass the read-only Go 1.27.1 source root');
const hashes = {
  VERSION: '25eb74b09036e1ad894fb63b86cb0f0ee53342df8d2e2e860da4ff5079607758',
  'src/strings/strings.go': 'a81bdf73a308277c322e35d0c602e3813f57b6f769c091f8f8962c8884a45ea3',
  'src/unicode/letter.go': 'b35a6d259313cef88d07e9e4027b8bd72c60ae140418f5be2c41a2ecea6074fe',
  'src/unicode/tables.go': 'ac0e5d5d1b58ed4b02d7cb4a0085bdefbdb4f7aae6fadddbc9477f8463da8bd8',
};
const sources = {};
for (const [path, sha] of Object.entries(hashes)) {
  const raw = readFileSync(resolve(goRoot, path));
  assert.equal(createHash('sha256').update(raw).digest('hex'), sha, path);
  sources[path] = raw.toString('utf8');
}
const tables = sources['src/unicode/tables.go'];
const asciiBody = tables.match(/var asciiFold = \[MaxASCII \+ 1\]uint16\{([\s\S]*?)\n\}/)?.[1];
const orbitBody = tables.match(/var caseOrbit = \[\]foldPair\{([\s\S]*?)\n\}/)?.[1];
assert(asciiBody && orbitBody);
const ascii = [...asciiBody.matchAll(/0x([0-9A-Fa-f]+)/g)].map(m => parseInt(m[1], 16));
assert.equal(ascii.length, 128);
const orbit = new Map([...orbitBody.matchAll(/\{0x([0-9A-Fa-f]+), 0x([0-9A-Fa-f]+)\}/g)]
  .map(m => [parseInt(m[1], 16), parseInt(m[2], 16)]));
// Every class containing ASCII closes using these tables, without entering
// SimpleFold's CaseRanges fallback. This establishes the complete ASCII boundary.
const classes = ascii.map((_, start) => {
  const found = new Set();
  let at = start;
  do {
    assert(!found.has(at), 'Fold orbit did not return to its origin');
    found.add(at);
    at = at < 128 ? ascii[at] : orbit.get(at);
    assert.notEqual(at, undefined, 'Unexpected CaseRanges fallback from ASCII orbit');
  } while (at !== start);
  return found;
});
const special = [...new Set(classes.flatMap(c => [...c].filter(n => n >= 128)))].sort((a,b) => a-b);
assert.deepEqual(special, [0x17f, 0x212a]);
assert(sources['src/strings/strings.go'].includes('r := unicode.SimpleFold(sr)'));
const rust = readFileSync(new URL('../../crates/lycheedev-data/src/definitions/semantics.rs', import.meta.url), 'utf8');
const body = rust.match(/fn table_matches\([^]*?\n\}/)?.[0];
assert(body);
const arms = new Map([...body.matchAll(/Some\('\\u\{([0-9a-f]+)\}'\) => b'([a-z])'/g)]
  .map(m => [parseInt(m[1], 16), m[2].charCodeAt(0)]));
assert.deepEqual([...arms.keys()].sort((a,b) => a-b), special);
for (const required of ['if !canonical.is_ascii()', 'selected.chars()', 'canonical.bytes()',
  'Some(actual) if actual.is_ascii() => actual as u8', '_ => return false',
  'actual.eq_ignore_ascii_case(&expected)', 'selected.next().is_none()']) assert(body.includes(required), required);
assert(rust.includes('if !table_matches(selected, table) || parts[0] == "COLOR"'));
const lower = n => n >= 65 && n <= 90 ? n+32 : n;
const admits = (n, expected, map = arms) => {
  const actual = n < 128 ? n : map.get(n);
  return actual !== undefined && lower(actual) === lower(expected);
};
let scalarComparisons = 0;
const candidates = new Set([...ascii.keys(), ...orbit.keys(), 0x130, 0x131, 0xdf, 0xfb00, 0xff33]);
for (let expected = 0; expected < 128; expected++) {
  for (const actual of candidates) {
    assert.equal(admits(actual, expected), classes[expected].has(actual), `${actual}/${expected}`);
    scalarComparisons++;
  }
}
for (const special of arms.keys()) {
  const mutated = new Map(arms);
  mutated.delete(special);
  assert([...classes.keys()].some(expected => admits(special,expected,mutated) !== classes[expected].has(special)), 'Removed production mapping must be detected');
}
console.log(JSON.stringify({status:'PASS', kind:'source-derived table/branch check only', goExecution:'NOT_RUN', rustExecution:'NOT_RUN', asciiClasses:128, nonAsciiMembers:special.map(n=>`U+${n.toString(16).toUpperCase().padStart(4,'0')}`), scalarComparisons, removedArmMutationsDetected:2}));
