// Public Mailbox probe. No heap enumeration, Snapshot closure or retained locator.
// Binding is generated only after a migrated candidate has been independently verified.
export const requires = ["module", "readExact", "emit"];
export const binding = Object.freeze({
  bundleID: "retail-lua-mailbox-v1-69933",
  recipeID: "retail-lua-state-root-rip-v1",
  buildKey: "retail@12.1.0.69933",
  executableSHA256: "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd",
  rootRVA: "0x79c0c18", release: "3.1.0",
});
const MAX = (1n << 64n) - 1n;
function fail(reason) { const e = new Error(`mailbox: ${reason}`); e.code = "MAILBOX_UNAVAILABLE"; e.reason = reason; throw e; }
function need(ok, reason) { if (!ok) fail(reason); }
function span(address, length) {
  need(address > 0n && address <= MAX && Number.isSafeInteger(length) && length > 0 && address <= MAX - BigInt(length), "address");
}
export function luaHash(text) {
  const b = Buffer.from(text, "utf8"); let h = b.length >>> 0;
  const step = (b.length >> 5) + 1;
  for (let n = b.length; n >= step; n -= step) h = (h ^ (((h << 5) + (h >>> 2) + b[n - 1]) >>> 0)) >>> 0;
  return h;
}
export function adler32(b) {
  let a = 1, c = 0;
  for (const x of b) { a = (a + x) % 65521; c = (c + a) % 65521; }
  return ((c << 16) | a) >>> 0;
}
// Each invocation owns its counters and locating guards. Failed IO is charged.
export class Access {
  constructor(ctx) { this.ctx = ctx; this.calls = 0; this.bytes = 0; this.guards = []; }
  async read(address, length) {
    span(address, length);
    need(this.calls < 512 && this.bytes + length <= 1048576, "read_budget");
    this.calls++; this.bytes += length;
    let b;
    try { b = await this.ctx.readExact(address, length); }
    catch (e) { if (e?.code === "PARTIAL_READ" || e?.code === "FIXTURE_MISS") fail("short_read"); throw e; }
    need(b?.length === length, "short_read"); return Buffer.from(b);
  }
  guard(address, b) { this.guards.push({ address, b: Buffer.from(b) }); }
  async verify() {
    for (const g of this.guards) need((await this.read(g.address, g.b.length)).equals(g.b), "path_changed");
  }
  async string(address, maximum, guarded = true) {
    const h = await this.read(address, 32), n = h.readBigUInt64LE(24);
    need(h[16] === 4 && n <= BigInt(maximum) && address <= MAX - 32n - n, "string_layout");
    const b = n ? await this.read(address + 32n, Number(n)) : Buffer.alloc(0);
    if (guarded) {
      this.guard(address + 16n, h.subarray(16, 17)); this.guard(address + 20n, h.subarray(20, 32));
      if (n) this.guard(address + 32n, b);
    }
    return b;
  }
  async lookup(table, name) {
    const h = await this.read(table, 72);
    need(h[16] === 5 && h[19] <= 20, "table_layout");
    const nodes = h.readBigUInt64LE(40), count = 1n << BigInt(h[19]);
    need(nodes > 0n && nodes <= MAX - count * 56n, "node_range");
    this.guard(table + 16n, h.subarray(16, 17)); this.guard(table + 19n, h.subarray(19, 20)); this.guard(table + 40n, h.subarray(40, 48));
    let node = nodes + BigInt(luaHash(name) & Number(count - 1n)) * 56n;
    const seen = new Set();
    for (let i = 0; i < 64 && node; i++) {
      need(node >= nodes && node < nodes + count * 56n && (node - nodes) % 56n === 0n && !seen.has(node), "collision_chain"); seen.add(node);
      const b = await this.read(node, 56); this.guard(node + 24n, b.subarray(24, 34));
      if (b[32] === 4) {
        need(b[33] === 0, "restricted_key");
        const key = await this.string(b.readBigUInt64LE(24), 256);
        if (key.equals(Buffer.from(name))) {
          need(b[9] === 0, "restricted_value"); this.guard(node, b.subarray(0, 10));
          return { pointer: b.readBigUInt64LE(0), tag: b[8] };
        }
      }
      this.guard(node + 48n, b.subarray(48, 56)); node = b.readBigUInt64LE(48);
    }
    need(node === 0n, "collision_limit"); fail("publication_missing");
  }
  async text(table, name, want, maximum = 64) {
    const v = await this.lookup(table, name); need(v.tag === 4, "metadata_type");
    const b = await this.string(v.pointer, maximum); const value = b.toString("utf8");
    need(Buffer.from(value).equals(b) && (want === undefined || value === want), `metadata_${name}`); return value;
  }
  async record(table, name, runtime, kind) {
    const v = await this.lookup(table, name); need(v.tag === 4, "record_type");
    const h = await this.read(v.pointer, 32), length = h.readBigUInt64LE(24);
    need(h[16] === 4 && length >= 120n && length <= 16504n && v.pointer <= MAX - 32n - length, "record_string_length");
    this.guard(v.pointer + 16n, h.subarray(16, 17)); this.guard(v.pointer + 24n, h.subarray(24, 32));
    const at = v.pointer + 32n, header = await this.read(at, 80);
    need(header.subarray(0, 8).toString() === "LYCMEM06" && header[58] === 0 && header[59] === 0 && header.readUInt32LE(72) === 80 && header.readUInt32LE(76) === adler32(header.subarray(0, 76)), "header_invalid");
    const rt = header.subarray(24, 40).toString("hex"), sequence = header.readUInt32LE(60), size = header.readUInt32LE(64);
    const nonce = header.subarray(8, 24);
    // SlotProtocol.Describe is a descriptor, with zero nonce and an initial
    // sequence of zero. InputState publications use runtime nonce and a
    // positive, non-exhausted sequence. Both keep zero ticket and current runtime.
    const publication = kind === 1 ? nonce.equals(Buffer.alloc(16)) : kind === 5 && nonce.toString("hex") === runtime && sequence > 0 && sequence < 0xffffffff;
    need(rt === runtime && publication && header.subarray(40, 56).equals(Buffer.alloc(16)) && header[56] === kind && header[57] === 1 && size <= 16384 && BigInt(size + 120) === length, "record_identity");
    const payload = size ? await this.read(at + 80n, size) : Buffer.alloc(0), trailer = await this.read(at + 80n + BigInt(size), 40);
    need(adler32(payload) === header.readUInt32LE(68) && trailer.subarray(0, 8).toString() === "LYCEND06" && trailer.subarray(8, 24).equals(header.subarray(8, 24)) && trailer.subarray(24, 40).equals(header.subarray(24, 40)), "record_checksum");
    need((await this.read(at, 80)).equals(header), "record_changed");
    let value; try { value = JSON.parse(payload.toString("utf8")); } catch { fail("record_json"); }
    need(Buffer.from(payload.toString("utf8")).equals(payload) && value && typeof value === "object" && !Array.isArray(value), "record_json");
    return { value, sequence };
  }
}
function integer(v, minimum, maximum = Number.MAX_SAFE_INTEGER) { return Number.isSafeInteger(v) && v >= minimum && v <= maximum; }
export async function readMailbox(ctx, selected = binding) {
  // wowdump's module primitive does not expose an executable digest. The caller
  // must bind its workspace/build to selected.executableSHA256 before live use.
  need(ctx.buildKey === undefined || ctx.buildKey === selected.buildKey, "build_binding");
  const module = await ctx.module(); const base = BigInt(module.base), rva = BigInt(selected.rootRVA);
  need(module.name.toLowerCase() === "wow.exe" && rva >= 0n && rva + 8n <= BigInt(module.size), "module_binding");
  const a = new Access(ctx), root = base + rva, rb = await a.read(root, 8); a.guard(root, rb);
  const state = rb.readBigUInt64LE(0); need(state > 0n && state <= MAX - 0xa8n, "lua_state");
  const tag = await a.read(state + 16n, 1); need(tag[0] === 8, "lua_state_tag"); a.guard(state + 16n, tag);
  const globals = await a.read(state + 0x90n, 24); need(globals[8] === 5 && globals[9] === 0, "globals_layout"); a.guard(state + 0x90n, globals.subarray(0, 10));
  const ns = await a.lookup(globals.readBigUInt64LE(0), "LycheeDevInternal"); need(ns.tag === 5, "namespace_layout");
  const box = await a.lookup(ns.pointer, "Mailbox"); need(box.tag === 5, "mailbox_layout");
  await a.text(box.pointer, "schema", "lycheedev.mailbox.v1"); await a.text(box.pointer, "release", selected.release);
  const runtime = await a.text(box.pointer, "runtime", undefined, 32); need(/^[0-9a-f]{32}$/.test(runtime) && !/^0+$/.test(runtime), "runtime_token");
  const identity = await a.record(box.pointer, "identity", runtime, 1), input = await a.record(box.pointer, "input", runtime, 5);
  const i = identity.value, s = input.value;
  need(i.schema === "lycheedev.slot.identity.v2" && i.runtime === runtime && i.release === selected.release && i.slots === 200 && integer(i.nextSlot, 1, 201) && [i.guid, i.character, i.realm, i.build, i.product].every(x => typeof x === "string" && x.length > 0) && i.build === selected.buildKey.split("@")[1] && i.product === selected.buildKey.split("@")[0] && typeof i.owner === "string" && integer(i.fence, 0) && (i.inventory === undefined || integer(i.inventory, 0, 200)), "identity_schema");
  need(s.schema === "lycheedev.input.v2" && s.runtime === runtime && s.guid === i.guid && s.build === i.build && s.owner === i.owner && s.fence === i.fence && s.nextSlot === i.nextSlot && typeof s.inputBlocked === "boolean" && typeof s.reason === "string" && s.inputBlocked === (s.reason.length > 0) && integer(s.sampleMillis, 0), "input_schema");
  // No clock conversion, freshness claim or permission to send input. Path
  // rechecks bracket the observations; they cannot eliminate an ABA change.
  await a.verify();
  return { schema: "lycheedev.mailbox.probe.v1", state: "observed", binding: { ...selected, executableHashCheck: "external_workspace_required" }, identity: i, input: s, identitySequence: identity.sequence, inputSequence: input.sequence, authorization: false, reads: a.calls, bytes: a.bytes };
}
export default async function run(ctx) { const result = await readMailbox(ctx); await ctx.emit(result); }
