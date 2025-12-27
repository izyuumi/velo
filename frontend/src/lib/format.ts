export function formatDuration(seconds?: number): string {
  if (!seconds || seconds <= 0) return "--:--"
  const m = Math.floor(seconds / 60)
  const s = Math.floor(seconds % 60)
    .toString()
    .padStart(2, "0")
  return `${m}:${s}`
}

export function formatDate(timestamp: number): string {
  if (!timestamp) return "--"
  const d = new Date(timestamp * 1000)
  return d.toLocaleDateString()
}

export function formatRelativeTime(timestamp: number): string {
  if (!timestamp) return "--"
  const now = Date.now()
  const diff = now - timestamp * 1000
  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)

  if (days > 0) return `${days}d ago`
  if (hours > 0) return `${hours}h ago`
  if (minutes > 0) return `${minutes}m ago`
  return "just now"
}

