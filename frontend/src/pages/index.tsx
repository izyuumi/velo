import { useState, useEffect, useCallback } from "react"
import { Link, useNavigate } from "react-router-dom"
import { Upload, Loader2, Video, AlertCircle, Plus, Trash2, FolderPlus, X, CheckSquare, Search, Download } from "lucide-react"
import { useBulkDownload } from "@/hooks/use-bulk-download"
import { BulkDownloadBar } from "@/components/bulk-download-bar"
import { offlineSupported } from "@/lib/offline"
import { VideoCard } from "@/components/video-card"
import { SeriesCard } from "@/components/series-card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { fetchVideos, fetchSeries, bulkDeleteVideos, createSeries, bulkAddToSeries } from "@/lib/api"
import type { VideoItem, SeriesItem } from "@/types"

export function IndexPage() {
  const navigate = useNavigate()
  const [videos, setVideos] = useState<VideoItem[]>([])
  const [series, setSeries] = useState<SeriesItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [search, setSearch] = useState("")
  const [debouncedSearch, setDebouncedSearch] = useState("")

  const [selectionMode, setSelectionMode] = useState(false)
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())

  const [showDeleteDialog, setShowDeleteDialog] = useState(false)
  const [showCreateSeriesDialog, setShowCreateSeriesDialog] = useState(false)
  const [showAddToSeriesDialog, setShowAddToSeriesDialog] = useState(false)
  const [newSeriesTitle, setNewSeriesTitle] = useState("")
  const [isDeleting, setIsDeleting] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const bulk = useBulkDownload()

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300)
    return () => clearTimeout(timer)
  }, [search])

  const fetchData = useCallback(async () => {
    try {
      const [videosData, seriesData] = await Promise.all([
        fetchVideos(debouncedSearch || undefined),
        fetchSeries(),
      ])
      setVideos(videosData)
      setSeries(seriesData)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error")
    } finally {
      setLoading(false)
    }
  }, [debouncedSearch])

  useEffect(() => {
    fetchData()
    const interval = setInterval(fetchData, 5000)
    return () => clearInterval(interval)
  }, [fetchData])

  const standaloneVideos = videos.filter((v) => !v.series_id)
  const processingVideos = standaloneVideos.filter((v) => v.status === "processing")
  const readyVideos = standaloneVideos.filter((v) => v.status === "ready")
  const errorVideos = standaloneVideos.filter((v) => v.status === "error")

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const selectAll = () => {
    const allIds = [...readyVideos, ...processingVideos, ...errorVideos].map((v) => v.id)
    setSelectedIds(new Set(allIds))
  }

  const clearSelection = () => {
    setSelectedIds(new Set())
    setSelectionMode(false)
  }

  const handleBulkDelete = async () => {
    setIsDeleting(true)
    try {
      await bulkDeleteVideos(Array.from(selectedIds))
      clearSelection()
      fetchData()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Delete failed")
    } finally {
      setIsDeleting(false)
      setShowDeleteDialog(false)
    }
  }

  const handleCreateSeriesWithSelected = async () => {
    if (!newSeriesTitle.trim()) return
    setIsCreating(true)
    try {
      const detail = await createSeries(newSeriesTitle.trim(), undefined, Array.from(selectedIds))
      clearSelection()
      setNewSeriesTitle("")
      setShowCreateSeriesDialog(false)
      navigate(`/series/${detail.id}`)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Create series failed")
    } finally {
      setIsCreating(false)
    }
  }

  const handleAddToExistingSeries = async (seriesId: string) => {
    try {
      await bulkAddToSeries(Array.from(selectedIds), seriesId)
      clearSelection()
      setShowAddToSeriesDialog(false)
      fetchData()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Add to series failed")
    }
  }

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
        <Button onClick={fetchData} variant="outline">
          Try Again
        </Button>
      </div>
    )
  }

  const isEmpty = videos.length === 0 && series.length === 0

  if (isEmpty && !debouncedSearch) {
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
      <div className="flex items-center gap-4">
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
          <Input
            placeholder="Search videos..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-9"
          />
        </div>

        <div className="flex items-center gap-2">
          {!selectionMode ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setSelectionMode(true)}
            >
              <CheckSquare className="h-4 w-4" />
              Select
            </Button>
          ) : (
            <>
              <span className="text-sm text-muted-foreground">
                {selectedIds.size} selected
              </span>
              <Button variant="outline" size="sm" onClick={selectAll}>
                All
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowCreateSeriesDialog(true)}
                disabled={selectedIds.size === 0}
              >
                <Plus className="h-4 w-4" />
                New Series
              </Button>
              {series.length > 0 && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setShowAddToSeriesDialog(true)}
                  disabled={selectedIds.size === 0}
                >
                  <FolderPlus className="h-4 w-4" />
                  Add to Series
                </Button>
              )}
              {offlineSupported() && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    bulk.start(videos.filter((v) => selectedIds.has(v.id)))
                    clearSelection()
                  }}
                  disabled={selectedIds.size === 0 || !!bulk.progress}
                >
                  <Download className="h-4 w-4" />
                  Download
                </Button>
              )}
              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowDeleteDialog(true)}
                disabled={selectedIds.size === 0}
                className="text-destructive hover:text-destructive"
              >
                <Trash2 className="h-4 w-4" />
                Delete
              </Button>
              <Button variant="ghost" size="sm" onClick={clearSelection}>
                <X className="h-4 w-4" />
                Cancel
              </Button>
            </>
          )}
        </div>
      </div>

      <BulkDownloadBar {...bulk} />

      {series.length > 0 && !debouncedSearch && (
        <section className="space-y-4">
          <h2 className="font-semibold">Series</h2>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {series.map((s) => (
              <SeriesCard key={s.id} series={s} />
            ))}
          </div>
        </section>
      )}

      {processingVideos.length > 0 && (
        <section className="space-y-4">
          <div className="flex items-center gap-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            <h2 className="font-semibold">Processing ({processingVideos.length})</h2>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {processingVideos.map((video) => (
              <VideoCard
                key={video.id}
                video={video}
                showCheckbox={selectionMode}
                isSelected={selectedIds.has(video.id)}
                onSelect={toggleSelect}
              />
            ))}
          </div>
        </section>
      )}

      {readyVideos.length > 0 && (
        <section className="space-y-4">
          {(series.length > 0 || processingVideos.length > 0) && (
            <h2 className="font-semibold">Videos</h2>
          )}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {readyVideos.map((video) => (
              <VideoCard
                key={video.id}
                video={video}
                showCheckbox={selectionMode}
                isSelected={selectedIds.has(video.id)}
                onSelect={toggleSelect}
              />
            ))}
          </div>
        </section>
      )}

      {errorVideos.length > 0 && (
        <section className="space-y-4">
          <div className="flex items-center gap-2">
            <AlertCircle className="h-4 w-4 text-destructive" />
            <h2 className="font-semibold">Failed ({errorVideos.length})</h2>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {errorVideos.map((video) => (
              <VideoCard
                key={video.id}
                video={video}
                showCheckbox={selectionMode}
                isSelected={selectedIds.has(video.id)}
                onSelect={toggleSelect}
              />
            ))}
          </div>
        </section>
      )}

      {debouncedSearch && videos.length === 0 && (
        <div className="text-center py-12 text-muted-foreground">
          No videos found for "{debouncedSearch}"
        </div>
      )}

      <AlertDialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete {selectedIds.size} Videos</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently delete the selected videos. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleBulkDelete} disabled={isDeleting}>
              {isDeleting ? <Loader2 className="h-4 w-4 animate-spin" /> : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={showCreateSeriesDialog} onOpenChange={setShowCreateSeriesDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Create New Series</AlertDialogTitle>
            <AlertDialogDescription>
              Create a series with the {selectedIds.size} selected videos.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <Input
            placeholder="Series title"
            value={newSeriesTitle}
            onChange={(e) => setNewSeriesTitle(e.target.value)}
            className="my-4"
          />
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleCreateSeriesWithSelected}
              disabled={isCreating || !newSeriesTitle.trim()}
            >
              {isCreating ? <Loader2 className="h-4 w-4 animate-spin" /> : "Create"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={showAddToSeriesDialog} onOpenChange={setShowAddToSeriesDialog}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Add to Series</AlertDialogTitle>
            <AlertDialogDescription>
              Select a series to add the {selectedIds.size} selected videos.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="space-y-2 my-4 max-h-64 overflow-y-auto">
            {series.map((s) => (
              <Button
                key={s.id}
                variant="outline"
                className="w-full justify-start"
                onClick={() => handleAddToExistingSeries(s.id)}
              >
                {s.title} ({s.episode_count} episodes)
              </Button>
            ))}
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
