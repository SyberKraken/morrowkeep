// The phone's own copy of your data, so the app works without the PC: kept in IndexedDB, loaded
// into Morrowkeep's rules (core.wasm), and every screen is drawn from it, with the changes still
// waiting for the PC applied on top. Online, a tiny version check tells when to fetch a new copy.
"use strict";

const IDB = {
  db: null,
  open() {
    if (IDB.db) return Promise.resolve(IDB.db);
    return new Promise((ok, fail) => {
      const rq = indexedDB.open("morrowkeep", 1);
      rq.onupgradeneeded = () => {
        rq.result.createObjectStore("kv");
        rq.result.createObjectStore("blobs");
      };
      rq.onsuccess = () => ok((IDB.db = rq.result));
      rq.onerror = () => fail(rq.error);
    });
  },
  async tx(store, mode, fn) {
    const db = await IDB.open();
    return new Promise((ok, fail) => {
      const t = db.transaction(store, mode);
      const rq = fn(t.objectStore(store));
      t.oncomplete = () => ok(rq && rq.result);
      t.onerror = () => fail(t.error);
    });
  },
  get: (key, store = "kv") => IDB.tx(store, "readonly", (s) => s.get(key)),
  set: (key, val, store = "kv") => IDB.tx(store, "readwrite", (s) => s.put(val, key)),
  del: (key, store = "kv") => IDB.tx(store, "readwrite", (s) => s.delete(key)),
};

const RP = { loaded: false, takenAt: null, version: null, pc: null, fetching: null };

/// The steps applied on top of the PC's copy (the outbox adds itself here).
let replicaSteps = () => [];

/// Loads the saved copy (if any) at start; works offline.
async function replicaInit() {
  try { if (navigator.storage && navigator.storage.persist) await navigator.storage.persist(); } catch (_) {}
  if (!MK.wasm) return;
  try {
    const saved = await IDB.get("snapshot");
    if (saved) replicaUse(saved);
  } catch (e) { console.warn("replica", e); }
}

function replicaUse(r) {
  MK.kinds = (r.pc.categories || []).map((c) => c.name);
  const res = MK.call({ fn: "load", snapshot: r.snapshot, pc: r.pc });
  if (res.error) throw new Error(res.error);
  Object.assign(RP, { loaded: true, takenAt: r.taken_at, version: r.version, pc: r.pc });
  if (r.pc.theme) applyTheme(r.pc.theme);
  replicaProject();
}

/// Re-applies the waiting steps; returns each step's result.
function replicaProject() {
  if (!RP.loaded) return [];
  return MK.call({ fn: "set_steps", steps: replicaSteps(), today: localDate() }).results || [];
}

/// Asks the PC whether anything changed, and fetches a new copy if so. Throws when the PC can't
/// be reached (the caller shows "PC offline").
async function replicaSync(force) {
  if (RP.fetching) return RP.fetching;
  RP.fetching = (async () => {
    if (!force && RP.loaded && MK.wasm) {
      const v = await api("/api/sync/version", { timeout: 6000 });
      if (v.version === RP.version) return false;
    }
    const r = await api("/api/sync/snapshot", { timeout: 20000 });
    if (MK.wasm) {
      replicaUse(r);
      await IDB.set("snapshot", r);
    }
    return true;
  })();
  try { return await RP.fetching; } finally { RP.fetching = null; }
}

/// A screen from the phone's copy; `fallback` is the PC's URL for it when there's no copy yet.
async function screenData(name, args, fallback) {
  if (RP.loaded) {
    const r = MK.call(Object.assign({ fn: "view", name, today: localDate() }, args || {}));
    if (r && r.error && Object.keys(r).length === 1) throw Object.assign(new Error(r.error), { server: true });
    return r;
  }
  return api(fallback);
}

/// "as of 14:02" for the header when the copy is old and the PC can't be reached.
function replicaAge() {
  if (!RP.takenAt) return "";
  const t = new Date(RP.takenAt);
  return t.toDateString() === new Date().toDateString() ? localTime(t) : t.toLocaleDateString(undefined, { day: "numeric", month: "short" }) + " " + localTime(t);
}
