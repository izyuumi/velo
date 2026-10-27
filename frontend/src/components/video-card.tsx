import { Link } from "react-router-dom"
import { Play, AlertCircle, Loader2, CheckCircle, Download } from "lucide-react"
import { getDownload } from "@/lib/offline"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { formatDuration } from "@/lib/format"
import { appUrl } from "@/lib/url"
import type { VideoItem } from "@/types"

interface VideoCardProps {
  video: VideoItem
  showCheckbox?: boolean
  isSelected?: boolean
  onSelect?: (id: string) => void
  compact?: boolean
}

export function VideoCard({ video, showCheckbox, isSelected, onSelect, compact }: VideoCardProps) {
  const isProcessing = video.status === "processing"
  const isError = video.status === "error"
  const isReady = video.status === "ready"

  const watchProgress = video.watch_position && video.duration_seconds
    ? Math.min(100, (video.watch_position / video.duration_seconds) * 100)
    : 0

  const content = (
    <Card className={`overflow-hidden transition-colors hover:bg-accent/50 ${compact ? "" : ""}`}>
      <div className="relative aspect-video bg-muted">
        {video.thumbnail ? (
          <img
            src={appUrl(video.thumbnail)}
            alt={video.title}
            className="absolute inset-0 w-full h-full object-cover"
            loading="lazy"
          />
        ) : (
          <div className="absolute inset-0 flex items-center justify-center">
            {isProcessing && (
              <Loader2 className="h-8 w-8 text-muted-foreground animate-spin" />
            )}
            {isError && (
              <AlertCircle className="h-8 w-8 text-destructive" />
            )}
            {isReady && (
              <Play className="h-8 w-8 text-muted-foreground" />
            )}
          </div>
        )}

        {showCheckbox && (
          <div className="absolute top-2 left-2 z-10">
            <div className={`w-5 h-5 rounded border-2 flex items-center justify-center transition-colors ${
              isSelected
                ? "bg-primary border-primary"
                : "bg-background/80 border-muted-foreground/50 hover:border-primary"
            }`}>
              {isSelected && <CheckCircle className="h-3 w-3 text-primary-foreground" />}
            </div>
          </div>
        )}

        <div className="absolute top-2 right-2 flex items-center gap-1">
          {getDownload(video.id) && (
            <div className="bg-black/75 text-white p-1 rounded" title="Available offline">
              <Download className="h-3 w-3" />
            </div>
          )}
          {video.episode_number && (
            <div className="bg-black/75 text-white text-xs px-1.5 py-0.5 rounded">
              Ep {video.episode_number}
            </div>
          )}
        </div>

        {isReady && video.duration_seconds && (
          <div className="absolute bottom-2 right-2 bg-black/75 text-white text-xs px-1.5 py-0.5 rounded">
            {formatDuration(video.duration_seconds)}
          </div>
        )}

        {video.watch_completed && (
          <div className="absolute bottom-2 left-2 bg-green-600/90 text-white text-xs px-1.5 py-0.5 rounded flex items-center gap-1">
            <CheckCircle className="h-3 w-3" />
            Watched
          </div>
        )}

        {isProcessing && (
          <div className="absolute inset-x-0 bottom-0 bg-black/75 p-2">
            <Progress value={video.progress || 0} className="h-1" />
            <p className="text-xs text-white mt-1">
              {video.progress !== undefined
                ? `${Math.round(video.progress)}%`
                : "Queued"}
            </p>
          </div>
        )}

        {isReady && watchProgress > 0 && !video.watch_completed && (
          <div className="absolute inset-x-0 bottom-0">
            <div
              className="h-1 bg-primary"
              style={{ width: `${watchProgress}%` }}
            />
          </div>
        )}
      </div>

      <div className={`p-3 space-y-2 ${compact ? "p-2" : ""}`}>
        <h3 className={`font-medium leading-tight line-clamp-2 ${compact ? "text-xs" : "text-sm"}`}>
          {video.title}
        </h3>

        {(isProcessing || isError) && !compact && (
          <div className="flex items-center gap-2">
            {isProcessing && <Badge variant="processing">Processing</Badge>}
            {isError && <Badge variant="error">Failed</Badge>}
          </div>
        )}

        {isError && video.error && !compact && (
          <p className="text-xs text-destructive line-clamp-2">{video.error}</p>
        )}
      </div>
    </Card>
  )

  if (showCheckbox) {
    return (
      <div
        className={`relative cursor-pointer ${isSelected ? "ring-2 ring-primary rounded-lg" : ""}`}
        onClick={() => onSelect?.(video.id)}
      >
        {content}
      </div>
    )
  }

  return <Link to={`/watch/${video.id}`}>{content}</Link>
}
