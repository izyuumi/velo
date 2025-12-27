export interface VideoItem {
  id: string
  title: string
  status: "processing" | "ready" | "error"
  progress?: number
  error?: string
  thumbnail?: string
  duration_seconds?: number
  width?: number
  height?: number
  created_at: number
}

export interface StatusResponse {
  id: string
  status: "processing" | "ready" | "error"
  progress?: number
  error?: string
  title: string
  watch_url: string
  duration_seconds?: number
  width?: number
  height?: number
  created_at: number
  retries: number
  upload_available: boolean
}

export interface UploadResult {
  id: string
  title: string
  watch_url: string
}

export interface ApiError {
  message: string
}

