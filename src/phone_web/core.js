// Morrowkeep's rules on the phone: the same Rust code as the PC (morrowkeep-core), compiled to
// WebAssembly. `MK.call({fn: "plan_entry", ...})` sends a JSON request and returns the answer;
// the phone's time, some randomness for new ids, and your category names go along every time.
"use strict";

const MK = {
  wasm: null,
  kinds: [],
  /// Loads the module from its bytes (the app fetches /core.wasm; tests read the file).
  async load(bytes) {
    const { instance } = await WebAssembly.instantiate(bytes, {});
    MK.wasm = instance.exports;
  },
  call(req) {
    const w = MK.wasm;
    if (!w) throw new Error("Morrowkeep's core isn't loaded");
    const rnd = new Uint32Array(2);
    (globalThis.crypto || require("crypto").webcrypto).getRandomValues(rnd);
    const full = Object.assign({ now_ms: Date.now(), tz_min: -new Date().getTimezoneOffset(), seed: rnd[0] * 4294967296 + rnd[1], kinds: MK.kinds }, req);
    const input = new TextEncoder().encode(JSON.stringify(full));
    const ptr = w.mk_alloc(input.length);
    new Uint8Array(w.memory.buffer, ptr, input.length).set(input);
    const out = w.mk_call(ptr, input.length);
    const len = new DataView(w.memory.buffer).getUint32(out, true);
    const text = new TextDecoder().decode(new Uint8Array(w.memory.buffer, out + 4, len));
    w.mk_free(out, len + 4);
    return JSON.parse(text);
  },
};
if (typeof module !== "undefined") module.exports = MK;
