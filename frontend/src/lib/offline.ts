import { appUrl } from "@/lib/url"

const MEDIA = "velo-media" // must match public/sw.js
const KEY = "velo:downloads"
const MAX_WIDTH = 1280 // pick the best variant at or below this width

export interface DownloadEntry {
  title: string
  variant: string // e.g. "v1/prog_index.m3u8"
}

export function listDownloads(): Record<string, DownloadEntry> {
  try {
    return JSON.parse(localStorage.getItem(KEY) || "{}")
  } catch {
    return {}
  }
}

export function getDownload(id: string): DownloadEntry | undefined {
  return listDownloads()[id]
}

function save(downloads: Record<string, DownloadEntry>) {
  localStorage.setItem(KEY, JSON.stringify(downloads))
}

export function offlineSupported(): boolean {
  return "serviceWorker" in navigator && "caches" in window
}

function pickVariant(master: string): string {
  const lines = master.split("\n").map((l) => l.trim())
  const variants: { width: number; uri: string }[] = []
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].startsWith("#EXT-X-STREAM-INF")) continue
    const width = Number(/RESOLUTION=(\d+)x/.exec(lines[i])?.[1] ?? 0)
    const uri = lines.slice(i + 1).find((l) => l && !l.startsWith("#"))
    if (uri) variants.push({ width, uri })
  }
  if (!variants.length) throw new Error("No variants in master playlist")
  const fitting = variants.filter((v) => v.width <= MAX_WIDTH).sort((a, b) => b.width - a.width)
  return (fitting[0] ?? variants[variants.length - 1]).uri
}

async function fetchAndCache(cache: Cache, url: string): Promise<Response> {
  const res = await fetch(url, { cache: "no-store" })
  if (!res.ok) throw new Error(`Failed to fetch ${url} (${res.status})`)
  await cache.put(url, res.clone())
  return res
}

export async function downloadVideo(
  id: string,
  title: string,
  onProgress?: (done: number, total: number) => void,
  signal?: AbortSignal
): Promise<void> {
  const cache = await caches.open(MEDIA)
  const masterUrl = appUrl(`/hls/${id}/master.m3u8`)
  const master = await (await fetchAndCache(cache, masterUrl)).text()
  const variant = pickVariant(master)
  const variantDir = variant.split("/").slice(0, -1).join("/")
  const playlistUrl = appUrl(`/hls/${id}/${variant}`)
  const playlist = await (await fetchAndCache(cache, playlistUrl)).text()

  const segments = playlist
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"))
    .map((seg) => appUrl(`/hls/${id}/${variantDir ? `${variantDir}/` : ""}${seg}`))

  // Metadata so the watch page renders offline.
  await Promise.allSettled([
    fetchAndCache(cache, appUrl(`/hls/${id}/thumb.jpg`)),
    caches.open("velo-api").then((c) => fetch(appUrl(`/api/status/${id}`)).then((r) => c.put(appUrl(`/api/status/${id}`), r))),
  ])

  let done = 0
  onProgress?.(0, segments.length)
  const queue = [...segments]
  const worker = async () => {
    while (queue.length) {
      if (signal?.aborted) throw new DOMException("Aborted", "AbortError")
      const url = queue.shift()!
      if (!(await cache.match(url))) await fetchAndCache(cache, url)
      onProgress?.(++done, segments.length)
    }
  }
  await Promise.all(Array.from({ length: 4 }, worker))

  save({ ...listDownloads(), [id]: { title, variant } })
}

export async function removeDownload(id: string): Promise<void> {
  const cache = await caches.open(MEDIA)
  const prefix = appUrl(`/hls/${id}/`)
  const keys = (await cache.keys()).filter((req) => new URL(req.url).pathname.startsWith(prefix))
  for (let i = 0; i < keys.length; i += 32) {
    await Promise.all(keys.slice(i, i + 32).map((req) => cache.delete(req)))
  }
  const downloads = listDownloads()
  delete downloads[id]
  save(downloads)
}
