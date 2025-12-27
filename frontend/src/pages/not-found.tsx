import { Link } from "react-router-dom"
import { Home, Upload } from "lucide-react"
import { Button } from "@/components/ui/button"

export function NotFoundPage() {
  return (
    <div className="flex flex-col items-center justify-center min-h-[60vh] gap-4">
      <div className="text-center">
        <p className="text-sm text-muted-foreground">404</p>
        <h1 className="text-2xl font-bold mt-1">Page Not Found</h1>
        <p className="text-muted-foreground mt-2">
          The page you're looking for doesn't exist.
        </p>
      </div>
      <div className="flex gap-2">
        <Button asChild>
          <Link to="/">
            <Home className="h-4 w-4" />
            Home
          </Link>
        </Button>
        <Button variant="outline" asChild>
          <Link to="/upload">
            <Upload className="h-4 w-4" />
            Upload
          </Link>
        </Button>
      </div>
    </div>
  )
}
