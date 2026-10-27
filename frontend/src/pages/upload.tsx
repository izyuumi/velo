import { useState, useCallback, useRef } from "react"
import { Link } from "react-router-dom"
import { Upload, X, FileVideo, Loader2, CheckCircle2, AlertCircle } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Card } from "@/components/ui/card"
import { Progress } from "@/components/ui/progress"
import { Badge } from "@/components/ui/badge"
import { uploadVideo } from "@/lib/api"
import type { StatusResponse } from "@/types"
import { appUrl } from "@/lib/url"

interface QueueItem {
  id: string
  file: File
  title: string
  status: "queued" | "uploading" | "processing" | "ready" | "error"
  uploadProgress: number
  processingProgress: number
  error?: string
  watchUrl?: string
}

export function UploadPage() {
  const [queue, setQueue] = useState<QueueItem[]>([])
  const [isDragging, setIsDragging] = useState(false)
  const [isUploading, setIsUploading] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const activePolls = useRef(0)

  const addFiles = useCallback((files: FileList) => {
    const newItems: QueueItem[] = Array.from(files).map((file) => ({
      id: crypto.randomUUID(),
      file,
      title: file.name.replace(/\.[^/.]+$/, ""),
      status: "queued",
      uploadProgress: 0,
      processingProgress: 0,
    }))
    setQueue((prev) => [...prev, ...newItems])
  }, [])

  const removeItem = useCallback((id: string) => {
    setQueue((prev) => prev.filter((item) => item.id !== id))
  }, [])

  const updateTitle = useCallback((id: string, title: string) => {
    setQueue((prev) =>
      prev.map((item) => (item.id === id ? { ...item, title } : item))
    )
  }, [])

  const pollStatus = useCallback(async (itemId: string, videoId: string) => {
    activePolls.current += 1
    try {
      const maxAttempts = 200
      for (let attempt = 0; attempt < maxAttempts; attempt++) {
        try {
          const resp = await fetch(appUrl(`/api/status/${videoId}`))
          if (!resp.ok) break

          const status: StatusResponse = await resp.json()

          if (status.status === "ready") {
            setQueue((prev) =>
              prev.map((item) =>
                item.id === itemId
                  ? { ...item, status: "ready", processingProgress: 100, watchUrl: status.watch_url }
                  : item
              )
            )
            return
          }

          if (status.status === "error") {
            setQueue((prev) =>
              prev.map((item) =>
                item.id === itemId ? { ...item, status: "error", error: status.error } : item
              )
            )
            return
          }

          if (typeof status.progress === "number") {
            setQueue((prev) =>
              prev.map((item) =>
                item.id === itemId
                  ? { ...item, processingProgress: Math.min(99, status.progress ?? 0) }
                  : item
              )
            )
          }
        } catch {
          break
        }

        await new Promise((r) => setTimeout(r, 1500))
      }
    } finally {
      activePolls.current -= 1
      if (activePolls.current === 0) {
        setIsUploading(false)
      }
    }
  }, [])

  const startUpload = useCallback(async () => {
    const pending = queue.filter((item) => item.status === "queued")
    if (pending.length === 0 || isUploading) return

    setIsUploading(true)
    let successCount = 0

    for (const item of pending) {
      setQueue((prev) =>
        prev.map((q) => (q.id === item.id ? { ...q, status: "uploading" } : q))
      )

      try {
        const result = await uploadVideo(item.file, item.title, (pct) => {
          setQueue((prev) =>
            prev.map((q) => (q.id === item.id ? { ...q, uploadProgress: pct } : q))
          )
        })

        setQueue((prev) =>
          prev.map((q) =>
            q.id === item.id
              ? { ...q, status: "processing", uploadProgress: 100, watchUrl: result.watch_url }
              : q
          )
        )

        successCount += 1
        pollStatus(item.id, result.id)
      } catch (err) {
        setQueue((prev) =>
          prev.map((q) =>
            q.id === item.id
              ? { ...q, status: "error", error: err instanceof Error ? err.message : "Upload failed" }
              : q
          )
        )
      }
    }

    if (successCount === 0) {
      setIsUploading(false)
    }
  }, [queue, isUploading, pollStatus])

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault()
      setIsDragging(false)
      if (e.dataTransfer.files?.length) {
        addFiles(e.dataTransfer.files)
      }
    },
    [addFiles]
  )

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    setIsDragging(false)
  }, [])

  const handleFileSelect = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (e.target.files?.length) {
        addFiles(e.target.files)
        e.target.value = ""
      }
    },
    [addFiles]
  )

  const hasQueued = queue.some((item) => item.status === "queued")

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Upload Videos</h1>
        <p className="text-sm text-muted-foreground mt-1">
          Drop videos here or click to select files
        </p>
      </div>

      <div
        className={`border-2 border-dashed rounded-lg p-8 text-center cursor-pointer transition-colors ${
          isDragging ? "border-primary bg-accent" : "border-muted-foreground/25 hover:border-muted-foreground/50"
        }`}
        onDrop={handleDrop}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onClick={() => fileInputRef.current?.click()}
      >
        <input
          ref={fileInputRef}
          type="file"
          accept="video/*"
          multiple
          className="hidden"
          onChange={handleFileSelect}
        />
        <Upload className="h-8 w-8 mx-auto text-muted-foreground" />
        <p className="mt-2 font-medium">Drag & drop videos here</p>
        <p className="text-sm text-muted-foreground">or click to browse</p>
      </div>

      {queue.length > 0 && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="font-semibold">Upload Queue</h2>
            {hasQueued && (
              <Button onClick={startUpload} disabled={isUploading} size="sm">
                {isUploading ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin" />
                    Uploading...
                  </>
                ) : (
                  <>
                    <Upload className="h-4 w-4" />
                    Start Upload
                  </>
                )}
              </Button>
            )}
          </div>

          <div className="space-y-2">
            {queue.map((item) => (
              <Card key={item.id} className="p-4">
                <div className="flex items-start gap-3">
                  <FileVideo className="h-5 w-5 text-muted-foreground mt-0.5 shrink-0" />

                  <div className="flex-1 min-w-0 space-y-2">
                    <div className="flex items-center gap-2">
                      {item.status === "queued" ? (
                        <Input
                          value={item.title}
                          onChange={(e) => updateTitle(item.id, e.target.value)}
                          placeholder="Video title"
                          className="h-8"
                        />
                      ) : (
                        <span className="font-medium text-sm truncate">{item.title}</span>
                      )}

                      {item.status === "queued" && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="shrink-0 h-8 w-8"
                          onClick={() => removeItem(item.id)}
                        >
                          <X className="h-4 w-4" />
                        </Button>
                      )}
                    </div>

                    <div className="flex items-center gap-2">
                      {item.status === "queued" && <Badge variant="secondary">Queued</Badge>}
                      {item.status === "uploading" && (
                        <>
                          <Badge variant="processing">
                            <Loader2 className="h-3 w-3 mr-1 animate-spin" />
                            Uploading
                          </Badge>
                          <span className="text-xs text-muted-foreground">{item.uploadProgress}%</span>
                        </>
                      )}
                      {item.status === "processing" && (
                        <>
                          <Badge variant="processing">
                            <Loader2 className="h-3 w-3 mr-1 animate-spin" />
                            Processing
                          </Badge>
                          <span className="text-xs text-muted-foreground">
                            {Math.round(item.processingProgress)}%
                          </span>
                        </>
                      )}
                      {item.status === "ready" && (
                        <>
                          <Badge variant="success">
                            <CheckCircle2 className="h-3 w-3 mr-1" />
                            Ready
                          </Badge>
                          {item.watchUrl && (
                            <Link to={item.watchUrl} className="text-xs text-primary hover:underline">
                              Watch →
                            </Link>
                          )}
                        </>
                      )}
                      {item.status === "error" && (
                        <Badge variant="error">
                          <AlertCircle className="h-3 w-3 mr-1" />
                          Failed
                        </Badge>
                      )}
                    </div>

                    {(item.status === "uploading" || item.status === "processing") && (
                      <Progress
                        value={item.status === "uploading" ? item.uploadProgress : item.processingProgress}
                        className="h-1"
                      />
                    )}

                    {item.status === "error" && item.error && (
                      <p className="text-xs text-destructive">{item.error}</p>
                    )}
                  </div>
                </div>
              </Card>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
