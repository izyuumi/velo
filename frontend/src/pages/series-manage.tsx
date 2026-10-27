import { useState, useEffect, useCallback } from "react"
import { useParams, useNavigate, Link } from "react-router-dom"
import { ArrowLeft, Loader2, AlertCircle, GripVertical, Trash2, Plus, Save, Film } from "lucide-react"
import { DndContext, closestCenter, KeyboardSensor, PointerSensor, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core"
import { SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy, arrayMove } from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
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
} from "@/components/ui/alert-dialog"
import { formatDuration } from "@/lib/format"
import { fetchSeriesDetail, fetchVideos, updateSeries, reorderSeriesEpisodes, addVideosToSeries, removeVideoFromSeries } from "@/lib/api"
import { appUrl } from "@/lib/url"
import type { SeriesDetail, VideoItem } from "@/types"

interface SortableEpisodeProps {
  episode: VideoItem
  onRemove: (id: string) => void
  onSetCover: (id: string) => void
  isCover: boolean
}

function SortableEpisode({ episode, onRemove, onSetCover, isCover }: SortableEpisodeProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: episode.id,
  })

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  }

  return (
    <Card
      ref={setNodeRef}
      style={style}
      className={`flex items-center gap-4 p-3 ${isDragging ? "ring-2 ring-primary" : ""}`}
    >
      <button
        {...attributes}
        {...listeners}
        className="cursor-grab active:cursor-grabbing p-1 hover:bg-muted rounded"
      >
        <GripVertical className="h-5 w-5 text-muted-foreground" />
      </button>

      <div className="w-20 h-12 rounded overflow-hidden bg-muted shrink-0">
        {episode.thumbnail ? (
          <img src={appUrl(episode.thumbnail)} alt="" className="w-full h-full object-cover" />
        ) : (
          <div className="w-full h-full flex items-center justify-center">
            <Film className="h-4 w-4 text-muted-foreground" />
          </div>
        )}
      </div>

      <div className="flex-1 min-w-0">
        <p className="font-medium text-sm truncate">{episode.title}</p>
        <p className="text-xs text-muted-foreground">
          Episode {episode.episode_number}
          {episode.duration_seconds && ` • ${formatDuration(episode.duration_seconds)}`}
        </p>
      </div>

      <div className="flex items-center gap-1 shrink-0">
        <Button
          variant={isCover ? "default" : "outline"}
          size="sm"
          onClick={() => onSetCover(episode.id)}
        >
          {isCover ? "Cover" : "Set Cover"}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => onRemove(episode.id)}
          className="text-destructive hover:text-destructive"
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </Card>
  )
}

export function SeriesManagePage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()

  const [series, setSeries] = useState<SeriesDetail | null>(null)
  const [availableVideos, setAvailableVideos] = useState<VideoItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [isSaving, setIsSaving] = useState(false)

  const [title, setTitle] = useState("")
  const [description, setDescription] = useState("")
  const [coverId, setCoverId] = useState<string | undefined>()
  const [episodes, setEpisodes] = useState<VideoItem[]>([])
  const [hasChanges, setHasChanges] = useState(false)

  const [showAddDialog, setShowAddDialog] = useState(false)
  const [selectedToAdd, setSelectedToAdd] = useState<Set<string>>(new Set())

  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
  )

  const fetchData = useCallback(async () => {
    if (!id) return
    try {
      const [seriesData, videosData] = await Promise.all([
        fetchSeriesDetail(id),
        fetchVideos(),
      ])
      setSeries(seriesData)
      setTitle(seriesData.title)
      setDescription(seriesData.description || "")
      setCoverId(seriesData.cover_video_id)
      setEpisodes(seriesData.episodes)

      const seriesVideoIds = new Set(seriesData.episodes.map((e) => e.id))
      setAvailableVideos(videosData.filter((v) => !v.series_id && v.status === "ready" && !seriesVideoIds.has(v.id)))
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

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event
    if (over && active.id !== over.id) {
      setEpisodes((items) => {
        const oldIndex = items.findIndex((i) => i.id === active.id)
        const newIndex = items.findIndex((i) => i.id === over.id)
        return arrayMove(items, oldIndex, newIndex)
      })
      setHasChanges(true)
    }
  }

  const handleRemove = async (videoId: string) => {
    if (!id) return
    try {
      await removeVideoFromSeries(id, videoId)
      setEpisodes((prev) => prev.filter((e) => e.id !== videoId))
      fetchData()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Remove failed")
    }
  }

  const handleSetCover = (videoId: string) => {
    setCoverId(videoId)
    setHasChanges(true)
  }

  const handleAddVideos = async () => {
    if (!id || selectedToAdd.size === 0) return
    try {
      await addVideosToSeries(id, Array.from(selectedToAdd))
      setSelectedToAdd(new Set())
      setShowAddDialog(false)
      fetchData()
    } catch (err) {
      setError(err instanceof Error ? err.message : "Add failed")
    }
  }

  const handleSave = async () => {
    if (!id) return
    setIsSaving(true)
    try {
      const videoIds = episodes.map((e) => e.id)
      await Promise.all([
        updateSeries(id, {
          title: title.trim() || undefined,
          description: description.trim() || undefined,
          cover_video_id: coverId,
        }),
        reorderSeriesEpisodes(id, videoIds),
      ])
      setHasChanges(false)
      navigate(`/series/${id}`)
    } catch (err) {
      setError(err instanceof Error ? err.message : "Save failed")
    } finally {
      setIsSaving(false)
    }
  }

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

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" asChild>
          <Link to={`/series/${id}`}>
            <ArrowLeft className="h-4 w-4" />
            Back to Series
          </Link>
        </Button>

        <Button onClick={handleSave} disabled={isSaving || !hasChanges}>
          {isSaving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
          Save Changes
        </Button>
      </div>

      <div className="space-y-4">
        <h1 className="text-2xl font-bold">Manage Series</h1>

        <div className="space-y-4">
          <div>
            <label className="text-sm font-medium">Title</label>
            <Input
              value={title}
              onChange={(e) => {
                setTitle(e.target.value)
                setHasChanges(true)
              }}
              placeholder="Series title"
              className="mt-1"
            />
          </div>

          <div>
            <label className="text-sm font-medium">Description</label>
            <Input
              value={description}
              onChange={(e) => {
                setDescription(e.target.value)
                setHasChanges(true)
              }}
              placeholder="Optional description"
              className="mt-1"
            />
          </div>
        </div>
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">Episodes ({episodes.length})</h2>
          <Button variant="outline" size="sm" onClick={() => setShowAddDialog(true)}>
            <Plus className="h-4 w-4" />
            Add Videos
          </Button>
        </div>

        {episodes.length === 0 ? (
          <Card className="p-8 text-center text-muted-foreground">
            No episodes yet. Add videos to this series.
          </Card>
        ) : (
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
            <SortableContext items={episodes.map((e) => e.id)} strategy={verticalListSortingStrategy}>
              <div className="space-y-2">
                {episodes.map((episode) => (
                  <SortableEpisode
                    key={episode.id}
                    episode={episode}
                    onRemove={handleRemove}
                    onSetCover={handleSetCover}
                    isCover={coverId === episode.id}
                  />
                ))}
              </div>
            </SortableContext>
          </DndContext>
        )}
      </div>

      <AlertDialog open={showAddDialog} onOpenChange={setShowAddDialog}>
        <AlertDialogContent className="max-w-2xl">
          <AlertDialogHeader>
            <AlertDialogTitle>Add Videos to Series</AlertDialogTitle>
            <AlertDialogDescription>
              Select videos to add as episodes.
            </AlertDialogDescription>
          </AlertDialogHeader>

          <div className="max-h-96 overflow-y-auto space-y-2 my-4">
            {availableVideos.length === 0 ? (
              <p className="text-muted-foreground text-center py-8">
                No available videos. All videos are already in a series.
              </p>
            ) : (
              availableVideos.map((video) => (
                <Card
                  key={video.id}
                  className={`flex items-center gap-4 p-3 cursor-pointer transition-colors ${
                    selectedToAdd.has(video.id) ? "ring-2 ring-primary bg-accent" : "hover:bg-muted"
                  }`}
                  onClick={() => {
                    setSelectedToAdd((prev) => {
                      const next = new Set(prev)
                      if (next.has(video.id)) {
                        next.delete(video.id)
                      } else {
                        next.add(video.id)
                      }
                      return next
                    })
                  }}
                >
                  <div className="w-20 h-12 rounded overflow-hidden bg-muted shrink-0">
                    {video.thumbnail ? (
                      <img src={appUrl(video.thumbnail)} alt="" className="w-full h-full object-cover" />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center">
                        <Film className="h-4 w-4 text-muted-foreground" />
                      </div>
                    )}
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="font-medium text-sm truncate">{video.title}</p>
                    {video.duration_seconds && (
                      <p className="text-xs text-muted-foreground">
                        {formatDuration(video.duration_seconds)}
                      </p>
                    )}
                  </div>
                </Card>
              ))
            )}
          </div>

          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleAddVideos}
              disabled={selectedToAdd.size === 0}
            >
              Add {selectedToAdd.size > 0 && `(${selectedToAdd.size})`}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
