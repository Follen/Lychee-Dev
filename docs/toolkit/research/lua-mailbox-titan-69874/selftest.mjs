import assert from "node:assert/strict";
import { writeFile, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readMailbox, luaHash, adler32, Access, binding } from "./reader.mjs";
import { Fixture, hash, encodeRecord } from "./fixture-helper.mjs";
const results = [];
// Generate the truth record with the real Lua producer, not this JS encoder.
const addon = fileURLToPath(new URL("../../../../addon/", import.meta.url)).replaceAll("\\", "/");
const lua = `local ns={}\nlocal root=${JSON.stringify(addon)}\nfor _,name in ipairs({"Bridge/CaptureWriter.lua","Bridge/MemoryProtocol.lua","Bridge/SlotProtocol.lua"}) do assert(loadfile(root..name))("Lychee Dev",ns) end\nlocal engine=ns.SlotProtocol.Create({runtime=string.rep("1",32),build="3.80.2.69874",product="titan",release="3.1.0",inventory=0,inputState="lycheedev.input-signal.v1",actor=function() return {character="Fixture",realm="Synthetic",guid="synthetic-guid"} end,encode=ns.CaptureWriter.Encode})\nlocal record=assert(engine.Describe())\nfor i=1,#record do io.write(string.format("%02x",string.byte(record,i))) end\n`;
const generated = spawnSync(process.env.LYCHEEDEV_LUA51 || "lua", ["-"], { input: lua, encoding: "utf8", windowsHide: true, timeout: 10000, maxBuffer: 65536 });
assert.equal(generated.status, 0, generated.error?.message || generated.stderr);
assert.match(generated.stdout, /^[0-9a-f]+$/);
const productionIdentity = Buffer.from(generated.stdout, "hex");
const productionValue = JSON.parse(productionIdentity.subarray(80, productionIdentity.length - 40).toString());
async function check(name, fn) { const start = performance.now(); await fn(); results.push({ name, state: "passed", elapsedMillis: Math.round(performance.now() - start) }); }
async function rejected(f, reason) { await assert.rejects(() => readMailbox(f.context()), e => e.code === "MAILBOX_UNAVAILABLE" && (!reason || e.reason === reason)); assert.equal(f.emitted, undefined); }
await check("current named public publications; no scan primitives", async () => {
  const f = new Fixture(); f.identity = productionValue; f.input.owner = productionValue.owner; f.input.fence = productionValue.fence;
  f.publish("identity", productionIdentity); f.replace("input", f.input, 5, 1);
  const r = await readMailbox(f.context()); assert.deepEqual(r.identity, f.identity); assert.deepEqual(r.input, f.input); assert.equal(r.identitySequence, 0); assert.equal(r.authorization, false); assert.ok(r.reads <= 512 && r.bytes <= 1048576);
  await writeFile(new URL("fixture.json", import.meta.url), JSON.stringify(f.json(r), null, 2) + "\n");
  const profileFixture = { schema: "wowdump.profile-fixture.v1", identity: { buildKey: binding.buildKey, executableSha256: binding.executableSHA256 }, module: f.module, memory: [...f.memory].map(([at, b]) => ({ address: `0x${at.toString(16)}`, dataHex: b.toString("hex") })), expected: { fields: { luaState: `0x${f.state.toString(16)}`, threadTag: 8, globalsValue: { pointer: `0x${f.globals.pointer.toString(16)}`, tag: 5, secret: 0 } } } };
  await writeFile(new URL("root.fixture.json", import.meta.url), JSON.stringify(profileFixture, null, 2) + "\n");
  profileFixture.identity.executableSha256 = "0".repeat(64); profileFixture.expected = { error: { code: "EXECUTABLE_MISMATCH" } };
  await writeFile(new URL("root-wrong-hash.fixture.json", import.meta.url), JSON.stringify(profileFixture, null, 2) + "\n");
});
await check("ASLR and root/heap relocation; old bytes retained", async () => {
  const f = new Fixture(0x180000000n, 0x210000000n), ctx = f.context(); const first = await readMailbox(ctx); const old = f.box.addresses.input.pointer;
  f.install(0x320000000n, 9); const next = await readMailbox(ctx); assert.equal(next.inputSequence, 9); assert.notEqual(first.input.sampleMillis, next.input.sampleMillis); assert.ok(f.memory.has(old)); assert.ok(f.reads.filter(r => r.address === f.root).length >= 4);
});
await check("same table new publication; valid old bytes cannot win", async () => {
  const f = new Fixture(), old = f.box.addresses.input.pointer; await readMailbox(f.context()); f.input.sampleMillis = 2025; f.replace("input", f.input, 5, 7);
  const r = await readMailbox(f.context()); assert.equal(r.inputSequence, 7); assert.equal(r.input.sampleMillis, 2025); assert.ok(f.memory.has(old));
});
await check("real hash collision follows chain and key bytes", async () => {
  assert.equal(luaHash("Mailbox"), 0x20d3e4bf); // pinned Lua 5.1 hash vector
  const f = new Fixture(); f.install(0x310000000n, 3, true); assert.notEqual(f.collisionKey, "identity"); assert.equal(hash(f.collisionKey) & 255, hash("identity") & 255); assert.equal((await readMailbox(f.context())).identitySequence, 0);
});
const mutations = [
  ["identity wrong nonce with valid checksums", "record_identity", f => { const wire = Buffer.from(productionIdentity); Buffer.from("11".repeat(16), "hex").copy(wire, 8); wire.writeUInt32LE(adler32(wire.subarray(0, 76)), 76); wire.subarray(8, 24).copy(wire, wire.length - 32); f.publish("identity", wire); }],
  ["input sequence zero with valid checksums", "record_identity", f => f.replace("input", f.input, 5, 0)],
  ["input exhausted sequence with valid checksums", "record_identity", f => f.replace("input", f.input, 5, 0xffffffff)],
  ["secret value", "restricted_value", f => f.mutate(f.box.addresses.input.node + 9n, Buffer.from([1]))],
  ["secret key", "restricted_key", f => f.mutate(f.box.addresses.input.node + 33n, Buffer.from([1]))],
  ["globals secret", "globals_layout", f => f.mutate(f.state + 0x99n, Buffer.from([1]))],
  ["wrong state tag", "lua_state_tag", f => f.mutate(f.state + 16n, Buffer.from([5]))],
  ["wrong table tag", "table_layout", f => f.mutate(f.box.pointer + 16n, Buffer.from([4]))],
  ["table count bound", "table_layout", f => f.mutate(f.box.pointer + 19n, Buffer.from([21]))],
  ["TString length bound", "record_string_length", f => { const b = Buffer.alloc(8); b.writeBigUInt64LE(16505n); f.mutate(f.box.addresses.input.pointer + 24n, b); }],
  ["old wire magic", "header_invalid", f => f.mutate(f.box.addresses.input.pointer + 32n, Buffer.from("LYCMEM05"))],
  ["CRC payload mismatch", "record_checksum", f => f.mutate(f.box.addresses.input.pointer + 112n, Buffer.from([0]))],
  ["wrong input version", "input_schema", f => f.replace("input", { ...f.input, schema: "lycheedev.input.v1" }, 5)],
  ["wrong identity version", "identity_schema", f => f.replace("identity", { ...f.identity, schema: "lycheedev.slot.identity.v1" }, 1)],
  ["wrong identity build", "identity_schema", f => f.replace("identity", { ...f.identity, build: "12.1.0.70000" }, 1)],
  ["wrong runtime publication", "record_identity", f => { const b = encodeRecord({ ...f.input, runtime: "22".repeat(16) }, 5, 2, "22".repeat(16)); const p = f.string(b), v = Buffer.alloc(8); v.writeBigUInt64LE(p); f.mutate(f.box.addresses.input.node, v); }],
  ["wrong release", "metadata_release", f => { const p = f.string("3.0.2"), b = Buffer.alloc(8); b.writeBigUInt64LE(p); f.mutate(f.box.addresses.release.node, b); }],
];
for (const [name, reason, mutate] of mutations) await check(name, async () => { const f = new Fixture(); mutate(f); await rejected(f, reason); });
await check("short IO fails; no partial record", async () => { const f = new Fixture(); const ctx = f.context(), read = ctx.readExact; ctx.readExact = async (a, n) => (await read(a, n)).subarray(0, n - 1); await assert.rejects(() => readMailbox(ctx), e => e.reason === "short_read"); });
await check("cancelled IO and driver budget errors propagate without output", async () => {
  for (const code of ["CANCELLED", "READ_BUDGET"]) { const f = new Fixture(), ctx = f.context(); const e = new Error(code); e.code = code; ctx.readExact = async () => { throw e; }; await assert.rejects(() => readMailbox(ctx), error => error === e); assert.equal(f.emitted, undefined); }
});
await check("root changed during path guard rejects", async () => { const f = new Fixture(); let n = 0; f.beforeRead = a => { if (a === f.root && ++n === 2) f.memory.get(f.root).writeBigUInt64LE(f.state + 16n); }; await rejected(f, "path_changed"); });
await check("current value pointer changed during path guard rejects", async () => { const f = new Fixture(); f.beforeRead = (a, n) => { if (a === f.box.addresses.input.node && n === 10) { const b = Buffer.alloc(8); b.writeBigUInt64LE(f.box.addresses.identity.pointer); f.mutate(a, b); } }; await rejected(f, "path_changed"); });
await check("key bytes and structural guards changed reject", async () => { for (const target of ["key", "struct"]) { const f = new Fixture(); let fired = false; f.beforeRead = (a, n) => { if (!fired && a === f.root && n === 8 && f.reads.length > 1) { fired = true; f.mutate(target === "key" ? f.box.addresses.input.key + 32n : f.box.pointer + 19n, Buffer.from([target === "key" ? 120 : 7])); } }; await rejected(f, "path_changed"); } });
await check("collision cycle rejects without full scan", async () => { const f = new Fixture(); const a = f.box.addresses.input; f.mutate(a.key + 32n, Buffer.from("xxxxx")); const b = Buffer.alloc(8); b.writeBigUInt64LE(a.node); f.mutate(a.node + 48n, b); await rejected(f, "collision_chain"); });
await check("512 calls and 1MiB budget checked before IO", async () => { for (const limit of ["calls", "bytes"]) { let calls = 0; const a = new Access({ async readExact(_p, n) { calls++; return Buffer.alloc(n); } }); a[limit] = limit === "calls" ? 512 : 1048576; await assert.rejects(() => a.read(1n, 1), e => e.reason === "read_budget"); assert.equal(calls, 0); } });
await check("valid 61-node collision path exhausts guard budget at physical call 512", async () => {
  const f = new Fixture(), values = {};
  for (let n = 0; Object.keys(values).length < 60; n++) { const name = `bounded-decoy-${n}`; if ((hash(name) & 255) === (hash("identity") & 255)) values[name] = { tag: 4, pointer: f.string("decoy") }; }
  for (const [name, value] of Object.entries(f.box.addresses)) values[name] = { pointer: value.pointer, tag: value.tag };
  const box = f.table(values), b = Buffer.alloc(8); b.writeBigUInt64LE(box.pointer); f.mutate(f.namespace.addresses.Mailbox.node, b);
  await rejected(f, "read_budget"); assert.equal(f.reads.length, 512);
});
await check("mismatched build binding fails before reads", async () => { const f = new Fixture(), ctx = f.context(); ctx.buildKey = "titan@next"; await assert.rejects(() => readMailbox(ctx), e => e.reason === "build_binding"); assert.equal(f.reads.length, 0); });
await check("profile every layout numeric value explicitly covered", async () => {
  const profile = JSON.parse(await readFile(new URL("root.profile.json", import.meta.url), "utf8")); const covered = new Set([...profile.bindings, ...profile.invariants].map(x => x.path));
  function walk(o, p) { for (const [k, v] of Object.entries(o ?? {})) { const path = `${p}/${k}`; if (["offset", "nextOffset", "keyOffset", "entryKeyOffset", "valueOffset", "objectKeyOffset", "size", "stride", "keySize", "count", "rva", "address"].includes(k) && ["string", "number"].includes(typeof v)) assert.ok(covered.has(path), path); else if (v && typeof v === "object") walk(v, path); } }
  walk(profile.fields, "/fields"); walk(profile.types, "/types"); assert.equal(profile.bindings.length, 3); assert.ok(profile.invariants.every(x => /revalidate/i.test(x.reason)));
});
await check("synthetic complete module manifest for actual RIP migration", async () => {
  const anchor = JSON.parse(await readFile(new URL("root.anchor.json", import.meta.url), "utf8"));
  const pattern = Buffer.from(anchor.pattern.split(" ").map(x => x === "??" ? 0 : Number.parseInt(x, 16)));
  pattern.writeInt32LE(0x4000 - (0x1100 + 7), 3);
  const bytes = Buffer.alloc(4096, 0xcc); pattern.copy(bytes, 0x100);
  const file = "migration-text.fixture.bin"; await writeFile(new URL(file, import.meta.url), bytes);
  const manifest = { schema: "wowdump.runtime-dump.v4", complete: true, buildKey: "fixture@lua-mailbox-migration", executableSha256: "3".repeat(64), synthetic: true, sections: [{ name: ".text", rva: "0x1000", bytes: bytes.length, readSize: bytes.length, file, sha256: createHash("sha256").update(bytes).digest("hex"), protection: "rx", gaps: [] }] };
  await writeFile(new URL("migration.fixture.manifest.json", import.meta.url), JSON.stringify(manifest, null, 2) + "\n");
});
const producerSources = [];
for (const path of ["Bridge/CaptureWriter.lua", "Bridge/MemoryProtocol.lua", "Bridge/SlotProtocol.lua"]) producerSources.push({ path: `addon/${path}`, sha256: createHash("sha256").update(await readFile(new URL(`../../../../addon/${path}`, import.meta.url))).digest("hex") });
const report = { schema: "lycheedev.mailbox.offline-validation.v1", binding, mode: "synthetic", liveValidation: "not_checked", state: "passed", regression: { before: "Actual Lua SlotProtocol.Describe fresh identity reproduced MAILBOX_UNAVAILABLE: record_identity", after: "Production Lua identity accepted with zero nonce/ticket and sequence zero; invalid identity nonce and zero/exhausted input sequence rejected", producerSources, identityWireSHA256: createHash("sha256").update(productionIdentity).digest("hex"), identityHeaderHex: productionIdentity.subarray(0, 80).toString("hex") }, tests: results };
await writeFile(new URL("selftest-report.json", import.meta.url), JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify({ state: "passed", tests: results.length, report: fileURLToPath(new URL("selftest-report.json", import.meta.url)) }));
