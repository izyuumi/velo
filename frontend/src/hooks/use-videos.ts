import { useState, useEffect, useCallback } from "react"
import type { VideoItem, StatusResponse } from "@/types"

export function useVideos() {
  const [videos, setVideos] = useState<VideoItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetchVideos = useCallback(async () => {
    try {
      const resp = await fetch("/api/videos")
      if (!resp.ok) throw new Error("Failed to fetch videos")
      const data = await resp.json()
      setVideos(data)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error")
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchVideos()
    const interval = setInterval(fetchVideos, 5000)
    return () => clearInterval(interval)
  }, [fetchVideos])

  return { videos, loading, error, refetch: fetchVideos }
}

export function useVideoStatus(id: string) {
  const [status, setStatus] = useState<StatusResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const fetchStatus = useCallback(async () => {
    try {
      const resp = await fetch(`/api/status/${id}`)
      if (!resp.ok) {
        if (resp.status === 404) {
          setError("Video not found")
          return
        }
        throw new Error("Failed to fetch status")
      }
      const data = await resp.json()
      setStatus(data)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error")
    } finally {
      setLoading(false)
    }
  }, [id])

  useEffect(() => {
    fetchStatus()
    const interval = setInterval(() => {
      if (status?.status !== "ready") {
        fetchStatus()
      }
    }, 2000)
    return () => clearInterval(interval)
  }, [fetchStatus, status?.status])

  return { status, loading, error, refetch: fetchStatus }
}

