import { Link } from "react-router-dom"
import { Play, Film } from "lucide-react"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { formatDuration } from "@/lib/format"
import type { SeriesItem } from "@/types"

interface SeriesCardProps {
  series: SeriesItem
}

export function SeriesCard({ series }: SeriesCardProps) {
  const progressPercent = series.episode_count > 0
    ? Math.round((series.watched_count / series.episode_count) * 100)
    : 0

  const hasProgress = series.in_progress_count > 0 || series.watched_count > 0

  return (
    <Link to={`/series/${series.id}`}>
      <Card className="overflow-hidden hover:bg-accent/50 transition-all hover:scale-[1.02] group">
        <div className="relative aspect-video bg-gradient-to-br from-primary/20 to-primary/5">
          {series.cover_thumbnail ? (
            <img
              src={series.cover_thumbnail}
              alt={series.title}
              className="absolute inset-0 w-full h-full object-cover"
              loading="lazy"
            />
          ) : (
            <div className="absolute inset-0 flex items-center justify-center">
              <Film className="h-12 w-12 text-muted-foreground/50" />
            </div>
          )}

          <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/20 to-transparent" />

          <div className="absolute top-2 left-2">
            <Badge variant="secondary" className="bg-primary/90 text-primary-foreground">
              {series.episode_count} {series.episode_count === 1 ? "Episode" : "Episodes"}
            </Badge>
          </div>

          {series.total_duration && series.total_duration > 0 && (
            <div className="absolute top-2 right-2 bg-black/75 text-white text-xs px-1.5 py-0.5 rounded">
              {formatDuration(series.total_duration)}
            </div>
          )}

          <div className="absolute bottom-0 inset-x-0 p-3">
            <h3 className="font-semibold text-white text-lg leading-tight line-clamp-2 drop-shadow-lg">
              {series.title}
            </h3>
            {series.description && (
              <p className="text-white/70 text-sm mt-1 line-clamp-1">
                {series.description}
              </p>
            )}
          </div>

          <div className="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity">
            <div className="w-14 h-14 rounded-full bg-primary/90 flex items-center justify-center">
              <Play className="h-6 w-6 text-primary-foreground ml-1" fill="currentColor" />
            </div>
          </div>

          {hasProgress && (
            <div className="absolute inset-x-0 bottom-0">
              <div
                className="h-1 bg-primary"
                style={{ width: `${progressPercent}%` }}
              />
            </div>
          )}
        </div>
      </Card>
    </Link>
  )
}

