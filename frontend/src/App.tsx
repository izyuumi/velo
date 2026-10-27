import { BrowserRouter, Routes, Route } from "react-router-dom"
import { Layout } from "@/components/layout"
import { IndexPage } from "@/pages/index"
import { UploadPage } from "@/pages/upload"
import { WatchPage } from "@/pages/watch"
import { SeriesPage } from "@/pages/series"
import { SeriesManagePage } from "@/pages/series-manage"
import { NotFoundPage } from "@/pages/not-found"
import { DownloadedPage } from "@/pages/downloaded"

function App() {
  return (
    <BrowserRouter basename={import.meta.env.BASE_URL}>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<IndexPage />} />
          <Route path="upload" element={<UploadPage />} />
          <Route path="downloaded" element={<DownloadedPage />} />
          <Route path="watch/:id" element={<WatchPage />} />
          <Route path="series/:id" element={<SeriesPage />} />
          <Route path="series/:id/manage" element={<SeriesManagePage />} />
          <Route path="*" element={<NotFoundPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}

export default App
