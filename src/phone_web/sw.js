// Keeps the app's files on the phone, so it opens at once, also without the PC (Tailscale off,
// PC asleep, Morrowkeep closed). Cache first: the saved files are used straight away and refreshed
// in the background; a new version (new CACHE name) installs as a whole set and is used from the
// next open. API requests always go to the network.
const CACHE = "morrowkeep-shell-v14";
const SHELL = ["/", "/core.js", "/core.wasm", "/app.js", "/replica.js", "/outbox.js", "/plan.js", "/foods.js", "/train.js", "/progress.js", "/journal.js", "/manifest.webmanifest", "/icon.png"];

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (e) => {
  e.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.pathname.startsWith("/api/")) return;
  const key = url.pathname === "/index.html" ? "/" : url.pathname;
  const fresh = fetch(e.request)
    .then((res) => {
      if (res.ok) {
        const copy = res.clone();
        caches.open(CACHE).then((c) => c.put(key, copy));
      }
      return res;
    })
    .catch(() => null);
  e.waitUntil(fresh);
  e.respondWith(
    caches.match(key).then((cached) => cached || fresh.then((res) => res || caches.match("/")))
  );
});

// ---------------------------------------------------------------- notifications from the PC

// ---- "Morrowkeep is on": send what's waiting on the phone, straight from here

function idb() {
  return new Promise((ok, fail) => {
    const rq = indexedDB.open("morrowkeep", 1);
    rq.onupgradeneeded = () => { rq.result.createObjectStore("kv"); rq.result.createObjectStore("blobs"); };
    rq.onsuccess = () => ok(rq.result);
    rq.onerror = () => fail(rq.error);
  });
}
function idbDo(db, store, mode, fn) {
  return new Promise((ok, fail) => {
    const t = db.transaction(store, mode);
    const rq = fn(t.objectStore(store));
    t.oncomplete = () => ok(rq && rq.result);
    t.onerror = () => fail(t.error);
  });
}

/// Sends the waiting steps (photos and recordings first); returns how many went.
async function sendWaiting() {
  const db = await idb();
  const steps = (await idbDo(db, "kv", "readonly", (s) => s.get("outbox"))) || [];
  const waiting = steps.filter((x) => x.state === "waiting");
  if (!waiting.length) return 0;
  const token = await caches.open("mk-auth").then((c) => c.match("/__token")).then((r) => (r ? r.text() : ""));
  const auth = { Authorization: "Bearer " + token };
  for (const x of waiting.filter((x) => x.blob && !x.blobSent)) {
    const b = await idbDo(db, "blobs", "readonly", (s) => s.get(x.id));
    if (b) {
      const r = await fetch("/api/sync/blob?id=" + encodeURIComponent(x.id), { method: "POST", headers: Object.assign({ "Content-Type": b.type || "application/octet-stream" }, auth), body: b });
      if (!r.ok) throw new Error("upload");
    }
    x.blobSent = true;
  }
  const r = await fetch("/api/sync/push", { method: "POST", headers: Object.assign({ "Content-Type": "application/json" }, auth),
    body: JSON.stringify({ steps: waiting.map(({ id, seq, at, date, time, step }) => ({ id, seq, at, date, time, step })) }) });
  if (!r.ok) throw new Error("push");
  const res = await r.json();
  for (const x of waiting) if (res.accepted.includes(x.id)) x.state = "sent";
  await idbDo(db, "kv", "readwrite", (s) => s.put(steps, "outbox"));
  return waiting.length;
}

async function pcIsOn() {
  let waiting = 0;
  try {
    const db = await idb();
    const steps = (await idbDo(db, "kv", "readonly", (s) => s.get("outbox"))) || [];
    waiting = steps.filter((x) => x.state === "waiting").length;
  } catch (_) {}
  const show = (body) => self.registration.showNotification("Morrowkeep is on", { body, icon: "/icon.png", badge: "/icon.png", tag: "pc-on", data: { go: "outbox" } });
  if (!waiting) {
    // nothing to send: stay quiet (iPhones need every push to show something)
    if (/iPhone|iPad/.test(self.navigator.userAgent)) await show("Nothing waiting on the phone.");
    return;
  }
  await show(`Sending ${waiting} change${waiting === 1 ? "" : "s"} you made on the phone…`);
  try {
    const n = await sendWaiting();
    await show(`${n} change${n === 1 ? "" : "s"} sent to your PC. Tap to see what it did with them.`);
  } catch (_) {
    await show(`${waiting} change${waiting === 1 ? "" : "s"} waiting. Tap to send (is Tailscale on?).`);
  }
}

self.addEventListener("push", (e) => {
  let d = {};
  try { d = e.data ? e.data.json() : {}; } catch (_) { d = { body: e.data ? e.data.text() : "" }; }
  if (d.type === "pc_on") return e.waitUntil(pcIsOn());
  const opts = {
    body: d.body || "",
    icon: "/icon.png",
    badge: "/icon.png",
    tag: d.tag || "morrowkeep",
    renotify: true,
    data: { go: d.go || "today" },
  };
  // Android shows buttons; iPhone ignores them (a tap opens the app)
  if (d.actions) opts.actions = [{ action: "open", title: "Log" }, { action: "snooze", title: "Snooze 2h" }];
  e.waitUntil(self.registration.showNotification(d.title || "Morrowkeep", opts));
});

self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  if (e.action === "snooze") {
    e.waitUntil(
      caches.open("mk-auth")
        .then((c) => c.match("/__token"))
        .then((r) => (r ? r.text() : ""))
        .then((token) => fetch("/api/snooze", { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer " + token }, body: JSON.stringify({ hours: 2 }) }))
        .catch(() => {})
    );
    return;
  }
  const url = "/?go=" + encodeURIComponent((e.notification.data && e.notification.data.go) || "today");
  e.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((list) => {
      for (const c of list) {
        if ("focus" in c) { c.navigate(url).catch(() => {}); return c.focus(); }
      }
      return self.clients.openWindow(url);
    })
  );
});
