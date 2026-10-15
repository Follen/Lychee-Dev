import test from 'node:test';
import { createHash } from 'node:crypto';
import { authenticate, ORACLE } from '../tests/rust-parity/formal-lua-3.2.2/assertions.mjs';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeZip } from './zip.mjs';
import { buildAddonZip, collectAddonZipEntries, parseToc, verifyAddonZip } from './addon-package.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MANIFEST = 'Lychee Dev/Lychee Dev.toc';
const SUPPORTED_INTERFACES = ['120105', '120100', '50504', '38002', '16001', '11509', '20506'];

// Fixture ZIPs are written with writeZip into temp dirs; the repository is
// never modified. `mutate` edits the collected entry map in memory.
async function fixtureZip(t, mutate = () => {}) {
  const dir = mkdtempSync(join(tmpdir(), 'lycheedev-package-'));
  t.after(() => rmSync(dir, { recursive: true }));
  const { entries } = collectAddonZipEntries(repo);
  mutate(entries);
  const outPath = join(dir, 'fixture.zip');
  await writeZip(entries, outPath);
  return { path: outPath, buffer: readFileSync(outPath) };
}

function assertViolation(report, pattern) {
  assert.equal(report.ok, false);
  assert.ok(report.violations.length > 0);
  assert.ok(report.violations.some(violation => pattern.test(violation)),
    `expected a violation matching ${pattern}, got ${JSON.stringify(report.violations)}`);
}

test('parseToc reads headers and skips comments and blank lines', () => {
  const toc = parseToc([
    '## Interface: 120100',
    '## Title: Lychee Dev',
    '## Version: 2.0.0-dev',
    '## SavedVariables: LycheeToolkitDB',
    '',
    '# --- section comment ---',
    'Core\\ClientGate.lua',
    '   ',
    'Core\\Locale.lua',
    '# trailing comment',
  ].join('\n'));
  assert.deepEqual(toc, {
    interface: '120100',
    title: 'Lychee Dev',
    version: '2.0.0-dev',
    savedVariables: 'LycheeToolkitDB',
    loads: ['Core\\ClientGate.lua', 'Core\\Locale.lua'],
  });
});

test('parseToc leaves absent headers null and tolerates CRLF', () => {
  const toc = parseToc('Core\\ClientGate.lua\r\n\r\nCore\\Runtime.lua');
  assert.deepEqual(toc, {
    interface: null, title: null, version: null, savedVariables: null,
    loads: ['Core\\ClientGate.lua', 'Core\\Runtime.lua'],
  });
});

test('collectAddonZipEntries ships one flat manifest, its loads and Media', () => {
  const { entries, tocs, report } = collectAddonZipEntries(repo);
  assert.deepEqual(tocs.map(toc => toc.name), ['Lychee Dev.toc']);
  assert.ok(entries.has(MANIFEST));
  // A per-client TOC variant is the engine selection rule the single-manifest
  // architecture removed; the archive must not carry one.
  for (const client of ['Mainline', 'Mists', 'Wrath', 'Forever']) {
    assert.equal(entries.has(`Lychee Dev/Lychee Dev_${client}.toc`), false);
  }
  assert.ok(entries.has('Lychee Dev/Media/Logo.png'));
  assert.equal(entries.size, report.length);
  const toc = tocs[0].parse;
  for (const line of toc.loads) assert.ok(entries.has(`Lychee Dev/${line.replaceAll('\\', '/')}`), line);
  const declared = toc.interface.split(',').map(part => part.trim());
  assert.deepEqual(declared, SUPPORTED_INTERFACES);
  assert.ok(toc.loads.some(line => /^Core\\ClientGate\.lua$/i.test(line)));
  assert.equal(toc.loads.at(-1), 'Core\\Startup.lua');
});

test('built addon zip verifies clean', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'lycheedev-package-'));
  t.after(() => rmSync(dir, { recursive: true }));
  const result = await buildAddonZip({ repoRoot: repo, outPath: join(dir, 'lycheedev-test-addon.zip') });
  assert.equal(result.bytes, readFileSync(result.path).length);
  assert.deepEqual(result.files, [...result.files].sort());
  assert.equal(result.files.length, result.report.length);
  const clean = verifyAddonZip(readFileSync(result.path));
  assert.deepEqual(clean.violations, []);
  assert.equal(clean.ok, true);
  assert.equal(verifyAddonZip(result.path, { repoRoot: repo }).ok, true);
});

test('a deleted load file entry fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => entries.delete('Lychee Dev/Core/Locale.lua'));
  assertViolation(verifyAddonZip(buffer), /Lychee Dev\/Core\/Locale\.lua/);
});

test('an extra tests entry fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => entries.set('Lychee Dev/tests/x.lua', Buffer.from('return\n')));
  assertViolation(verifyAddonZip(buffer), /Lychee Dev\/tests\/x\.lua/);
});

test('a Python file fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => entries.set('Lychee Dev/automation.py', Buffer.from('print(1)\n')));
  assertViolation(verifyAddonZip(buffer), /Lychee Dev\/automation\.py/);
});

test('a task block in Definitions.lua fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => {
    entries.set('Lychee Dev/Bridge/Definitions.lua',
      Buffer.from('local _, ns = ...\nns.ProbeDefinitions = {schema="lycheedev.queue.v1",entries={\n  {id="local"},\n}}\n'));
  });
  assertViolation(verifyAddonZip(buffer), /Lychee Dev\/Bridge\/Definitions\.lua/);
});

test('an incomplete Interface declaration fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => {
    const text = entries.get(MANIFEST).toString('utf8');
    const trimmed = text.replace('## Interface: 120105, 120100, 50504, 38002, 16001', '## Interface: 120105, 120100, 50504, 38002');
    assert.notEqual(trimmed, text);
    entries.set(MANIFEST, Buffer.from(trimmed));
  });
  assertViolation(verifyAddonZip(buffer), /Interface: missing 16001/);
});

test('an unsupported Interface value fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => {
    const text = entries.get(MANIFEST).toString('utf8');
    const doctored = text.replace('## Interface: 120105', '## Interface: 99999');
    assert.notEqual(doctored, text);
    entries.set(MANIFEST, Buffer.from(doctored));
  });
  assertViolation(verifyAddonZip(buffer), /declares unsupported value 99999/);
});

test('a per-client TOC variant fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => {
    entries.set('Lychee Dev/Lychee Dev_Mists.toc', entries.get(MANIFEST));
  });
  assertViolation(verifyAddonZip(buffer), /Lychee Dev\/Lychee Dev_Mists\.toc/);
});

test('a wrong top-level directory fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => {
    const moved = new Map([...entries]
      .map(([name, bytes]) => [`add-on/${name.slice('Lychee Dev/'.length)}`, bytes]));
    entries.clear();
    for (const [name, bytes] of moved) entries.set(name, bytes);
  });
  assertViolation(verifyAddonZip(buffer), /add-on\//);
});

test('missing Media fails verification', async t => {
  const { buffer } = await fixtureZip(t, entries => {
    for (const name of [...entries.keys()]) if (name.startsWith('Lychee Dev/Media/')) entries.delete(name);
  });
  assertViolation(verifyAddonZip(buffer), /Lychee Dev\/Media\//);
});


test('Go 3.2.2 ZIP closure keeps all 55 authenticated addon resources and two npm licenses', () => {
  assert.equal(ORACLE, '0bec60e4406f5d5dc155ee25cef44f9a710e37e8');
  const identity = authenticate(repo);
  const { entries, report, tocs } = collectAddonZipEntries(repo);
  const expected = identity.addon.map(row => row.path.replace(/^addon\//, 'Lychee Dev/'));
  assert.equal(expected.length, 55);
  expected.push('Lychee Dev/LICENSE', 'Lychee Dev/THIRD_PARTY_NOTICES');
  assert.deepEqual([...entries.keys()].sort(), expected.sort());
  assert.equal(entries.size, 57);
  assert.equal(report.length, 57);
  for (const row of identity.addon) {
    const bytes = entries.get(row.path.replace(/^addon\//, 'Lychee Dev/'));
    assert.equal(bytes.length, row.bytes, row.path);
    assert.equal(createHash('sha256').update(bytes).digest('hex'), row.sha256, row.path);
  }
  assert(!tocs[0].parse.loads.includes('Bindings.xml'), 'fixture must exercise implicit loading');
  assert.deepEqual(entries.get('Lychee Dev/Bindings.xml'), readFileSync(join(repo, 'addon/Bindings.xml')));
  assert.deepEqual(report.find(row => row.path === 'Lychee Dev/Bindings.xml'), {path: 'Lychee Dev/Bindings.xml', selected: 'load'});
  for (const name of ['LICENSE', 'THIRD_PARTY_NOTICES']) {
    assert.deepEqual(entries.get('Lychee Dev/' + name), readFileSync(join(repo, 'packages/npm/lycheedev', name)));
    assert.equal(report.find(row => row.path === 'Lychee Dev/' + name).selected, 'license');
  }
});

test('Go 3.2.2 implicit binding declarations are mandatory without adding a TOC load', async t => {
  const { buffer } = await fixtureZip(t, entries => entries.delete('Lychee Dev/Bindings.xml'));
  assertViolation(verifyAddonZip(buffer), /missing bindings: Lychee Dev\/Bindings\.xml/);
});

test('Go 3.2.2 project license and notices must both exist and be nonempty', async t => {
  for (const name of ['LICENSE', 'THIRD_PARTY_NOTICES']) for (const empty of [false, true]) {
    await t.test(name + (empty ? ' empty' : ' absent'), async t => {
      const { buffer } = await fixtureZip(t, entries => {
        if (empty) entries.set('Lychee Dev/' + name, Buffer.alloc(0));
        else entries.delete('Lychee Dev/' + name);
      });
      assertViolation(verifyAddonZip(buffer), new RegExp('missing license: Lychee Dev/' + name));
    });
  }
});

test('Go 3.2.2 ZIP excludes and rejects every unselected legacy addon resource', async t => {
  // Negative fixtures remain meaningful after these production files retire.
  const legacy = ['Bridge/Definitions.lua', 'Bridge/FaultRunner.lua', 'Bridge/Identity.lua',
    'Bridge/InputProtocol.lua', 'Bridge/InputSignal.lua', 'Bridge/Investigation.lua',
    'Bridge/MatrixSymbol.lua', 'Bridge/NativeAdapter.lua', 'Bridge/NativeInputState.lua',
    'Bridge/NativeProtocol.lua', 'Bridge/NativeRuntime.lua', 'Bridge/NativeWire.lua',
    'Bridge/ProbeQueue.lua', 'Bridge/ProbeRunner.lua', 'Bridge/ReceiptView.lua',
    'Bridge/Receiver.lua', 'Bridge/Reentry.lua', 'Bridge/ReportStore.lua',
    'Bridge/Session.lua', 'Bridge/StartupBeacon.lua', 'Lychee Dev Rust.toc'];
  const { entries } = collectAddonZipEntries(repo);
  for (const path of legacy) {
    assert(!entries.has('Lychee Dev/' + path), path);
    await t.test(path, async t => {
      const { buffer } = await fixtureZip(t, files => files.set('Lychee Dev/' + path, Buffer.from('return\n')));
      const result = verifyAddonZip(buffer);
      assert.equal(result.ok, false);
      assert(result.violations.includes('entry not selected as toc/load/media: Lychee Dev/' + path));
    });
  }
});
