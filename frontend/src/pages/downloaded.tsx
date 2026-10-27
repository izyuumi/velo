import { useState } from "react"
import { Link } from "react-router-dom"
import { Download, Loader2, Play, Trash2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { listDownloads, removeDownload } from "@/lib/offline"

export function DownloadedPage() {
  const [downloads, setDownloads] = useState(listDownloads)
  const [removing, setRemoving] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const entries = Object.entries(downloads)

  async function remove(id: string) {
    setRemoving(id)
    setError(null)
    try {
      await removeDownload(id)
      setDownloads(listDownloads())
    } catch {
      setError("Could not remove download. Try again.")
    } finally {
      setRemoving(null)
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Downloaded</h1>
        <p className="text-sm text-muted-foreground mt-1">Saved on this device for offline playback.</p>
      </div>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      {entries.length === 0 ? (
        <div className="py-16 text-center space-y-3">
          <Download className="h-8 w-8 mx-auto text-muted-foreground" />
          <p className="text-muted-foreground">No downloaded videos on this device.</p>
          <Button asChild variant="outline"><Link to="/">Browse videos</Link></Button>
        </div>
      ) : (
        <ul className="divide-y rounded-lg border">
          {entries.map(([id, entry]) => (
            <li key={id} className="flex items-center gap-3 p-4">
              <Link to={`/watch/${id}`} className="flex min-w-0 flex-1 items-center gap-3 hover:underline">
                <Play className="h-4 w-4 shrink-0" />
                <span className="truncate">{entry.title}</span>
              </Link>
              <Button variant="ghost" size="sm" disabled={removing === id} onClick={() => remove(id)} aria-label={`Remove ${entry.title} download`}>
                {removing === id ? <Loader2 className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
                {removing === id ? "Removing…" : "Remove"}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
