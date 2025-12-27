import { useState, useEffect, useCallback, useRef } from "react"
import { useParams, useNavigate, Link } from "react-router-dom"
import Hls from "hls.js"
import { ArrowLeft, Loader2, AlertCircle, Edit3, Trash2, RefreshCw, Clock, Monitor } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Progress } from "@/components/ui/progress"
import { Card } from "@/components/ui/card"
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
import { renameVideo, deleteVideo, reprocessVideo } from "@/lib/api"
import type { StatusResponse } from "@/types"

export function WatchPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const videoRef = useRef<HTMLVideoElement>(null)
  const hlsRef = useRef<Hls | null>(null)

  const [status, setStatus] = useState<StatusResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [playerError, setPlayerError] = useState<string | null>(null)

  const [isEditing, setIsEditing] = useState(false)
  const [editTitle, setEditTitle] = useState("")
  const [isSaving, setIsSaving] = useState(false)
  const [isDeleting, setIsDeleting] = useState(false)
  const [isReprocessing, setIsReprocessing] = useState(false)

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
    } else {
      setPlayerError("Your browser doesn't support video playback.")
    }

    return () => {
      if (hlsRef.current) {
        hlsRef.current.destroy()
        hlsRef.current = null
      }
    }
  }, [status, id])

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

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      <Button variant="ghost" size="sm" asChild>
        <Link to="/">
          <ArrowLeft className="h-4 w-4" />
          Back
        </Link>
      </Button>

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
            <h1 className="text-xl font-bold">{status.title}</h1>
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
        </div>
      )}

    </div>
  )
}
