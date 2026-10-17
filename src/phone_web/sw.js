// Keeps the app shell available when the PC can't be reached, so the phone app still opens
// (and can queue entries). API requests always go to the network.
const CACHE = "morrowkeep-shell-v1";
const SHELL = ["/", "/app.js", "/manifest.webmanifest", "/icon.png"];

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
