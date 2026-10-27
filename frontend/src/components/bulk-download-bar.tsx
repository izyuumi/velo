import { Loader2, X, AlertCircle } from "lucide-react"
import { Card } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import type { BulkProgress } from "@/hooks/use-bulk-download"

interface Props {
  progress: BulkProgress | null
  error: string | null
  cancel: () => void
}

export function BulkDownloadBar({ progress, error, cancel }: Props) {
  if (error) {
    return (
      <Card className="p-3 flex items-center gap-2 border-destructive/50 text-sm text-destructive">
        <AlertCircle className="h-4 w-4" />
        {error}
      </Card>
    )
  }
  if (!progress) return null
  const overall = ((progress.done + progress.currentPct) / progress.total) * 100
  return (
    <Card className="p-3 space-y-2">
      <div className="flex items-center gap-2 text-sm">
        <Loader2 className="h-4 w-4 animate-spin" />
        <span className="flex-1 truncate">
          Downloading {progress.done + 1}/{progress.total}: {progress.current}
        </span>
        <Button variant="ghost" size="icon" onClick={cancel} title="Cancel">
          <X className="h-4 w-4" />
        </Button>
      </div>
      <Progress value={overall} className="h-1.5" />
    </Card>
  )
}
