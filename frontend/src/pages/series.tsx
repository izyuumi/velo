import { useState, useEffect, useCallback } from "react"
import { useParams, useNavigate, Link } from "react-router-dom"
import { ArrowLeft, Loader2, AlertCircle, Play, Settings, Trash2, Film, Download, CheckCircle } from "lucide-react"
import { useBulkDownload } from "@/hooks/use-bulk-download"
import { BulkDownloadBar } from "@/components/bulk-download-bar"
import { getDownload, offlineSupported, removeDownload } from "@/lib/offline"
import { Button } from "@/components/ui/button"
import { VideoCard } from "@/components/video-card"
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
import { fetchSeriesDetail, deleteSeries } from "@/lib/api"
import { appUrl } from "@/lib/url"
import type { SeriesDetail } from "@/types"

export function SeriesPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()

  const [series, setSeries] = useState<SeriesDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)
  const bulk = useBulkDownload()
  const [, bump] = useState(0) // re-render after remove-all
  const [removingDownloads, setRemovingDownloads] = useState(false)

  const fetchData = useCallback(async () => {
    if (!id) return
    try {
      const data = await fetchSeriesDetail(id)
      setSeries(data)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error")
    } finally {
      setLoading(false)
    }
  }, [id])

  useEffect(() => {
    fetchData()
  }, [fetchData])

  const handleDelete = async () => {
    if (!id) return
    setIsDeleting(true)
    try {
      await deleteSeries(id)
      navigate("/")
    } catch (err) {
      setError(err instanceof Error ? err.message : "Delete failed")
      setIsDeleting(false)
    }
  }

  const totalDuration = series?.episodes.reduce((sum, ep) => sum + (ep.duration_seconds || 0), 0) || 0
  const watchedCount = series?.episodes.filter((ep) => ep.watch_completed).length || 0
  const readyEpisodes = series?.episodes.filter((ep) => ep.status === "ready") || []

  const firstUnwatched = readyEpisodes.find((ep) => !ep.watch_completed && (!ep.watch_position || ep.watch_position === 0))
  const continueEpisode = readyEpisodes.find((ep) => !ep.watch_completed && ep.watch_position && ep.watch_position > 0)
  const playNext = continueEpisode || firstUnwatched || readyEpisodes[0]

  if (loading) {
    return (
      <div className="flex items-center justify-center min-h-[50vh]">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (error || !series) {
    return (
      <div className="flex flex-col items-center justify-center min-h-[50vh] gap-4">
        <AlertCircle className="h-10 w-10 text-destructive" />
        <p className="text-destructive">{error || "Series not found"}</p>
        <Button asChild variant="outline">
          <Link to="/">
            <ArrowLeft className="h-4 w-4" />
            Back to Home
          </Link>
        </Button>
      </div>
    )
  }

  const coverEpisode = series.cover_video_id
    ? series.episodes.find((ep) => ep.id === series.cover_video_id)
    : series.episodes[0]

  return (
    <div className="space-y-8">
      <Button variant="ghost" size="sm" asChild>
        <Link to="/">
          <ArrowLeft className="h-4 w-4" />
          Back
        </Link>
      </Button>

      <div className="relative rounded-lg overflow-hidden bg-muted">
        <div className="absolute inset-0">
          {coverEpisode?.thumbnail ? (
            <img
              src={appUrl(coverEpisode.thumbnail)}
              alt={series.title}
              className="w-full h-full object-cover opacity-30 blur-sm"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center">
              <Film className="h-24 w-24 text-muted-foreground/20" />
            </div>
          )}
        </div>

        <div className="relative p-8 flex flex-col md:flex-row gap-6">
          <div className="w-48 h-28 md:w-64 md:h-36 rounded-lg overflow-hidden bg-muted shrink-0 shadow-lg">
            {coverEpisode?.thumbnail ? (
              <img
                src={appUrl(coverEpisode.thumbnail)}
                alt={series.title}
                className="w-full h-full object-cover"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center">
                <Film className="h-12 w-12 text-muted-foreground" />
              </div>
            )}
          </div>

          <div className="flex-1 space-y-4">
            <div>
              <h1 className="text-2xl md:text-3xl font-bold">{series.title}</h1>
              {series.description && (
                <p className="text-muted-foreground mt-2">{series.description}</p>
              )}
            </div>

            <div className="flex items-center gap-4 text-sm text-muted-foreground">
              <span>{series.episodes.length} episodes</span>
              {totalDuration > 0 && (
                <span>• {formatDuration(totalDuration)} total</span>
              )}
              {watchedCount > 0 && (
                <span>• {watchedCount} watched</span>
              )}
              <span>• Created {formatDate(series.created_at)}</span>
            </div>

            <div className="flex items-center gap-2">
              {playNext && (
                <Button asChild>
                  <Link to={`/watch/${playNext.id}`}>
                    <Play className="h-4 w-4" fill="currentColor" />
                    {continueEpisode ? "Continue" : "Play"}
                  </Link>
                </Button>
              )}

              {offlineSupported() && readyEpisodes.length > 0 && (
                readyEpisodes.every((ep) => getDownload(ep.id)) ? (
                  <Button
                    variant="outline"
                    disabled={removingDownloads}
                    onClick={async () => {
                      setRemovingDownloads(true)
                      try {
                        for (const ep of readyEpisodes) await removeDownload(ep.id)
                        bump((n) => n + 1)
                      } catch {
                        setError("Could not remove all downloads. Try again.")
                      } finally {
                        setRemovingDownloads(false)
                      }
                    }}
                    title="Remove all downloads"
                  >
                    {removingDownloads ? <Loader2 className="h-4 w-4 animate-spin" /> : <CheckCircle className="h-4 w-4 text-green-600" />}
                    {removingDownloads ? "Removing…" : "Downloaded · Remove All"}
                  </Button>
                ) : (
                  <Button variant="outline" onClick={() => bulk.start(readyEpisodes)} disabled={!!bulk.progress}>
                    <Download className="h-4 w-4" />
                    Download All
                  </Button>
                )
              )}

              <Button variant="outline" asChild>
                <Link to={`/series/${id}/manage`}>
                  <Settings className="h-4 w-4" />
                  Manage
                </Link>
              </Button>

              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <Button variant="outline" className="text-destructive hover:text-destructive">
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Delete Series</AlertDialogTitle>
                    <AlertDialogDescription>
                      This will delete the series. Videos will become standalone but won't be deleted.
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
        </div>
      </div>

      <BulkDownloadBar {...bulk} />

      <section className="space-y-4">
        <h2 className="font-semibold text-lg">Episodes</h2>
        {series.episodes.length === 0 ? (
          <div className="text-center py-12 text-muted-foreground">
            <p>No episodes yet</p>
            <Button asChild className="mt-4" variant="outline">
              <Link to={`/series/${id}/manage`}>
                Add Episodes
              </Link>
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {series.episodes.map((episode) => (
              <VideoCard key={episode.id} video={episode} />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}
