import { useState, useEffect, useCallback, useRef } from "react"
import { useParams, useNavigate, Link } from "react-router-dom"
import Hls from "hls.js"
import { ArrowLeft, Loader2, AlertCircle, Edit3, Trash2, RefreshCw, Clock, Monitor, ChevronLeft, ChevronRight, List, X, Play } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"
import { formatDuration, formatDate } from "@/lib/format"
import { renameVideo, deleteVideo, reprocessVideo, updateWatchProgress, fetchSeriesDetail } from "@/lib/api"
import type { StatusResponse, VideoItem } from "@/types"

const PROGRESS_SAVE_INTERVAL = 10000
const AUTOPLAY_COUNTDOWN = 10

export function WatchPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const videoRef = useRef<HTMLVideoElement>(null)
  const hlsRef = useRef<Hls | null>(null)
  const progressIntervalRef = useRef<number | null>(null)

  const [status, setStatus] = useState<StatusResponse | null>(null)
  const [seriesEpisodes, setSeriesEpisodes] = useState<VideoItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [playerError, setPlayerError] = useState<string | null>(null)

  const [isEditing, setIsEditing] = useState(false)
  const [editTitle, setEditTitle] = useState("")
  const [isSaving, setIsSaving] = useState(false)
  const [isDeleting, setIsDeleting] = useState(false)
  const [isReprocessing, setIsReprocessing] = useState(false)

  const [showEpisodeList, setShowEpisodeList] = useState(false)
  const [showAutoplay, setShowAutoplay] = useState(false)
  const [autoplayCountdown, setAutoplayCountdown] = useState(AUTOPLAY_COUNTDOWN)

  const fetchStatus = useCallback(async () => {
    if (!id) return
    try {
      const resp = await fetch(`/api/status/${id}`)
      if (!resp.ok) {
        if (resp.status === 404) {
          setError("Video not found")
          return
        }
        throw new Error("Failed to fetch status")
      }
      const data: StatusResponse = await resp.json()
      setStatus(data)
      setEditTitle(data.title)
      setError(null)

      if (data.series_id) {
        const seriesData = await fetchSeriesDetail(data.series_id)
        setSeriesEpisodes(seriesData.episodes)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error")
    } finally {
      setLoading(false)
    }
  }, [id])

  useEffect(() => {
    fetchStatus()
  }, [fetchStatus])

  useEffect(() => {
    if (!status || status.status === "ready") return
    const interval = setInterval(fetchStatus, 2000)
    return () => clearInterval(interval)
  }, [status, fetchStatus])

  useEffect(() => {
    if (!status || status.status !== "ready" || !videoRef.current) return

    const video = videoRef.current
    const src = `/hls/${id}/master.m3u8`

    const resumePosition = status.watch_position && status.watch_position > 0 && !status.watch_completed
      ? status.watch_position
      : null

    const handleLoadedMetadata = () => {
      if (resumePosition !== null) {
        video.currentTime = resumePosition
      }
    }

    video.addEventListener("loadedmetadata", handleLoadedMetadata)

    if (video.canPlayType("application/vnd.apple.mpegurl")) {
      video.src = src
    } else if (Hls.isSupported()) {
      const hls = new Hls()
      hlsRef.current = hls
      hls.loadSource(src)
      hls.attachMedia(video)
      hls.on(Hls.Events.ERROR, (_, data) => {
        if (data.fatal) {
          setPlayerError("Playback failed. Try refreshing.")
        }
      })
      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        if (resumePosition !== null) {
          video.currentTime = resumePosition
        }
      })
    } else {
      setPlayerError("Your browser doesn't support video playback.")
    }

    return () => {
      video.removeEventListener("loadedmetadata", handleLoadedMetadata)
      if (hlsRef.current) {
        hlsRef.current.destroy()
        hlsRef.current = null
      }
    }
  }, [status, id])

  useEffect(() => {
    if (!status || status.status !== "ready" || !videoRef.current || !id) return

    const saveProgress = async () => {
      const video = videoRef.current
      if (!video || video.paused || video.ended) return

      const position = Math.floor(video.currentTime)
      const duration = video.duration || status.duration_seconds || 0
      const completed = duration > 0 && position >= duration * 0.9

      try {
        await updateWatchProgress(id, position, completed)
      } catch {
        console.error("Failed to save progress")
      }
    }

    progressIntervalRef.current = window.setInterval(saveProgress, PROGRESS_SAVE_INTERVAL)

    return () => {
      if (progressIntervalRef.current) {
        clearInterval(progressIntervalRef.current)
      }
    }
  }, [status, id])

  useEffect(() => {
    const video = videoRef.current
    if (!video || !id) return

    const handleEnded = async () => {
      const duration = video.duration || status?.duration_seconds || 0
      try {
        await updateWatchProgress(id, Math.floor(duration), true)
      } catch {
        console.error("Failed to mark as watched")
      }

      if (status?.next_episode_id) {
        setShowAutoplay(true)
        setAutoplayCountdown(AUTOPLAY_COUNTDOWN)
      }
    }

    video.addEventListener("ended", handleEnded)
    return () => video.removeEventListener("ended", handleEnded)
  }, [status, id])

  useEffect(() => {
    const nextId = status?.next_episode_id
    if (!showAutoplay || !nextId) return

    const timer = setInterval(() => {
      setAutoplayCountdown((prev) => {
        if (prev <= 1) {
          navigate(`/watch/${nextId}`)
          return 0
        }
        return prev - 1
      })
    }, 1000)

    return () => clearInterval(timer)
  }, [showAutoplay, status?.next_episode_id, navigate])

  const handleSave = async () => {
    if (!id || !editTitle.trim()) return
    setIsSaving(true)
    try {
      const updated = await renameVideo(id, editTitle.trim())
      setStatus(updated)
      setIsEditing(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save")
    } finally {
      setIsSaving(false)
    }
  }

  const handleDelete = async () => {
    if (!id) return
    setIsDeleting(true)
    try {
      await deleteVideo(id)
      navigate("/")
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete")
      setIsDeleting(false)
    }
  }

  const handleReprocess = async () => {
    if (!id) return
    setIsReprocessing(true)
    try {
      const updated = await reprocessVideo(id)
      setStatus(updated)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to reprocess")
    } finally {
      setIsReprocessing(false)
    }
  }

  const cancelAutoplay = () => {
    setShowAutoplay(false)
    setAutoplayCountdown(AUTOPLAY_COUNTDOWN)
  }

  const playNextNow = () => {
    if (status?.next_episode_id) {
      navigate(`/watch/${status.next_episode_id}`)
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[50vh]">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (error || !status) {
    return (
      <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4">
        <AlertCircle className="h-10 w-10 text-destructive" />
        <p className="text-destructive">{error || "Video not found"}</p>
        <Button asChild variant="outline">
          <Link to="/">
            <ArrowLeft className="h-4 w-4" />
            Back to Home
          </Link>
        </Button>
      </div>
    )
  }

  const isProcessing = status.status === "processing"
  const isError = status.status === "error"
  const isReady = status.status === "ready"
  const nextEpisode = status.next_episode_id ? seriesEpisodes.find((e) => e.id === status.next_episode_id) : null

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" asChild>
          <Link to={status.series_id ? `/series/${status.series_id}` : "/"}>
            <ArrowLeft className="h-4 w-4" />
            {status.series_title || "Back"}
          </Link>
        </Button>

        {seriesEpisodes.length > 0 && (
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={!status.prev_episode_id}
              asChild={!!status.prev_episode_id}
            >
              {status.prev_episode_id ? (
                <Link to={`/watch/${status.prev_episode_id}`}>
                  <ChevronLeft className="h-4 w-4" />
                  Prev
                </Link>
              ) : (
                <>
                  <ChevronLeft className="h-4 w-4" />
                  Prev
                </>
              )}
            </Button>

            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowEpisodeList(!showEpisodeList)}
            >
              <List className="h-4 w-4" />
              {status.episode_number}/{seriesEpisodes.length}
            </Button>

            <Button
              variant="outline"
              size="sm"
              disabled={!status.next_episode_id}
              asChild={!!status.next_episode_id}
            >
              {status.next_episode_id ? (
                <Link to={`/watch/${status.next_episode_id}`}>
                  Next
                  <ChevronRight className="h-4 w-4" />
                </Link>
              ) : (
                <>
                  Next
                  <ChevronRight className="h-4 w-4" />
                </>
              )}
            </Button>
          </div>
        )}
      </div>

      <div className="space-y-3">
        {isEditing ? (
          <div className="flex items-center gap-2">
            <Input
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              className="text-lg font-bold"
              autoFocus
            />
            <Button onClick={handleSave} disabled={isSaving} size="sm">
              {isSaving ? <Loader2 className="h-4 w-4 animate-spin" /> : "Save"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setIsEditing(false)
                setEditTitle(status.title)
              }}
            >
              Cancel
            </Button>
          </div>
        ) : (
          <div className="flex items-start justify-between gap-4">
            <div>
              {status.series_title && status.episode_number && (
                <Badge variant="secondary" className="mb-2">
                  Episode {status.episode_number}
                </Badge>
              )}
              <h1 className="text-xl font-bold">{status.title}</h1>
            </div>
            <div className="flex items-center gap-1 shrink-0">
              <Button variant="ghost" size="icon" onClick={() => setIsEditing(true)}>
                <Edit3 className="h-4 w-4" />
              </Button>
              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button variant="ghost" size="icon">
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Delete Video</AlertDialogTitle>
                    <AlertDialogDescription>
                      This will permanently delete the video. This action cannot be undone.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction onClick={handleDelete} disabled={isDeleting}>
                      {isDeleting ? <Loader2 className="h-4 w-4 animate-spin" /> : "Delete"}
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </div>
          </div>
        )}

        <div className="flex items-center gap-4 text-sm text-muted-foreground">
          {status.width && status.height && (
            <span className="flex items-center gap-1">
              <Monitor className="h-4 w-4" />
              {status.width}×{status.height}
            </span>
          )}
          {status.duration_seconds && (
            <span className="flex items-center gap-1">
              <Clock className="h-4 w-4" />
              {formatDuration(status.duration_seconds)}
            </span>
          )}
          <span>{formatDate(status.created_at)}</span>
        </div>
      </div>

      {isProcessing && (
        <Card className="p-4 space-y-3">
          <div className="flex items-center gap-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span className="font-medium text-sm">Processing video...</span>
          </div>
          <Progress value={status.progress ?? 0} className="h-2" />
          <p className="text-xs text-muted-foreground">
            {typeof status.progress === "number" ? `${Math.round(status.progress)}% complete` : "Queued"}
          </p>
        </Card>
      )}

      {isError && (
        <Card className="p-4 space-y-3 border-destructive/50">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-4 w-4 text-destructive" />
            <span className="font-medium text-sm text-destructive">Processing failed</span>
          </div>
          {status.error && <p className="text-sm text-muted-foreground">{status.error}</p>}
          {status.upload_available && (
            <Button variant="outline" size="sm" onClick={handleReprocess} disabled={isReprocessing}>
              {isReprocessing ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
              Retry
            </Button>
          )}
        </Card>
      )}

      {isReady && (
        <div className="relative rounded-lg overflow-hidden bg-black">
          <video ref={videoRef} controls className="w-full aspect-video" playsInline />

          {playerError && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/80">
              <div className="text-center space-y-2">
                <AlertCircle className="h-8 w-8 text-destructive mx-auto" />
                <p className="text-destructive text-sm">{playerError}</p>
              </div>
            </div>
          )}

          {showAutoplay && nextEpisode && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/90">
              <div className="text-center space-y-6 max-w-md mx-auto p-6">
                <p className="text-white/70 text-sm">Next episode in {autoplayCountdown} seconds</p>

                <div className="flex items-center gap-4">
                  {nextEpisode.thumbnail && (
                    <div className="w-32 h-20 rounded overflow-hidden shrink-0">
                      <img
                        src={nextEpisode.thumbnail}
                        alt={nextEpisode.title}
                        className="w-full h-full object-cover"
                      />
                    </div>
                  )}
                  <div className="text-left">
                    <Badge variant="secondary" className="mb-1">
                      Episode {nextEpisode.episode_number}
                    </Badge>
                    <p className="text-white font-medium">{nextEpisode.title}</p>
                  </div>
                </div>

                <div className="flex items-center justify-center gap-3">
                  <Button variant="outline" onClick={cancelAutoplay}>
                    <X className="h-4 w-4" />
                    Cancel
                  </Button>
                  <Button onClick={playNextNow}>
                    <Play className="h-4 w-4" fill="currentColor" />
                    Play Now
                  </Button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {showEpisodeList && seriesEpisodes.length > 0 && (
        <Card className="p-4 space-y-2">
          <div className="flex items-center justify-between mb-2">
            <h3 className="font-semibold">Episodes</h3>
            <Button variant="ghost" size="icon" onClick={() => setShowEpisodeList(false)}>
              <X className="h-4 w-4" />
            </Button>
          </div>
          <div className="space-y-1 max-h-64 overflow-y-auto">
            {seriesEpisodes.map((episode, idx) => (
              <Link
                key={episode.id}
                to={`/watch/${episode.id}`}
                className={`flex items-center gap-3 p-2 rounded hover:bg-muted transition-colors ${
                  episode.id === id ? "bg-accent" : ""
                }`}
              >
                <span className="text-muted-foreground w-6 text-right shrink-0">
                  {idx + 1}
                </span>
                <div className="w-16 h-10 rounded overflow-hidden bg-muted shrink-0 relative">
                  {episode.thumbnail && (
                    <img
                      src={episode.thumbnail}
                      alt=""
                      className="w-full h-full object-cover"
                    />
                  )}
                  {episode.watch_position && episode.duration_seconds && !episode.watch_completed && (
                    <div className="absolute inset-x-0 bottom-0">
                      <div
                        className="h-0.5 bg-primary"
                        style={{ width: `${Math.min(100, (episode.watch_position / episode.duration_seconds) * 100)}%` }}
                      />
                    </div>
                  )}
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate">{episode.title}</p>
                  {episode.duration_seconds && (
                    <p className="text-xs text-muted-foreground">
                      {formatDuration(episode.duration_seconds)}
                    </p>
                  )}
                </div>
                {episode.watch_completed && (
                  <Badge variant="secondary" className="shrink-0">Watched</Badge>
                )}
              </Link>
            ))}
          </div>
        </Card>
      )}
    </div>
  )
}
