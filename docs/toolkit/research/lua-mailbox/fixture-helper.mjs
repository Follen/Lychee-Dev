// Offline synthetic memory only; no process driver or production dependencies.
import { binding } from "./reader.mjs";
const hex = a => `0x${a.toString(16)}`;
export function hash(text) {
  const b = Buffer.from(text); let h = b.length;
  for (let n = b.length, step = Math.floor(b.length / 32) + 1; n >= step; n -= step) h = (h ^ (Math.imul(h, 32) + (h >>> 2) + b[n - 1])) >>> 0;
  return h;
}
function checksum(b) { let lo = 1, hi = 0; for (const x of b) { lo = (lo + x) % 65521; hi = (hi + lo) % 65521; } return (hi * 65536 + lo) >>> 0; }
export function encodeRecord(value, kind, sequence = kind === 1 ? 0 : 1, runtime = "11".repeat(16)) {
  const p = Buffer.from(JSON.stringify(value)), h = Buffer.alloc(80), t = Buffer.alloc(40);
  h.write("LYCMEM06"); if (kind !== 1) Buffer.from(runtime, "hex").copy(h, 8); Buffer.from(runtime, "hex").copy(h, 24);
  h[56] = kind; h[57] = 1; h.writeUInt32LE(sequence, 60); h.writeUInt32LE(p.length, 64); h.writeUInt32LE(checksum(p), 68); h.writeUInt32LE(80, 72); h.writeUInt32LE(checksum(h.subarray(0, 76)), 76);
  t.write("LYCEND06"); h.subarray(8, 40).copy(t, 8); return Buffer.concat([h, p, t]);
}
export class Fixture {
  constructor(base = 0x140000000n, heap = 0x200000000n) {
    this.module = { name: "Wow.exe", base: hex(base), size: 0x9000000 };
    this.base = base; this.memory = new Map(); this.reads = []; this.root = base + BigInt(binding.rootRVA);
    this.memory.set(this.root, Buffer.alloc(8)); this.install(heap);
  }
  alloc(b) { const at = this.cursor; this.cursor += BigInt((b.length + 15) & ~15); this.memory.set(at, Buffer.from(b)); return at; }
  string(b) {
    if (typeof b === "string") b = Buffer.from(b);
    const h = Buffer.alloc(32); h[16] = 4; h.writeUInt32LE(hash(b.toString()), 20); h.writeBigUInt64LE(BigInt(b.length), 24);
    return this.alloc(Buffer.concat([h, b]));
  }
  table(values) {
    const count = 256, b = Buffer.alloc(count * 56), reserved = new Set(Object.keys(values).map(k => hash(k) & 255)), used = new Set(), homes = new Map(), addresses = {};
    const nodes = this.alloc(b);
    for (const [name, value] of Object.entries(values)) {
      const home = hash(name) & 255; let index = home;
      if (used.has(home)) { index = 0; while (used.has(index) || reserved.has(index)) index++; }
      used.add(index); const at = index * 56, key = this.string(name);
      b.writeBigUInt64LE(value.pointer, at); b[at + 8] = value.tag; b.writeBigUInt64LE(key, at + 24); b[at + 32] = 4;
      if (homes.has(home)) b.writeBigUInt64LE(nodes + BigInt(at), homes.get(home) * 56 + 48);
      homes.set(home, index); addresses[name] = { node: nodes + BigInt(at), key, ...value };
    }
    this.memory.set(nodes, b); const t = Buffer.alloc(72); t[16] = 5; t[19] = 8; t.writeBigUInt64LE(nodes, 40);
    return { pointer: this.alloc(t), tag: 5, nodes, addresses };
  }
  install(heap, sequence = 1, collision = false) {
    this.cursor = heap;
    const runtime = "11".repeat(16);
    this.identity = { schema: "lycheedev.slot.identity.v2", runtime, owner: "", fence: 0, nextSlot: 1, slots: 200, inventory: 0, inputState: "lycheedev.input-signal.v1", character: "Fixture", realm: "Synthetic", guid: "synthetic-guid", build: "12.1.0.69933", product: "retail", release: "3.1.0" };
    this.input = { schema: "lycheedev.input.v2", runtime, owner: this.identity.owner, fence: this.identity.fence, nextSlot: 1, guid: this.identity.guid, build: this.identity.build, sampleMillis: 1000 + sequence, inputBlocked: false, reason: "" };
    const values = { schema: { pointer: this.string("lycheedev.mailbox.v1"), tag: 4 }, release: { pointer: this.string("3.1.0"), tag: 4 }, runtime: { pointer: this.string(runtime), tag: 4 } };
    if (collision) { let key = ""; for (let n = 0; ; n++) { key = `decoy${n}`; if ((hash(key) & 255) === (hash("identity") & 255)) break; } values[key] = { pointer: this.string("old decoy bytes"), tag: 4 }; this.collisionKey = key; }
    values.identity = { pointer: this.string(encodeRecord(this.identity, 1)), tag: 4 };
    values.input = { pointer: this.string(encodeRecord(this.input, 5, sequence)), tag: 4 };
    this.box = this.table(values); this.namespace = this.table({ Mailbox: this.box }); this.globals = this.table({ LycheeDevInternal: this.namespace });
    const state = Buffer.alloc(0xa8); state[16] = 8; state.writeBigUInt64LE(this.globals.pointer, 0x90); state[0x98] = 5;
    this.state = this.alloc(state); this.memory.get(this.root).writeBigUInt64LE(this.state);
  }
  locate(address, size) {
    for (const [at, b] of this.memory) if (address >= at && address + BigInt(size) <= at + BigInt(b.length)) return { b, offset: Number(address - at) };
    throw new Error("synthetic missing bytes");
  }
  bytes(address, size) { const { b, offset } = this.locate(address, size); return Buffer.from(b.subarray(offset, offset + size)); }
  mutate(address, b) { const found = this.locate(address, b.length); b.copy(found.b, found.offset); }
  context() {
    const f = this;
    return { buildKey: binding.buildKey, async module() { return f.module; }, async readExact(address, size) {
      f.reads.push({ address, size }); await f.beforeRead?.(address, size, f.reads.length);
      return f.bytes(address, size);
    }, async emit(value) { f.emitted = value; }, async regions() { throw new Error("forbidden Regions"); }, async scan() { throw new Error("forbidden scan"); }, async find() { throw new Error("forbidden find"); } };
  }
  json(expect) { return { module: this.module, memory: Object.fromEntries([...this.memory].map(([at, b]) => [hex(at), b.toString("hex")])), expect: [expect] }; }
  replace(name, value, kind, sequence = 2) {
    return this.publish(name, encodeRecord(value, kind, sequence));
  }
  publish(name, wire) { const at = this.string(wire), p = Buffer.alloc(8); p.writeBigUInt64LE(at); this.mutate(this.box.addresses[name].node, p); return at; }
}
