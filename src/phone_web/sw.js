// Keeps the app shell available when the PC can't be reached, so the phone app still opens
// (and can queue entries). API requests always go to the network.
const CACHE = "morrowkeep-shell-v6";
const SHELL = ["/", "/app.js", "/plan.js", "/foods.js", "/train.js", "/manifest.webmanifest", "/icon.png"];

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (e) => {
  e.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.pathname.startsWith("/api/")) return;
  // network first, so updates from the PC show up at once; the cache when offline
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        const copy = res.clone();
        caches.open(CACHE).then((c) => c.put(url.pathname === "/index.html" ? "/" : url.pathname, copy));
        return res;
      })
      .catch(() => caches.match(url.pathname).then((r) => r || caches.match("/")))
  );
});

// ---------------------------------------------------------------- notifications from the PC

self.addEventListener("push", (e) => {
  let d = {};
  try { d = e.data ? e.data.json() : {}; } catch (_) { d = { body: e.data ? e.data.text() : "" }; }
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
