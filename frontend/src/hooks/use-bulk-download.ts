import { useState, useRef, useCallback } from "react"
import { downloadVideo, getDownload } from "@/lib/offline"
import type { VideoItem } from "@/types"

export interface BulkProgress {
  done: number
  total: number
  current: string
  currentPct: number
}

// Sequentially downloads videos not yet cached; skips non-ready and already-downloaded ones.
export function useBulkDownload() {
  const [progress, setProgress] = useState<BulkProgress | null>(null)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  const start = useCallback(async (videos: VideoItem[]) => {
    const todo = videos.filter((v) => v.status === "ready" && !getDownload(v.id))
    if (!todo.length) return
    const controller = new AbortController()
    abortRef.current = controller
    setError(null)
    try {
      for (let i = 0; i < todo.length; i++) {
        const v = todo[i]
        setProgress({ done: i, total: todo.length, current: v.title, currentPct: 0 })
        await downloadVideo(v.id, v.title, (d, t) => {
          setProgress({ done: i, total: todo.length, current: v.title, currentPct: t ? d / t : 0 })
        }, controller.signal)
      }
    } catch (err) {
      if (!(err instanceof DOMException && err.name === "AbortError")) {
        setError(err instanceof Error ? err.message : "Download failed")
      }
    } finally {
      setProgress(null)
      abortRef.current = null
    }
  }, [])

  const cancel = useCallback(() => abortRef.current?.abort(), [])

  return { progress, error, start, cancel }
}
