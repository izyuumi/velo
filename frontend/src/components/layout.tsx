import { Link, Outlet } from "react-router-dom"
import { Upload } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useEffect } from "react"

export function Layout() {
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const apply = (dark: boolean) => {
      document.documentElement.classList.toggle("dark", dark)
    }
    apply(mq.matches)
    const handler = (e: MediaQueryListEvent) => apply(e.matches)
    mq.addEventListener("change", handler)
    return () => mq.removeEventListener("change", handler)
  }, [])

  return (
    <div className="min-h-screen bg-background">
      <header className="sticky top-0 z-50 w-full border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
        <div className="container flex h-14 max-w-7xl items-center mx-auto px-4">
          <Link to="/" className="mr-6 flex items-center space-x-2">
            <span className="font-bold">Velo</span>
          </Link>
          <div className="flex flex-1 items-center justify-end space-x-2">
            <Button asChild size="sm">
              <Link to="/upload">
                <Upload className="h-4 w-4" />
                Upload
              </Link>
            </Button>
          </div>
        </div>
      </header>
      <main className="container max-w-7xl mx-auto px-4 py-6">
        <Outlet />
      </main>
    </div>
  )
}
