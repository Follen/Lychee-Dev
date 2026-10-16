// Reproduce Go Unicode17 L/Nd identifier admission from the existing fixed data.
// Usage: node ... --check <Rust output> [UnicodeData.txt]
//        node ... --output <Rust output> [UnicodeData.txt]
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const [mode, output, input = 'release/data/unicode-17.0.0/UnicodeData.txt'] = process.argv.slice(2);
if (!['--check', '--output'].includes(mode) || !output) throw Error('Expected --check or --output and a Rust path');
const expected = '2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c';
const raw = readFileSync(input);
if (createHash('sha256').update(raw).digest('hex') !== expected) throw Error('Fixed Unicode17 input differs');
const groups = {LETTERS: [], DIGITS: []};
const categories = new Uint8Array(0x110000);
let first;
for (const line of raw.toString('utf8').trim().split('\n')) {
  const fields = line.trimEnd().split(';');
  const high = parseInt(fields[0], 16), category = fields[2];
  if (fields[1].endsWith(', First>')) { if (first) throw Error('Nested range'); first = [high, category]; continue; }
  let low = high;
  if (fields[1].endsWith(', Last>')) {
    if (!first || first[1] !== category) throw Error('Mismatched range');
    low = first[0]; first = undefined;
  }
  const name = category.startsWith('L') ? 'LETTERS' : category === 'Nd' ? 'DIGITS' : undefined;
  if (!name) continue;
  categories.fill(name === 'LETTERS' ? 1 : 2, low, high + 1);
  const rows = groups[name], prior = rows.at(-1);
  if (prior && prior[1] + 1 === low) prior[1] = high;
  else rows.push([low, high]);
}
if (first) throw Error('Unclosed range');
let out = `//! Unicode17 identifier categories, from the already fixed official UnicodeData.txt.
//! SHA256 ${expected}.
//! Unicode License V3: ../../UNICODE-LICENSE. No locale or full-case folding.
fn contains(ranges: &[(u32, u32)], value: char) -> bool {
    let value = value as u32;
    let at = ranges.partition_point(|(_, end)| *end < value);
    ranges.get(at).is_some_and(|(start, _)| *start <= value)
}
pub(super) fn letter(value: char) -> bool {
    contains(LETTERS, value)
}
pub(super) fn digit(value: char) -> bool {
    contains(DIGITS, value)
}
`;
for (const [name, rows] of Object.entries(groups)) out += `const ${name}: &[(u32, u32)] = &[\n${rows.map(([a,b]) => `    (0x${a.toString(16)}, 0x${b.toString(16)}),`).join('\n')}\n];\n`;
if (mode === '--output') writeFileSync(output, out);
else if (readFileSync(output, 'utf8') !== out) throw Error('Generated Rust bytes differ');
// Independent full-domain interval membership check against Unicode records.
for (const [name, rows] of Object.entries(groups)) {
  let index = 0;
  for (let cp = 0; cp < categories.length; cp++) {
    while (index < rows.length && rows[index][1] < cp) index++;
    const actual = index < rows.length && rows[index][0] <= cp;
    if (actual !== (categories[cp] === (name === 'LETTERS' ? 1 : 2))) throw Error(`Membership differs at ${cp}`);
  }
}
console.log(JSON.stringify({status: 'PASS', mode, input_sha256: expected,
  ranges: Object.fromEntries(Object.entries(groups).map(([name, rows]) => [name, rows.length])),
  members: Object.fromEntries(Object.entries(groups).map(([name, rows]) => [name, rows.reduce((n,[a,b]) => n+b-a+1,0)])),
  checked_code_points: categories.length, rust_compile: 'NOT_RUN', rust_tests: 'NOT_RUN'}, null, 2));
