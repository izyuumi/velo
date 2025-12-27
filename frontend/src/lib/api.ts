import type { StatusResponse, UploadResult, VideoItem, SeriesItem, SeriesDetail, WatchProgress } from "@/types"

export async function fetchVideos(search?: string): Promise<VideoItem[]> {
  const url = search ? `/api/videos?search=${encodeURIComponent(search)}` : "/api/videos"
  const resp = await fetch(url)
  if (!resp.ok) throw new Error("Failed to fetch videos")
  return resp.json()
}

export async function fetchContinueWatching(): Promise<VideoItem[]> {
  const resp = await fetch("/api/videos/continue")
  if (!resp.ok) throw new Error("Failed to fetch continue watching")
  return resp.json()
}

export async function fetchVideoStatus(id: string): Promise<StatusResponse> {
  const resp = await fetch(`/api/status/${id}`)
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to fetch status")
  }
  return resp.json()
}

export async function uploadVideo(
  file: File,
  title: string,
  onProgress?: (percent: number) => void
): Promise<UploadResult> {
  return new Promise((resolve, reject) => {
    const form = new FormData()
    form.append("video", file)
    form.append("title", title)

    const xhr = new XMLHttpRequest()
    xhr.open("POST", "/upload")

    xhr.upload.addEventListener("progress", (evt) => {
      if (evt.lengthComputable) {
        const pct = Math.round((evt.loaded / evt.total) * 100)
        onProgress?.(pct)
      }
    })

    xhr.onload = () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        try {
          resolve(JSON.parse(xhr.responseText))
        } catch {
          reject(new Error("Invalid response"))
        }
      } else {
        try {
          const data = JSON.parse(xhr.responseText)
          reject(new Error(data.message || `Upload failed (${xhr.status})`))
        } catch {
          reject(new Error(`Upload failed (${xhr.status})`))
        }
      }
    }

    xhr.onerror = () => reject(new Error("Upload failed"))
    xhr.send(form)
  })
}

export async function renameVideo(
  id: string,
  title: string
): Promise<StatusResponse> {
  const resp = await fetch(`/api/video/${id}/rename`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Rename failed")
  }
  return resp.json()
}

export async function deleteVideo(id: string): Promise<void> {
  const resp = await fetch(`/api/video/${id}`, { method: "DELETE" })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Delete failed")
  }
}

export async function reprocessVideo(id: string): Promise<StatusResponse> {
  const resp = await fetch(`/api/video/${id}/reprocess`, { method: "POST" })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Reprocess failed")
  }
  return resp.json()
}

export async function fetchSeries(): Promise<SeriesItem[]> {
  const resp = await fetch("/api/series")
  if (!resp.ok) throw new Error("Failed to fetch series")
  return resp.json()
}

export async function fetchSeriesDetail(id: string): Promise<SeriesDetail> {
  const resp = await fetch(`/api/series/${id}`)
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to fetch series")
  }
  return resp.json()
}

export async function createSeries(
  title: string,
  description?: string,
  videoIds?: string[]
): Promise<SeriesDetail> {
  const resp = await fetch("/api/series", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ title, description, video_ids: videoIds }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to create series")
  }
  return resp.json()
}

export async function updateSeries(
  id: string,
  updates: { title?: string; description?: string; cover_video_id?: string }
): Promise<SeriesDetail> {
  const resp = await fetch(`/api/series/${id}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(updates),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to update series")
  }
  return resp.json()
}

export async function deleteSeries(id: string): Promise<void> {
  const resp = await fetch(`/api/series/${id}`, { method: "DELETE" })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to delete series")
  }
}

export async function addVideosToSeries(
  seriesId: string,
  videoIds: string[]
): Promise<SeriesDetail> {
  const resp = await fetch(`/api/series/${seriesId}/videos`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ video_ids: videoIds }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to add videos to series")
  }
  return resp.json()
}

export async function removeVideoFromSeries(
  seriesId: string,
  videoId: string
): Promise<SeriesDetail> {
  const resp = await fetch(`/api/series/${seriesId}/videos/${videoId}`, {
    method: "DELETE",
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to remove video from series")
  }
  return resp.json()
}

export async function reorderSeriesEpisodes(
  seriesId: string,
  videoIds: string[]
): Promise<SeriesDetail> {
  const resp = await fetch(`/api/series/${seriesId}/reorder`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ video_ids: videoIds }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to reorder episodes")
  }
  return resp.json()
}

export async function updateWatchProgress(
  videoId: string,
  positionSeconds: number,
  completed?: boolean
): Promise<WatchProgress> {
  const resp = await fetch(`/api/video/${videoId}/progress`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ position_seconds: positionSeconds, completed }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to update progress")
  }
  return resp.json()
}

export async function getWatchProgress(videoId: string): Promise<WatchProgress> {
  const resp = await fetch(`/api/video/${videoId}/progress`)
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to get progress")
  }
  return resp.json()
}

export async function bulkDeleteVideos(videoIds: string[]): Promise<{ ok: boolean; deleted: number }> {
  const resp = await fetch("/api/videos/bulk/delete", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ video_ids: videoIds }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to delete videos")
  }
  return resp.json()
}

export async function bulkAddToSeries(
  videoIds: string[],
  seriesId: string
): Promise<SeriesDetail> {
  const resp = await fetch("/api/videos/bulk/add-to-series", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ video_ids: videoIds, series_id: seriesId }),
  })
  if (!resp.ok) {
    const data = await resp.json().catch(() => ({}))
    throw new Error(data.message || "Failed to add videos to series")
  }
  return resp.json()
}
