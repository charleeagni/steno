import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import OverlayApp from "./OverlayApp";
import "./styles.css";

function resolveCurrentWindowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

const currentWindowLabel = resolveCurrentWindowLabel();

if (currentWindowLabel === "overlay") {
  document.documentElement.classList.add("overlay-body");
  document.body.classList.add("overlay-body");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {currentWindowLabel === "overlay" ? <OverlayApp /> : <App />}
  </React.StrictMode>,
);
