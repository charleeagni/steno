import React from "react";
import ReactDOM from "react-dom/client";

window.addEventListener("error", (e) => {
  const el = document.createElement("div");
  el.style.position = "absolute";
  el.style.top = "0";
  el.style.left = "0";
  el.style.zIndex = "9999";
  el.style.backgroundColor = "rgba(255,0,0,0.8)";
  el.style.color = "white";
  el.style.padding = "20px";
  el.innerText = `Global Error: ${e.message}\n${e.filename}:${e.lineno}`;
  document.body.appendChild(el);
});

window.addEventListener("unhandledrejection", (e) => {
  const el = document.createElement("div");
  el.style.position = "absolute";
  el.style.top = "50px";
  el.style.left = "0";
  el.style.zIndex = "9999";
  el.style.backgroundColor = "rgba(255,100,0,0.8)";
  el.style.color = "white";
  el.style.padding = "20px";
  el.innerText = `Unhandled Promise: ${e.reason}`;
  document.body.appendChild(el);
});

import App from "./App";
import OverlayApp from "./OverlayApp";
import "./styles.css";

function shouldRenderOverlay(): boolean {
  const params = new URLSearchParams(window.location.search);
  if (params.get("overlay") === "1") {
    return true;
  }

  return false;
}

const renderOverlay = shouldRenderOverlay();

if (renderOverlay) {
  document.documentElement.classList.add("overlay-body");
  document.body.classList.add("overlay-body");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {renderOverlay ? <OverlayApp /> : <App />}
  </React.StrictMode>,
);
