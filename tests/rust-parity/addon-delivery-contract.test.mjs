// Static contract checks, not a claim of Rust compilation or game behavior.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { GO_COMMIT, addonInventory } from './oracle.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const oracle = JSON.parse(readFileSync(join(root, 'tests/rust-parity/go-3.2.0.manifest.json'), 'utf8'));
const read = path => readFileSync(join(root, path), 'utf8');
const digest = bytes => createHash('sha256').update(bytes).digest('hex');

test('all 55 formal addon resources match fixed Go 3.2.0 bytes', () => {
  assert.equal(oracle.oracle.commit, GO_COMMIT);
  assert.equal(oracle.addon.length, 55);
  const current = new Map(addonInventory(root).map(row => [row.path, row]));
  for (const expected of oracle.addon) assert.deepEqual(current.get(expected.path), expected, expected.path);
  // Extra historical files remain a separate failing gate in run.mjs until
  // their consumers have been migrated. Do not silently whitelist them there.
});

test('formal TOC and automatic Bindings.xml close over the pinned resource set', () => {
  const paths = new Set(oracle.addon.map(row => row.path));
  const toc = read('addon/Lychee Dev.toc');
  const loaded = toc.split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith('#')).map(line => `addon/${line.replaceAll('\\', '/')}`);
  assert.equal(new Set(loaded).size, loaded.length);
  for (const path of loaded) assert.ok(paths.has(path), `unowned load path: ${path}`);
  assert.ok(loaded.includes('addon/Bridge/SlotRuntime.lua'));
  assert.ok(!loaded.some(path => /Native(?:Runtime|Wire|Protocol|Adapter|InputState)\.lua$/.test(path)));
  assert.ok(loaded.indexOf('addon/Core/Runtime.lua') < loaded.indexOf('addon/Bridge/SlotRuntime.lua'), 'bootstrap must precede full runtime');
  assert.equal(loaded.at(-1), 'addon/Core/Startup.lua');
  const bindings = read('addon/Bindings.xml');
  assert.ok(paths.has('addon/Bindings.xml'));
  assert.match(bindings, /name="LYCHEEDEV_WAKE"/);
  assert.match(bindings, /name="LYCHEEDEV_RELOAD"/);
});

function rustLiteral(source, pattern) {
  const match = source.match(pattern);
  assert.ok(match, 'reviewed literal-only generator shape changed; review this adapter');
  return JSON.parse(`"${match[1]}"`);
}

test('reviewed Rust slot generator literals equal Go bytes at slot boundaries', () => {
  const source = read('crates/lycheedev-delivery/src/slots.rs');
  const interfaces = rustLiteral(source, /pub const INTERFACES: &str = "([^"\n]*)";/);
  const transport = rustLiteral(source, /pub const TRANSPORT: &str = "([^"\n]*)";/);
  const body = source.slice(source.indexOf('pub fn files('), source.indexOf('\npub fn install('));
  const toc = rustLiteral(body, /format!\("(## Interface: (?:[^"\\]|\\.)*)"\)/);
  const loader = rustLiteral(body, /format!\("(local envelope = (?:[^"\\]|\\.)*)"\)/);
  assert.equal(interfaces, '120100, 50504, 38002, 16001, 11509, 20506');
  assert.equal(transport, 'Lycheedev mailbox protocol v1');
  assert.match(body, /\("Payload\.lua"\.into\(\), b"LycheeDevSlotEnvelope = nil\\n"\.to_vec\(\)\)/);
  for (const index of [1, 9, 10, 64, 200, 201, 499, 500]) {
    const version = '3.2.0';
    const format = input => input.replaceAll('{INTERFACES}', interfaces).replaceAll('{TRANSPORT}', transport).replaceAll('{version}', version).replaceAll('{index:02}', String(index).padStart(2, '0')).replaceAll('{index}', String(index));
    const expectedToc = `## Interface: ${interfaces}\n## Title: Lychee Dev input slot ${String(index).padStart(2, '0')}\n## Version: ${version}\n## LoadOnDemand: 1\n## X-Lychee-Slot: ${index}\n## X-Lychee-Transport: ${transport}\nPayload.lua\nLoader.lua\n`;
    const expectedLoader = `local envelope = LycheeDevSlotEnvelope\nLycheeDevSlotEnvelope = nil\nlocal ns = LycheeDevInternal\nif ns and ns.SlotRuntime then ns.SlotRuntime.Receive(${index}, envelope) end\n`;
    assert.equal(digest(format(toc)), digest(expectedToc));
    assert.equal(digest(format(loader)), digest(expectedLoader));
  }
});

test('installation and update gate the formal SlotRuntime closure', () => {
  for (const path of ['crates/lycheedev-delivery/src/addon_transaction.rs', 'crates/lycheedev-delivery/src/update.rs']) {
    const source = read(path);
    assert.match(source, /line\.trim\(\)\.replace\('\\\\', "\/"\) == "Bridge\/SlotRuntime\.lua"/);
    assert.ok(!source.includes('Bridge/NativeRuntime.lua'), path);
  }
  assert.match(read('crates/lycheedev-delivery/src/installation.rs'), /protocol != crate::slots::TRANSPORT/);
});
