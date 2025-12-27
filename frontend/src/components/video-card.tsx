import { Link } from "react-router-dom"
import { Play, AlertCircle, Loader2 } from "lucide-react"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { formatDuration } from "@/lib/format"
import type { VideoItem } from "@/types"

interface VideoCardProps {
  video: VideoItem
}

export function VideoCard({ video }: VideoCardProps) {
  const isProcessing = video.status === "processing"
  const isError = video.status === "error"
  const isReady = video.status === "ready"

  return (
    <Link to={`/watch/${video.id}`}>
      <Card className="overflow-hidden hover:bg-accent/50 transition-colors">
        <div className="relative aspect-video bg-muted">
          {video.thumbnail ? (
            <img
              src={video.thumbnail}
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

          {isReady && video.duration_seconds && (
            <div className="absolute bottom-2 right-2 bg-black/75 text-white text-xs px-1.5 py-0.5 rounded">
              {formatDuration(video.duration_seconds)}
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
        </div>

        <div className="p-3 space-y-2">
          <h3 className="font-medium text-sm leading-tight line-clamp-2">
            {video.title}
          </h3>

          {(isProcessing || isError) && (
            <div className="flex items-center gap-2">
              {isProcessing && <Badge variant="processing">Processing</Badge>}
              {isError && <Badge variant="error">Failed</Badge>}
            </div>
          )}

          {isError && video.error && (
            <p className="text-xs text-destructive line-clamp-2">{video.error}</p>
          )}
        </div>
      </Card>
    </Link>
  )
}
