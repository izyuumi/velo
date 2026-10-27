import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import "./index.css"
import App from "./App.tsx"
import { appUrl } from "@/lib/url"

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register(appUrl("/sw.js"), { scope: appUrl("/") }).catch(console.error)
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
