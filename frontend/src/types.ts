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
  series_id?: string
  episode_number?: number
  watch_position?: number
  watch_completed: boolean
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
  series_id?: string
  series_title?: string
  episode_number?: number
  next_episode_id?: string
  prev_episode_id?: string
  watch_position?: number
  watch_completed: boolean
}

export interface UploadResult {
  id: string
  title: string
  watch_url: string
}

export interface ApiError {
  message: string
}

export interface SeriesItem {
  id: string
  title: string
  description?: string
  cover_video_id?: string
  cover_thumbnail?: string
  episode_count: number
  total_duration?: number
  created_at: number
  updated_at: number
  watched_count: number
  in_progress_count: number
}

export interface SeriesDetail {
  id: string
  title: string
  description?: string
  cover_video_id?: string
  episodes: VideoItem[]
  created_at: number
  updated_at: number
}

export interface WatchProgress {
  video_id: string
  position_seconds: number
  completed: boolean
}
