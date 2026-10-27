// Offline support: app shell + API fallback + explicitly downloaded HLS media.
const SHELL = "velo-shell"
const API = "velo-api"
const MEDIA = "velo-media" // populated by src/lib/offline.ts
const base = new URL(self.registration.scope).pathname.replace(/\/$/, "")

self.addEventListener("install", () => self.skipWaiting())
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()))

self.addEventListener("fetch", (event) => {
  const req = event.request
  if (req.method !== "GET") return
  const url = new URL(req.url)
  if (url.origin !== self.location.origin || !url.pathname.startsWith(base)) return
  const path = url.pathname.slice(base.length)

  if (path.startsWith("/hls/")) {
    event.respondWith(caches.match(req, { cacheName: MEDIA }).then((r) => r || fetch(req)))
  } else if (path.startsWith("/api/")) {
    event.respondWith(networkFirst(req, API))
  } else if (req.mode === "navigate") {
    event.respondWith(networkFirst(req, SHELL, `${base}/`))
  } else if (path.startsWith("/assets/") || path.endsWith(".svg")) {
    event.respondWith(cacheFirst(req, SHELL))
  }
})

async function networkFirst(req, cacheName, key = req) {
  const cache = await caches.open(cacheName)
  try {
    const res = await fetch(req)
    if (res.ok) cache.put(key, res.clone())
    return res
  } catch {
    const hit = await cache.match(key)
    return hit || Response.error()
  }
}

async function cacheFirst(req, cacheName) {
  const cache = await caches.open(cacheName)
  const hit = await cache.match(req)
  if (hit) return hit
  const res = await fetch(req)
  if (res.ok) cache.put(req, res.clone())
  return res
}
