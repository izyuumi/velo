import { BrowserRouter, Routes, Route } from "react-router-dom"
import { Layout } from "@/components/layout"
import { IndexPage } from "@/pages/index"
import { UploadPage } from "@/pages/upload"
import { WatchPage } from "@/pages/watch"
import { NotFoundPage } from "@/pages/not-found"

function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<IndexPage />} />
          <Route path="upload" element={<UploadPage />} />
          <Route path="watch/:id" element={<WatchPage />} />
          <Route path="*" element={<NotFoundPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}

export default App
