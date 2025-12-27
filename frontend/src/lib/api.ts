import type { StatusResponse, UploadResult, VideoItem } from "@/types"

export async function fetchVideos(): Promise<VideoItem[]> {
  const resp = await fetch("/api/videos")
  if (!resp.ok) throw new Error("Failed to fetch videos")
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

