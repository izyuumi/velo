import { useState, useEffect, useCallback } from "react"
import { Link } from "react-router-dom"
import { Upload, Loader2, Video, AlertCircle } from "lucide-react"
import { VideoCard } from "@/components/video-card"
import { Button } from "@/components/ui/button"
import type { VideoItem } from "@/types"

export function IndexPage() {
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

  const processingVideos = videos.filter((v) => v.status === "processing")
  const readyVideos = videos.filter((v) => v.status === "ready")
  const errorVideos = videos.filter((v) => v.status === "error")

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[50vh]">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4">
        <AlertCircle className="h-10 w-10 text-destructive" />
        <p className="text-destructive">{error}</p>
        <Button onClick={fetchVideos} variant="outline">
          Try Again
        </Button>
      </div>
    )
  }

  if (videos.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4">
        <Video className="h-10 w-10 text-muted-foreground" />
        <div className="text-center">
          <h2 className="text-lg font-semibold">No videos yet</h2>
          <p className="text-sm text-muted-foreground">Upload your first video to get started</p>
        </div>
        <Button asChild>
          <Link to="/upload">
            <Upload className="h-4 w-4" />
            Upload Video
          </Link>
        </Button>
      </div>
    )
  }

  return (
    <div className="space-y-8">
      {processingVideos.length > 0 && (
        <section className="space-y-4">
          <div className="flex items-center gap-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            <h2 className="font-semibold">Processing ({processingVideos.length})</h2>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {processingVideos.map((video) => (
              <VideoCard key={video.id} video={video} />
            ))}
          </div>
        </section>
      )}

      {readyVideos.length > 0 && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          {readyVideos.map((video) => (
            <VideoCard key={video.id} video={video} />
          ))}
        </div>
      )}

      {errorVideos.length > 0 && (
        <section className="space-y-4">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-4 w-4 text-destructive" />
            <h2 className="font-semibold">Failed ({errorVideos.length})</h2>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {errorVideos.map((video) => (
              <VideoCard key={video.id} video={video} />
            ))}
          </div>
        </section>
      )}
    </div>
  )
}
